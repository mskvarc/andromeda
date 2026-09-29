//! The clause 5.7.5-5.7.10 discovery answers, read from the census ledgers.
//!
//! A count is the sum of its ledger rows, and a key exists when that sum is
//! positive: every read groups a ledger by its key and keeps the groups
//! `HAVING SUM(delta) > 0`, so a type whose last Entity was deleted, or an
//! attribute whose last instance was removed, is simply absent. Nothing here
//! reads a payload or an Entity row; the answers are the size of the
//! dictionaries, not of the store. The names come from the dictionaries by key.
//!
//! Each endpoint that combines two ledgers — a type's count and its
//! attributes, an attribute's count and its families and types — reads both in
//! one statement, so the answer comes from one snapshot and can never pair a
//! count with attributes from a different moment.

use crate::error::ContextStoreError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, attribute_kind_code::attribute_type_of};
use andromeda_postgresql_schema::{
    census::{EntityTypeAttributeCensus, EntityTypeCensus},
    name_dictionary::{AttributeNames, EntityTypeNames},
};
use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
use sea_query::{Cond, Expr, ExprTrait, Func, Iden, JoinType, Query, SelectStatement, SimpleExpr};

/// Clauses 5.7.8 to 5.7.10: the attribute answers.
mod attributes;
/// Clauses 5.7.5 to 5.7.7: the entity type answers.
mod entity_types;

/// The live types of `entity_type_census`, `c`.
#[derive(Iden, Clone, Copy)]
#[iden = "c"]
pub(super) struct LiveTypes;

/// The live (type, attribute, family) keys of `entity_type_attribute_census`, `a`.
#[derive(Iden, Clone, Copy)]
#[iden = "a"]
pub(super) struct LiveInstances;

/// The one-row count a type or attribute answer is anchored on, `total`.
#[derive(Iden, Clone, Copy)]
#[iden = "total"]
pub(super) struct Total;

/// The count column of [`Total`].
#[derive(Iden, Clone, Copy)]
#[iden = "count"]
pub(super) struct Count;

/// The `entity_type_names` row a type key is read back through, `n`.
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
pub(super) struct TypeName;

/// The `attribute_names` row an attribute key is read back through, `an`.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
pub(super) struct AttributeNameRow;

/// The aggregated name array an answer reports, `names`.
#[derive(Iden, Clone, Copy)]
#[iden = "names"]
pub(super) struct Names;

/// The sum of a ledger's `delta` column, as the `bigint` every count is read as.
pub(super) fn summed(delta: SimpleExpr) -> SimpleExpr {
    cast(Func::sum(delta).into(), PostgresType::Bigint)
}

/// `SELECT entity_type_name_key FROM entity_type_census GROUP BY 1 HAVING
/// SUM(entity_delta) > 0`: every type at least one Entity carries.
pub(super) fn live_types(target: &ContextTarget) -> SelectStatement {
    Query::select()
        .column((EntityTypeCensus::Table, EntityTypeCensus::EntityTypeNameKey))
        .from(table_of(target, EntityTypeCensus::Table))
        .group_by_col((EntityTypeCensus::Table, EntityTypeCensus::EntityTypeNameKey))
        .and_having(Func::sum(Expr::col((EntityTypeCensus::Table, EntityTypeCensus::EntityDelta))).gt(0))
        .take()
}

/// Every (type, attribute, family) at least one instance has on an Entity of
/// the type, narrowed by `narrowing`.
pub(super) fn live_instances(target: &ContextTarget, narrowing: Cond) -> SelectStatement {
    let column = |column: EntityTypeAttributeCensus| Expr::col((EntityTypeAttributeCensus::Table, column));
    Query::select()
        .column((EntityTypeAttributeCensus::Table, EntityTypeAttributeCensus::EntityTypeNameKey))
        .column((EntityTypeAttributeCensus::Table, EntityTypeAttributeCensus::AttributeNameKey))
        .column((EntityTypeAttributeCensus::Table, EntityTypeAttributeCensus::AttributeKind))
        .from(table_of(target, EntityTypeAttributeCensus::Table))
        .cond_where(narrowing)
        .group_by_col((EntityTypeAttributeCensus::Table, EntityTypeAttributeCensus::EntityTypeNameKey))
        .group_by_col((EntityTypeAttributeCensus::Table, EntityTypeAttributeCensus::AttributeNameKey))
        .group_by_col((EntityTypeAttributeCensus::Table, EntityTypeAttributeCensus::AttributeKind))
        .and_having(Func::sum(column(EntityTypeAttributeCensus::InstanceDelta)).gt(0))
        .take()
}

/// `JOIN entity_type_names n ON n.entity_type_name_key = <key>`.
pub(super) fn join_type_name(statement: &mut SelectStatement, join: JoinType, key: SimpleExpr) {
    statement.join_as(
        join,
        EntityTypeNames::Table,
        TypeName,
        Expr::col((TypeName, EntityTypeNames::EntityTypeNameKey)).eq(key),
    );
}

/// `JOIN attribute_names an ON an.attribute_name_key = <key>`.
pub(super) fn join_attribute_name(statement: &mut SelectStatement, join: JoinType, key: SimpleExpr) {
    statement.join_as(
        join,
        AttributeNames::Table,
        AttributeNameRow,
        Expr::col((AttributeNameRow, AttributeNames::AttributeNameKey)).eq(key),
    );
}

/// The live key of a type in [`LiveTypes`] or [`LiveInstances`].
pub(super) fn type_key_of(source: impl Iden + 'static) -> SimpleExpr {
    Expr::col((source, EntityTypeCensus::EntityTypeNameKey))
}

/// The family a stored census code names.
pub(super) fn stored_family(code: i16) -> Result<AttributeType, ContextStoreError> {
    AttributeKind::try_from(code)
        .map(attribute_type_of)
        .map_err(|error| ContextStoreError::Internal(format!("stored attribute family is not a known code: {error}")))
}

/// A summed count read back as the unsigned count it is: a ledger never sums
/// below zero, so a negative sum is a census that drifted from its Entities.
pub(super) fn stored_count(sum: i64) -> Result<u64, ContextStoreError> {
    u64::try_from(sum).map_err(|_| ContextStoreError::Internal(format!("the discovery census sums to {sum}, below zero")))
}
