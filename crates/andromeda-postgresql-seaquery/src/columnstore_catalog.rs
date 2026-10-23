//! The `TimescaleDB` catalogue rows that hold a hypertable's columnstore
//! settings: its segment and order columns, its declared sparse indexes and
//! whether conversions merge chunks.
//!
//! Read from `_timescaledb_catalog` where no information view reports a value
//! exactly. Every hypertable name is bound as a value.

use crate::{
    cast::cast,
    jsonb::{jsonb_array_elements_text, jsonb_path_query, jsonb_path_query_array},
    pg_array::array_of_subquery,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    timescale::{Ht, Hypertables, TimescaleInformation},
};
use sea_query::{Alias, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, SimpleExpr};

/// The `_timescaledb_catalog` schema.
#[derive(Iden, Clone, Copy)]
#[iden = "_timescaledb_catalog"]
pub(crate) struct TimescaleCatalog;

/// The `_timescaledb_catalog.compression_settings` table.
///
/// Read rather than the `hypertable_columnstore_settings` information view
/// because it holds the segment and order columns as arrays of names with the
/// sort direction beside them, which compare exactly; the view renders them as
/// free text whose spacing and defaulted `NULLS` clauses vary by release.
#[derive(Iden, Clone, Copy)]
enum CompressionSettings {
    /// Table name: `compression_settings`
    #[iden = "compression_settings"]
    Table,
    /// The relation the settings belong to
    Relid,
    /// The segment-by column names
    Segmentby,
    /// The order-by column names
    Orderby,
    /// Whether each order-by column sorts descending
    OrderbyDesc,
    /// The sparse indexes, as a JSONB array of entries with `type`,
    /// `column` (one name, or an array of names for a composite) and `source`
    Index,
}

/// The `_timescaledb_catalog.hypertable` table.
#[derive(Iden, Clone, Copy)]
pub(crate) enum CatalogHypertable {
    /// Table name: `hypertable`
    #[iden = "hypertable"]
    Table,
    /// The hypertable's catalogue id
    Id,
    /// The hypertable's relation name
    TableName,
}

/// The `_timescaledb_catalog.dimension` table.
///
/// Read because no information view reports a dimension's
/// `compress_interval_length`, the span `timescaledb.compress_chunk_time_interval`
/// stores.
#[derive(Iden, Clone, Copy)]
enum CatalogDimension {
    /// Table name: `dimension`
    #[iden = "dimension"]
    Table,
    /// The hypertable the dimension partitions
    HypertableId,
    /// The span converted chunks are merged into, NULL when they are not
    CompressIntervalLength,
}

/// The `merges` projection of [`columnstore_merge_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "merges"]
pub struct ColumnstoreMerges;

/// The `kind` projection of [`columnstore_sparse_index_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "kind"]
pub struct SparseIndexKindColumn;

/// The `columns` projection of [`columnstore_sparse_index_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "columns"]
pub struct SparseIndexColumns;

/// Alias `entry` for one configured sparse index.
#[derive(Iden, Clone, Copy)]
#[iden = "entry"]
struct Entry;

/// Alias `cs` for the compression settings table.
#[derive(Iden, Clone, Copy)]
#[iden = "cs"]
struct Cs;

/// Whether the columnstore is enabled on `hypertable`, and with which segment
/// and order columns.
///
/// Projects `compression_enabled` from `timescaledb_information.hypertables`
/// and, left-joined, the `segmentby` and `orderby` column-name arrays (as
/// `text[]`) and the `orderby_desc` flags of the hypertable's own
/// `_timescaledb_catalog.compression_settings` row, which is absent until the
/// columnstore has been enabled. No row means `hypertable` is not a hypertable.
/// The hypertable name is a fixed catalogue name bound as a value.
#[must_use]
pub fn columnstore_settings_statement(hypertable: &str) -> SelectStatement {
    Query::select()
        .expr_as(Expr::col((Ht, Hypertables::CompressionEnabled)), Alias::new("compression_enabled"))
        .expr_as(
            cast(Expr::col((Cs, CompressionSettings::Segmentby)), PostgresType::TextArray),
            Alias::new("segmentby"),
        )
        .expr_as(
            cast(Expr::col((Cs, CompressionSettings::Orderby)), PostgresType::TextArray),
            Alias::new("orderby"),
        )
        .expr_as(Expr::col((Cs, CompressionSettings::OrderbyDesc)), Alias::new("orderby_desc"))
        .from_as((TimescaleInformation, Hypertables::Table), Ht)
        .join_as(
            JoinType::LeftJoin,
            (TimescaleCatalog, CompressionSettings::Table),
            Cs,
            Expr::col((Cs, CompressionSettings::Relid)).eq(cast(Expr::val(hypertable), PostgresType::Regclass)),
        )
        .and_where(Expr::col((Ht, Hypertables::HypertableName)).eq(hypertable))
        .to_owned()
}

