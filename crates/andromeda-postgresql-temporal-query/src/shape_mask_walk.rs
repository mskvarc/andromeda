//! The distinct shape masks the streams of named attributes hold, read by
//! walking the catalogue's `(attribute_name_key, shape_mask)` index.
//!
//! Most streams of an attribute hold the same few combinations of family and
//! shape, so an attribute holds few distinct masks however many streams carry
//! it. A loose index scan reads them in one descent each: for each attribute
//! the walk takes the lowest mask its index range holds, then repeatedly the
//! lowest above the one it found, until none is left. Ordering each step by
//! the index key keeps it on the index; probing the masks a combination needs
//! leaves the plan to an estimate the planner cannot make for a bound mask.

use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_schema::{name_dictionary::AttributeNames, temporal::TemporalAttributeStreams};
use andromeda_postgresql_seaquery::{branch::qualified, pg_array::is_any_of};
use sea_query::{CommonTableExpression, DynIden, Expr, ExprTrait, Iden, IntoIden, JoinType, Order, Query, SelectStatement, SimpleExpr, UnionType, WithClause};

/// The recursive table the walk builds, `held`: one row per distinct mask an
/// attribute holds, under [`TemporalAttributeStreams::AttributeNameKey`] and
/// [`TemporalAttributeStreams::ShapeMask`].
#[derive(Iden, Clone, Copy)]
#[iden = "held"]
pub struct HeldMasks;

/// Alias `an`: a named attribute the walk starts from.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
struct Named;

/// Alias `ms`: a stream row one step of the walk reads.
#[derive(Iden, Clone, Copy)]
#[iden = "ms"]
struct MaskedStream;

/// Alias `step`: the next mask of one attribute the walk finds.
#[derive(Iden, Clone, Copy)]
#[iden = "step"]
struct Step;

/// The two columns the walk carries.
const COLUMNS: [TemporalAttributeStreams; 2] = [TemporalAttributeStreams::AttributeNameKey, TemporalAttributeStreams::ShapeMask];

/// `WITH RECURSIVE held (attribute_name_key, shape_mask)`: every distinct mask
/// the streams of `branch` hold for the attributes keyed `keys`, leaving out
/// the streams whose keys `excluded` selects.
#[must_use]
pub fn held_masks(branch: &HistoryBranch, keys: &[i32], excluded: Option<&SelectStatement>) -> WithClause {
    let mut seed = Query::select();
    seed.columns(COLUMNS.map(|column| (Step, column)))
        .from_as(AttributeNames::Table, Named)
        .join_lateral(
            JoinType::Join,
            next_mask(branch, Expr::col((Named, AttributeNames::AttributeNameKey)), None, excluded),
            Step,
            SimpleExpr::from(true),
        )
        .and_where(is_any_of(Expr::col((Named, AttributeNames::AttributeNameKey)), keys.iter().copied()));
    let mut walk = Query::select();
    walk.columns(COLUMNS.map(|column| (Step, column))).from(HeldMasks).join_lateral(
        JoinType::Join,
        next_mask(
            branch,
            Expr::col((HeldMasks, TemporalAttributeStreams::AttributeNameKey)),
            Some(HeldMasks.into_iden()),
            excluded,
        ),
        Step,
        SimpleExpr::from(true),
    );
    seed.union(UnionType::All, walk);
    let held = CommonTableExpression::new()
        .table_name(HeldMasks)
        .columns(COLUMNS.map(IntoIden::into_iden))
        .query(seed)
        .to_owned();
    WithClause::new().recursive(true).cte(held).to_owned()
}

/// The lowest mask the attribute `attribute_key` holds among the streams of
/// `branch`, above the one the row `after` names when it names one, leaving
/// out the streams `excluded` selects.
fn next_mask(branch: &HistoryBranch, attribute_key: SimpleExpr, after: Option<DynIden>, excluded: Option<&SelectStatement>) -> SelectStatement {
    let mut step = Query::select();
    step.columns(COLUMNS.map(|column| (MaskedStream, column)))
        .from(qualified(branch.schema(), TemporalAttributeStreams::Table).alias(MaskedStream))
        .and_where(Expr::col((MaskedStream, TemporalAttributeStreams::AttributeNameKey)).eq(attribute_key));
    if let Some(after) = after {
        step.and_where(Expr::col((MaskedStream, TemporalAttributeStreams::ShapeMask)).gt(Expr::col((after, TemporalAttributeStreams::ShapeMask))));
    }
    if let Some(excluded) = excluded {
        // clone: the seed and every step leave out the same streams, so each takes
        // its own copy of the borrowed subquery.
        step.and_where(Expr::col((MaskedStream, TemporalAttributeStreams::StreamKey)).not_in_subquery(excluded.clone()));
    }
    step.order_by((MaskedStream, TemporalAttributeStreams::ShapeMask), Order::Asc).limit(1).take()
}

#[cfg(test)]
mod tests {
    use crate::shape_mask_walk::{HeldMasks, held_masks};
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Asterisk, Expr, IntoIden, PostgresQueryBuilder, Query, SelectStatement};

    fn rendered(branch: &HistoryBranch, excluded: Option<&SelectStatement>) -> String {
        Query::select()
            .column(Asterisk)
            .from(HeldMasks)
            .with_cte(held_masks(branch, &[7], excluded))
            .to_string(PostgresQueryBuilder)
    }

    /// Each step is one descent of the attribute's index range, ordered by
    /// the index key, rather than a read of every stream of the attribute.
    #[test]
    fn the_catalogue_is_walked_one_distinct_mask_at_a_time() {
        let sql = rendered(&HistoryBranch::live_recorded(), None);
        assert_eq!(
            sql,
            concat!(
                r#"WITH RECURSIVE "held" ("attribute_name_key", "shape_mask") AS (SELECT "step"."attribute_name_key", "step"."shape_mask" FROM "attribute_names" AS "an""#,
                r#" JOIN LATERAL (SELECT "ms"."attribute_name_key", "ms"."shape_mask" FROM "temporal_attribute_streams" AS "ms" WHERE "ms"."attribute_name_key" = "an"."attribute_name_key" ORDER BY "ms"."shape_mask" ASC LIMIT 1) AS "step" ON TRUE"#,
                r#" WHERE "an"."attribute_name_key" = 7"#,
                r#" UNION ALL (SELECT "step"."attribute_name_key", "step"."shape_mask" FROM "held""#,
                r#" JOIN LATERAL (SELECT "ms"."attribute_name_key", "ms"."shape_mask" FROM "temporal_attribute_streams" AS "ms" WHERE "ms"."attribute_name_key" = "held"."attribute_name_key" AND "ms"."shape_mask" > "held"."shape_mask" ORDER BY "ms"."shape_mask" ASC LIMIT 1) AS "step" ON TRUE))"#,
                r#" SELECT * FROM "held""#
            )
        );
    }

    #[test]
    fn a_snapshot_branch_walks_its_own_catalogue() {
        let sql = rendered(&HistoryBranch::recorded_in(Alias::new("snap_1").into_iden()), None);
        assert_eq!(sql.matches(r#"FROM "snap_1"."temporal_attribute_streams" AS "ms""#).count(), 2, "{sql}");
    }

    #[test]
    fn the_excluded_streams_are_left_out_of_every_step() {
        let excluded = Query::select().expr(Expr::val(4)).to_owned();
        let sql = rendered(&HistoryBranch::live_recorded(), Some(&excluded));
        assert_eq!(sql.matches(r#""ms"."stream_key" NOT IN (SELECT 4)"#).count(), 2, "{sql}");
    }
}
