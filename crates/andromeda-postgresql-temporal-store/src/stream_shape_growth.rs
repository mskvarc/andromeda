//! Adding the combinations of family and value shape recorded instances carry
//! to the shape masks of their streams.
//!
//! Every path that records an instance (the application of a queued intent,
//! a clause 5.6.14 modification) grows the mask of the instance's stream by
//! the combination the stored row carries, read through the projection an
//! aggregation reads, so the mask names exactly the shapes the rows carry. A
//! mask already holding every recorded combination is left alone, which is
//! the steady state of a stream: the write then updates no catalogue row.

use andromeda_postgresql_query::tombstone::carries_a_value;
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams};
use andromeda_postgresql_seaquery::bit_string::{bit_union, bit_union_of_rows};
use andromeda_postgresql_temporal_query::{alias::Ah, shape_mask::recorded_combination};
use sea_query::{Expr, ExprTrait, Iden, IntoIden, Query, SelectStatement, SimpleExpr, TableRef, UpdateStatement};

/// The alias of the combinations each stream's rows carry, `grown`.
#[derive(Iden, Clone, Copy)]
#[iden = "grown"]
struct Grown;

/// Grow the mask of every stream of the `table` catalogue that a row of
/// `rows` belongs to by the combinations those rows carry.
///
/// `rows` names its history-shaped rows `ah` and selects nothing yet: this
/// projects each stream's key and the union of its rows' combinations.
/// Tombstones carry no value and add no combination.
pub(crate) fn shape_growth_statement(table: TableRef, mut rows: SelectStatement) -> UpdateStatement {
    rows.column((Ah, AttributeHistory::StreamKey))
        .expr_as(bit_union_of_rows(recorded_combination()), TemporalAttributeStreams::ShapeMask)
        .and_where(carries_a_value(Ah))
        .group_by_col((Ah, AttributeHistory::StreamKey));
    let mask = || SimpleExpr::from(Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::ShapeMask)));
    let grown = || SimpleExpr::from(Expr::col((Grown, TemporalAttributeStreams::ShapeMask)));
    Query::update()
        .table(table)
        .value(TemporalAttributeStreams::ShapeMask, bit_union(mask(), grown()))
        .from(TableRef::SubQuery(Box::new(rows), Grown.into_iden()))
        .and_where(Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).equals((Grown, AttributeHistory::StreamKey)))
        .and_where(bit_union(mask(), grown()).ne(mask()))
        .take()
}

#[cfg(test)]
mod tests {
    use crate::stream_shape_growth::shape_growth_statement;
    use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams};
    use andromeda_postgresql_temporal_query::alias::Ah;
    use sea_query::{PostgresQueryBuilder, Query, TableRef};

    fn rendered() -> String {
        let rows = Query::select().from_as(AttributeHistory::Table, Ah).take();
        shape_growth_statement(TableRef::from(TemporalAttributeStreams::Table), rows).to_string(PostgresQueryBuilder)
    }

    /// The mask grows by the union of the combinations the stream's rows
    /// carry, read off the refined shape an aggregation projects.
    #[test]
    fn each_stream_grows_by_the_union_of_its_rows_combinations() {
        let sql = rendered();
        assert!(
            sql.starts_with(r#"UPDATE "temporal_attribute_streams" SET "shape_mask" = "temporal_attribute_streams"."shape_mask" | "grown"."shape_mask" FROM (SELECT "ah"."stream_key", bit_or((B'1'::bit(80) >> ((("ah"."attribute_kind" - 1) * (10)) + ((CASE"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"WHERE "ah"."value_kind" <> (0) GROUP BY "ah"."stream_key") AS "grown""#),
            "{sql}"
        );
    }

    /// A mask already holding every combination its rows carry is left
    /// alone, so the steady state of a stream writes no catalogue row.
    #[test]
    fn a_mask_holding_every_recorded_combination_is_not_rewritten() {
        let sql = rendered();
        assert!(
            sql.ends_with(concat!(
                r#"WHERE "temporal_attribute_streams"."stream_key" = "grown"."stream_key""#,
                r#" AND ("temporal_attribute_streams"."shape_mask" | "grown"."shape_mask") <> "temporal_attribute_streams"."shape_mask""#
            )),
            "{sql}"
        );
    }
}