/// Whether converting a chunk of `hypertable` merges it into a converted
/// neighbour.
///
/// Projects `merges`, true when a dimension of the hypertable stores a
/// `compress_interval_length`, which `timescaledb.compress_chunk_time_interval`
/// sets. The hypertable name is a fixed catalogue name bound as a value.
#[must_use]
pub fn columnstore_merge_statement(hypertable: &str) -> SelectStatement {
    let merging_dimension = Query::select()
        .expr(Expr::val(1))
        .from((TimescaleCatalog, CatalogDimension::Table))
        .inner_join(
            (TimescaleCatalog, CatalogHypertable::Table),
            Expr::col((CatalogHypertable::Table, CatalogHypertable::Id)).equals((CatalogDimension::Table, CatalogDimension::HypertableId)),
        )
        .and_where(Expr::col((CatalogHypertable::Table, CatalogHypertable::TableName)).eq(hypertable))
        .and_where(Expr::col((CatalogDimension::Table, CatalogDimension::CompressIntervalLength)).is_not_null())
        .to_owned();
    Query::select().expr_as(Expr::exists(merging_dimension), ColumnstoreMerges).to_owned()
}

/// The sparse indexes declared on `hypertable`'s columnstore, one row each.
///
/// Reads the `index` array of the hypertable's own
/// `_timescaledb_catalog.compression_settings` row and keeps the entries whose
/// `source` is `config`: the ones `timescaledb.sparse_index` declared, not the
/// summaries the order-by columns get regardless. Each row projects `kind`, the
/// summary type, and `columns`, its column names in declared order as a
/// `text[]`, one element for a single-column index. The lax `$.column[*]` path
/// reads a single name and a composite's array alike. The hypertable name is a
/// fixed catalogue name bound as a value.
#[must_use]
pub fn columnstore_sparse_index_statement(hypertable: &str) -> SelectStatement {
    let columns = Query::select()
        .expr(SimpleExpr::from(jsonb_array_elements_text(jsonb_path_query_array(
            Expr::col(Entry),
            "$.column[*]",
        ))))
        .to_owned();
    Query::select()
        .expr_as(SimpleExpr::from(Expr::col(Entry)).json_get_text(Expr::val("type")), SparseIndexKindColumn)
        .expr_as(array_of_subquery(columns), SparseIndexColumns)
        .from_as((TimescaleCatalog, CompressionSettings::Table), Cs)
        .from_function(
            jsonb_path_query(Expr::col((Cs, CompressionSettings::Index)), r#"$[*] ? (@.source == "config")"#),
            Entry,
        )
        .and_where(Expr::col((Cs, CompressionSettings::Relid)).eq(cast(Expr::val(hypertable), PostgresType::Regclass)))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::columnstore_catalog::{columnstore_merge_statement, columnstore_settings_statement, columnstore_sparse_index_statement};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_columnstore_settings_are_read_from_the_view_and_the_catalogue_arrays() {
        let (sql, values) = columnstore_settings_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "ht"."compression_enabled" AS "compression_enabled", "cs"."segmentby"::text[] AS "segmentby", "#,
                r#""cs"."orderby"::text[] AS "orderby", "cs"."orderby_desc" AS "orderby_desc" "#,
                r#"FROM "timescaledb_information"."hypertables" AS "ht" "#,
                r#"LEFT JOIN "_timescaledb_catalog"."compression_settings" AS "cs" ON "cs"."relid" = ($1::regclass) "#,
                r#"WHERE "ht"."hypertable_name" = $2"#
            )
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn a_merge_is_a_stored_compress_interval_on_a_dimension_of_the_named_hypertable() {
        let (sql, values) = columnstore_merge_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT EXISTS(SELECT $1 FROM "_timescaledb_catalog"."dimension" "#,
                r#"INNER JOIN "_timescaledb_catalog"."hypertable" ON "hypertable"."id" = "dimension"."hypertable_id" "#,
                r#"WHERE "hypertable"."table_name" = $2 AND "dimension"."compress_interval_length" IS NOT NULL) AS "merges""#
            )
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn the_declared_sparse_indexes_are_read_from_the_configured_entries_of_the_catalogue_array() {
        let (sql, values) = columnstore_sparse_index_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "entry" ->> $1 AS "kind", "#,
                r#"ARRAY (SELECT jsonb_array_elements_text(jsonb_path_query_array("entry", '$.column[*]'))) AS "columns" "#,
                r#"FROM "_timescaledb_catalog"."compression_settings" AS "cs", "#,
                r#"jsonb_path_query("cs"."index", '$[*] ? (@.source == "config")') AS "entry" "#,
                r#"WHERE "cs"."relid" = ($2::regclass)"#
            )
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }
}
