//! `TimescaleDB` chunk skipping: per-chunk ranges of a non-partitioning column
//! that let the planner exclude a converted chunk a predicate on that column
//! cannot match.
//!
//! A column is registered once per hypertable with `enable_chunk_skipping`,
//! and a chunk's range is computed when the chunk is converted to the
//! columnstore. The catalogue lists a registered column on a row of its own
//! with no chunk, beside one row per chunk.

use crate::{
    cast::cast,
    columnstore_catalog::{CatalogHypertable, TimescaleCatalog},
    pg_type::PostgresType,
};
use sea_query::{Expr, ExprTrait, Iden, Query, SelectStatement, SimpleExpr};

/// The `_timescaledb_catalog.chunk_column_stats` table.
#[derive(Iden, Clone, Copy)]
enum ChunkColumnStats {
    /// Table name: `chunk_column_stats`
    #[iden = "chunk_column_stats"]
    Table,
    /// The hypertable the column belongs to
    HypertableId,
    /// The chunk the range belongs to, NULL on the row registering the column
    ChunkId,
    /// The column the range is kept for
    ColumnName,
}

/// The `column_name` projection of [`chunk_skipping_columns_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "column_name"]
pub struct SkippingColumn;

/// The columns registered for chunk skipping on `hypertable`, one row each.
///
/// Projects `column_name` as `text`. The hypertable name is a fixed catalogue
/// name bound as a value.
#[must_use]
pub fn chunk_skipping_columns_statement(hypertable: &str) -> SelectStatement {
    Query::select()
        .expr_as(
            cast(Expr::col((ChunkColumnStats::Table, ChunkColumnStats::ColumnName)), PostgresType::Text),
            SkippingColumn,
        )
        .from((TimescaleCatalog, ChunkColumnStats::Table))
        .inner_join(
            (TimescaleCatalog, CatalogHypertable::Table),
            Expr::col((CatalogHypertable::Table, CatalogHypertable::Id)).equals((ChunkColumnStats::Table, ChunkColumnStats::HypertableId)),
        )
        .and_where(Expr::col((CatalogHypertable::Table, CatalogHypertable::TableName)).eq(hypertable))
        .and_where(Expr::col((ChunkColumnStats::Table, ChunkColumnStats::ChunkId)).is_null())
        .to_owned()
}

/// `enable_chunk_skipping(<hypertable>::regclass, <column>::name)`: register
/// `column` of `hypertable` for chunk skipping.
///
/// Both names are bound values. Only chunks converted afterwards gain a range;
/// a chunk without one is never excluded.
#[must_use]
pub fn enable_chunk_skipping(hypertable: &str, column: &str) -> SimpleExpr {
    Expr::cust_with_exprs(
        "enable_chunk_skipping($1, $2)",
        [cast(Expr::val(hypertable), PostgresType::Regclass), cast(Expr::val(column), PostgresType::Name)],
    )
}

#[cfg(test)]
mod tests {
    use crate::chunk_skipping::{chunk_skipping_columns_statement, enable_chunk_skipping};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn the_registered_columns_are_the_rows_without_a_chunk_of_the_named_hypertable() {
        let (sql, values) = chunk_skipping_columns_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "chunk_column_stats"."column_name"::text AS "column_name" FROM "_timescaledb_catalog"."chunk_column_stats" "#,
                r#"INNER JOIN "_timescaledb_catalog"."hypertable" ON "hypertable"."id" = "chunk_column_stats"."hypertable_id" "#,
                r#"WHERE "hypertable"."table_name" = $1 AND "chunk_column_stats"."chunk_id" IS NULL"#
            )
        );
        assert_eq!(values.0.len(), 1, "{sql}");
    }

    #[test]
    fn a_column_is_registered_with_both_names_bound() {
        let (sql, values) = Query::select()
            .expr(enable_chunk_skipping("attribute_history", "observed_at"))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT enable_chunk_skipping($1::regclass, $2::name)");
        assert_eq!(values.0.len(), 2, "{sql}");
    }
}
