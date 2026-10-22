//! Group-key projection for the `aggrGroupBy` temporal aggregation grouping
//! extension.
//!
//! Clause 4.5.19 fixes the aggregation key at `(attribute, datasetId, period)`.
//! The extension partitions each of those groups further, by one or more
//! grouping terms. Every term adds a column to the statement's `GROUP BY` and
//! `ORDER BY`, ahead of the period so a group's periods stay contiguous:
//!
//! - `datasetId` is already a key column (clause 4.5.5), so it splits nothing
//!   new and the reader reads the value from the row it already decodes.
//! - `unitCode` and the attribute family (`type`) become key columns, which is
//!   what turns clause 4.5.2's mixed-unit refusal into one group per unit; the
//!   [`crate::aggregation_group`] guard is told the facet is promoted so it no
//!   longer treats the (now-uniform) span as a contradiction.
//! - `id` and `scope` are per-entity constants within one entity's temporal
//!   representation, so they add no key column and the reader supplies the
//!   value; they only become interesting under a cross-entity resource.
//! - A calendar or cyclic bucket keys on the truncated or folded time axis.
//! - A value-in-effect key correlates the attribute's value at each instant
//!   through a `JOIN LATERAL` selecting the instance whose axis value is the
//!   greatest not after the aggregated row's (the "in effect at t" rule). It
//!   keys on two columns: the value as text, and a `DateTime` value as the
//!   instant, which the reader spells as every emitted instant is spelled.

use crate::{aggregation_group::PromotedGroupFacets, alias::Ah, value_text::value_text};
use andromeda_ngsi_ld_query::temporal::grouping::{
    AggregationGrouping,
    bucket_term::{BucketKind, BucketTerm},
    entity_member::EntityMember,
    group_key::GroupKey,
};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::query_names::QueryNames,
    history_source::{HistoryBranch, HistoryOf, attribute_rows::attribute_rows, grain::HistoryGrain},
    tombstone::carries_a_value,
};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    date_field::{date_part, date_trunc},
    literal::false_literal,
};
use sea_query::{Alias, Expr, ExprTrait, JoinType, Order, Query, SelectStatement, SimpleExpr};

/// Which facets an `aggrGroupBy` promotes to group keys, for the mixed-group
/// guard, computed from the grouping alone so the SQL builder and the reader
/// agree without one depending on the other.
#[must_use]
pub fn promoted_facets(grouping: &AggregationGrouping) -> PromotedGroupFacets {
    let mut promoted = PromotedGroupFacets::default();
    for term in grouping.terms() {
        match &term.key {
            GroupKey::Member(EntityMember::UnitCode) => promoted.units = true,
            GroupKey::Member(EntityMember::Type) => promoted.families = true,
            GroupKey::Member(EntityMember::DatasetId | EntityMember::Id | EntityMember::Scope) | GroupKey::Path(_) => {}
        }
    }
    promoted
}

/// The projected column name grouping term `index` is read back under.
#[must_use]
pub fn group_key_column(index: usize) -> String {
    format!("group_key_{index}")
}

/// The projected column a value-in-effect key of term `index` holding a
/// `DateTime` is read back under, beside [`group_key_column`].
///
/// An instant is projected as the instant, never as text, so the reader spells
/// it the way the broker spells every instant it emits; PostgreSQL's own text
/// rendering of a `timestamptz` carries a numeric offset.
#[must_use]
pub fn group_key_instant_column(index: usize) -> String {
    format!("group_key_{index}_instant")
}

/// The projected alias for grouping term `index`.
fn group_key_alias(index: usize) -> Alias {
    Alias::new(group_key_column(index))
}

/// The projected alias for the instant of grouping term `index`.
fn group_key_instant_alias(index: usize) -> Alias {
    Alias::new(group_key_instant_column(index))
}

/// The lateral-join alias a value-in-effect key of term `index` is read under.
fn value_key_alias(index: usize) -> Alias {
    Alias::new(format!("gk{index}"))
}

/// The attribute-history alias inside term `index`'s value-in-effect lateral.
fn value_key_history_alias(index: usize) -> Alias {
    Alias::new(format!("ahk{index}"))
}

/// The Postgres `date_trunc`/`date_part` field a calendar or cyclic bucket names.
const fn bucket_field(bucket: BucketTerm) -> &'static str {
    match bucket {
        BucketTerm::Year => "year",
        BucketTerm::Month | BucketTerm::MonthOfYear => "month",
        BucketTerm::Week => "week",
        BucketTerm::Day => "day",
        BucketTerm::Hour | BucketTerm::HourOfDay => "hour",
        BucketTerm::DayOfWeek => "dow",
    }
}

/// Project each `aggrGroupBy` term and return the keys it groups by, in term
/// order.
///
/// The caller groups and orders by the keys between the fixed key and the
/// period, so a group's periods stay contiguous. A term that adds no key
/// column (`datasetId`, `id`, `scope`) returns none.
pub fn apply_grouping(
    select: &mut SelectStatement,
    grouping: &AggregationGrouping,
    axis: AttributeHistory,
    branch: &HistoryBranch,
    of: &HistoryOf,
    names: &QueryNames,
) -> Vec<SimpleExpr> {
    let mut keys = Vec::with_capacity(grouping.terms().len());
    for (index, term) in grouping.terms().iter().enumerate() {
        match (&term.key, term.bucket) {
            (GroupKey::Member(EntityMember::DatasetId | EntityMember::Id | EntityMember::Scope), _) => {
                // datasetId already keys the group; id and scope are per-entity
                // constants. None adds a key column; the reader supplies the value.
            }
            (GroupKey::Member(EntityMember::UnitCode), _) => keys.push(Expr::col((Ah, AttributeHistory::UnitCodeId))),
            (GroupKey::Member(EntityMember::Type), _) => keys.push(Expr::col((Ah, AttributeHistory::AttributeKind))),
            (GroupKey::Path(_), Some(bucket)) => {
                let axis_value = Expr::col((Ah, axis));
                let expr = match bucket.kind() {
                    BucketKind::Calendar => date_trunc(bucket_field(bucket), axis_value),
                    BucketKind::Cyclic => date_part(bucket_field(bucket), axis_value),
                };
                // clone: the key is projected and grouped by.
                select.expr_as(expr.clone(), group_key_alias(index));
                keys.push(expr);
            }
            (GroupKey::Path(path), None) => {
                project_value_in_effect(select, index, names.attributes.get(&path.attr), axis, branch, of);
                for column in [group_key_alias(index), group_key_instant_alias(index)] {
                    // clone: the column is projected and grouped by.
                    select.expr_as(Expr::col((value_key_alias(index), column.clone())), column.clone());
                    keys.push(Expr::col((value_key_alias(index), column)));
                }
            }
        }
    }
    keys
}

/// Join the value in effect at each aggregated instant for one value key.
///
/// Reads the history of the aggregated entity, which `of` names: the instance
/// of the key attribute whose axis value is the greatest one not after the
/// aggregated row's axis value (clause 4.8; the "in effect at t" rule). A
/// tombstone carries no value, so it is excluded. The key attribute is the
/// dictionary key its name resolved to.
fn project_value_in_effect(
    select: &mut SelectStatement,
    index: usize,
    attribute: Option<AttributeNameKey>,
    axis: AttributeHistory,
    branch: &HistoryBranch,
    of: &HistoryOf,
) {
    let history = value_key_history_alias(index);
    // An attribute the dictionary never keyed was never written, so no value
    // of it is in effect anywhere.
    let of_attribute = attribute.map_or_else(false_literal, |key| Expr::col((history.clone(), HistoryGrain::AttributeNameKey)).eq(key.get()));
    // clone: the aliases are numbered per value key, and every clause naming one takes it by value.
    let mut lateral = Query::select();
    lateral
        .expr_as(value_text(history.clone()), group_key_alias(index))
        .expr_as(Expr::col((history.clone(), AttributeHistory::ValueDateTime)), group_key_instant_alias(index))
        .from(attribute_rows(branch, of, history.clone()))
        .and_where(of_attribute)
        .and_where(carries_a_value(history.clone()))
        .and_where(Expr::col((history.clone(), axis)).lte(Expr::col((Ah, axis))))
        .order_by((history, axis), Order::Desc)
        .limit(1);
    select.join_lateral(JoinType::LeftJoin, lateral.take(), value_key_alias(index), SimpleExpr::from(true));
}
