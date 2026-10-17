//! `TimescaleDB` functions sea-query has no form for.
//!
//! `TimescaleDB` extends PostgreSQL with hypertable-management and time-bucketing
//! functions that take keyword arguments (`older_than => …`) and stand in
//! statement positions sea-query models no other way. They are spelled here, in
//! the one crate sanctioned to write a dialect fragment, with every
//! caller-supplied value passed as a bound expression rather than interpolated.

use crate::{
    cast::cast,
    jsonb::{jsonb_array_elements_text, jsonb_path_query, jsonb_path_query_array},
    literal::text_literal,
    maintenance::total_relation_size,
    pg_array::array_of_subquery,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, CaseStatement, Expr, ExprTrait, Func, Iden, JoinType, Keyword, Order, Query, SelectStatement, SimpleExpr};

/// The `time_bucket(width, ts, origin)` bucketing function.
#[derive(Iden, Clone, Copy)]
#[iden = "time_bucket"]
struct TimeBucket;

/// `TimescaleDB`: `time_bucket(width, ts, origin)` - the start of the fixed-width
/// window `ts` falls in, aligned so one bucket boundary lands on `origin`.
///
/// All three arguments are expressions: `width` is an `interval`, `ts` the time
/// axis column being bucketed, and `origin` either a bound reference instant or a
/// correlated per-entity earliest observation. None is interpolated - a
/// caller-supplied period reaches `width` as a bound value already cast to
/// `interval`.
#[must_use]
pub fn time_bucket(width: SimpleExpr, ts: SimpleExpr, origin: SimpleExpr) -> SimpleExpr {
    Func::cust(TimeBucket).args([width, ts, origin]).into()
}

/// `TimescaleDB`: `drop_chunks(hypertable, older_than => cutoff::timestamptz)` -
/// drop every chunk of `hypertable` whose range ends before `cutoff`.
///
/// A set-returning function naming each dropped chunk, so it stands in a
/// projection whose row count is the number of chunks removed. The hypertable is
/// a fixed catalog name rendered inline; the cutoff is a bound value.
#[must_use]
pub fn drop_chunks(hypertable: &str, cutoff: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs(
        "drop_chunks($1, older_than => $2)",
        [text_literal(hypertable), cast(cutoff, PostgresType::Timestamptz)],
    )
}

/// The `timescaledb_information` schema every catalogue view below lives in.
#[derive(Iden, Clone, Copy)]
#[iden = "timescaledb_information"]
pub(crate) struct TimescaleInformation;

/// The `timescaledb_information.dimensions` view.
#[derive(Iden, Clone, Copy)]
enum Dimensions {
    /// View name: `dimensions`
    #[iden = "dimensions"]
    Table,
    /// The hypertable the dimension partitions
    HypertableName,
    /// Ordinal of the dimension, `1` for the time dimension
    DimensionNumber,
    /// The interval one chunk spans on a time dimension
    TimeInterval,
}

/// The `timescaledb_information.hypertables` view.
#[derive(Iden, Clone, Copy)]
enum Hypertables {
    /// View name: `hypertables`
    #[iden = "hypertables"]
    Table,
    /// The hypertable's relation name
    HypertableName,
    /// Whether the columnstore is enabled on the hypertable
    CompressionEnabled,
}

/// The `timescaledb_information.chunks` view.
#[derive(Iden, Clone, Copy)]
pub(crate) enum Chunks {
    /// View name: `chunks`
    #[iden = "chunks"]
    Table,
    /// The hypertable the chunk belongs to
    HypertableName,
    /// The schema the chunk relation lives in
    ChunkSchema,
    /// The chunk relation's name
    ChunkName,
    /// The first instant the chunk covers on the time dimension
    RangeStart,
    /// The first instant past the chunk on the time dimension
    RangeEnd,
    /// Whether the chunk is converted to the columnstore
    IsCompressed,
}

/// The `compressed_until` projection of [`chunk_span_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "compressed_until"]
pub struct CompressedUntil;

/// The `history_from` projection of [`chunk_span_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "history_from"]
pub struct HistoryFrom;

/// The `largest_rowstore_bytes` projection of [`rowstore_chunk_size_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "largest_rowstore_bytes"]
pub struct LargestRowstoreBytes;

/// The `chunk` projection of [`chunk_states_statement`]: the chunk relation
/// as a schema-qualified, quoted name.
#[derive(Iden, Clone, Copy)]
#[iden = "chunk"]
pub struct ChunkRelationColumn;

/// The `range_end` projection of [`chunk_states_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "range_end"]
pub struct ChunkRangeEnd;

/// The `is_compressed` projection of [`chunk_states_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "is_compressed"]
pub struct ChunkIsCompressed;

/// Alias `ch` for the chunks view.
#[derive(Iden, Clone, Copy)]
#[iden = "ch"]
struct Ch;

/// The `_timescaledb_catalog` schema.
#[derive(Iden, Clone, Copy)]
#[iden = "_timescaledb_catalog"]
struct TimescaleCatalog;

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

/// Alias `ht` for the hypertables view.
#[derive(Iden, Clone, Copy)]
#[iden = "ht"]
struct Ht;

/// Alias `cs` for the compression settings table.
#[derive(Iden, Clone, Copy)]
#[iden = "cs"]
struct Cs;

/// The interval one chunk of `hypertable` spans on its time dimension.
///
/// Reads `timescaledb_information.dimensions` for the first dimension, which is
/// the time dimension every hypertable in this schema is partitioned by, and
/// projects its `time_interval`. The hypertable name is a fixed catalogue name
/// bound as a value.
#[must_use]
pub fn chunk_interval_statement(hypertable: &str) -> SelectStatement {
    Query::select()
        .column(Dimensions::TimeInterval)
        .from((TimescaleInformation, Dimensions::Table))
        .and_where(Expr::col(Dimensions::HypertableName).eq(hypertable))
        .and_where(Expr::col(Dimensions::DimensionNumber).eq(1))
        .to_owned()
}

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

/// Where the chunks of `hypertables` lie on their time dimension, together.
///
/// Projects `compressed_until`, the latest `range_end` of a converted chunk
/// (NULL when none is converted), and `history_from`, the earliest
/// `range_start` of any chunk (NULL when there is none), from
/// `timescaledb_information.chunks`. The hypertable names are fixed catalogue
/// names bound as values.
#[must_use]
pub fn chunk_span_statement(hypertables: &[&str]) -> SelectStatement {
    let converted_end = CaseStatement::new()
        .case(Expr::col(Chunks::IsCompressed), Expr::col(Chunks::RangeEnd))
        .finally(Keyword::Null);
    Query::select()
        .expr_as(Func::max(converted_end), CompressedUntil)
        .expr_as(Func::min(Expr::col(Chunks::RangeStart)), HistoryFrom)
        .from((TimescaleInformation, Chunks::Table))
        .and_where(Expr::col(Chunks::HypertableName).is_in(hypertables.iter().copied()))
        .to_owned()
}

/// The largest chunk of each of `hypertables` still in the rowstore, in total
/// bytes with indexes and TOAST.
///
/// Projects `hypertable_name` and `largest_rowstore_bytes` from
/// `timescaledb_information.chunks`, grouped by hypertable, over the chunks not
/// yet converted of hypertables whose columnstore is enabled; a hypertable with
/// no such chunk, or without the columnstore, has no row. The hypertable names
/// are fixed catalogue names bound as values.
#[must_use]
pub fn rowstore_chunk_size_statement(hypertables: &[&str]) -> SelectStatement {
    let chunk = cast(
        Expr::cust_with_exprs(
            "format('%I.%I', $1, $2)",
            [Expr::col((Ch, Chunks::ChunkSchema)), Expr::col((Ch, Chunks::ChunkName))],
        ),
        PostgresType::Regclass,
    );
    Query::select()
        .column((Ch, Chunks::HypertableName))
        .expr_as(Func::max(total_relation_size(chunk)), LargestRowstoreBytes)
        .from_as((TimescaleInformation, Chunks::Table), Ch)
        .join_as(
            JoinType::InnerJoin,
            (TimescaleInformation, Hypertables::Table),
            Ht,
            Expr::col((Ht, Hypertables::HypertableName)).equals((Ch, Chunks::HypertableName)),
        )
        .and_where(Expr::col((Ch, Chunks::HypertableName)).is_in(hypertables.iter().copied()))
        .and_where(Expr::col((Ch, Chunks::IsCompressed)).not())
        .and_where(Expr::col((Ht, Hypertables::CompressionEnabled)))
        .group_by_col((Ch, Chunks::HypertableName))
        .to_owned()
}

/// Every chunk of `hypertable`, oldest first: its relation, where it ends on
/// the time dimension and whether it is converted to the columnstore.
///
/// Projects `chunk`, the relation as `format('%I.%I', chunk_schema,
/// chunk_name)` so it names the relation however its schema or name would
/// need quoting, `range_end` and `is_compressed` from
/// `timescaledb_information.chunks`, ordered by `range_start`. The hypertable
/// name is a fixed catalogue name bound as a value.
#[must_use]
pub fn chunk_states_statement(hypertable: &str) -> SelectStatement {
    Query::select()
        .expr_as(
            Expr::cust_with_exprs("format('%I.%I', $1, $2)", [Expr::col(Chunks::ChunkSchema), Expr::col(Chunks::ChunkName)]),
            ChunkRelationColumn,
        )
        .expr_as(Expr::col(Chunks::RangeEnd), ChunkRangeEnd)
        .expr_as(Expr::col(Chunks::IsCompressed), ChunkIsCompressed)
        .from((TimescaleInformation, Chunks::Table))
        .and_where(Expr::col(Chunks::HypertableName).eq(hypertable))
        .order_by(Chunks::RangeStart, Order::Asc)
        .to_owned()
}

/// `TimescaleDB`: `set_chunk_time_interval(hypertable, make_interval(days => n))`
/// - the interval every chunk created from now on spans.
///
/// The hypertable is a fixed catalogue name rendered inline; the day count is a
/// bound value turned into an `interval` server-side.
#[must_use]
pub fn set_chunk_time_interval(hypertable: &str, days: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("set_chunk_time_interval($1, make_interval(days => $2))", [text_literal(hypertable), days])
}

#[cfg(test)]
mod tests {
    use crate::timescale::{
        chunk_interval_statement,
        chunk_span_statement,
        chunk_states_statement,
        columnstore_settings_statement,
        columnstore_sparse_index_statement,
        drop_chunks,
        rowstore_chunk_size_statement,
        set_chunk_time_interval,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_chunk_interval_is_read_from_the_time_dimension_of_the_named_hypertable() {
        let (sql, values) = chunk_interval_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "time_interval" FROM "timescaledb_information"."dimensions" WHERE "hypertable_name" = $1 AND "dimension_number" = $2"#
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn the_chunk_states_name_each_chunk_relation_oldest_first() {
        let (sql, values) = chunk_states_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT format('%I.%I', "chunk_schema", "chunk_name") AS "chunk", "range_end" AS "range_end", "is_compressed" AS "is_compressed" "#,
                r#"FROM "timescaledb_information"."chunks" WHERE "hypertable_name" = $1 ORDER BY "range_start" ASC"#
            )
        );
        assert_eq!(values.0.len(), 1, "{sql}");
    }

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
    fn the_declared_sparse_indexes_are_read_from_the_configured_entries_of_the_catalogue_array() {
        let (sql, values) = columnstore_sparse_index_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "entry" ->> $1 AS "kind", "#,
                r#"ARRAY((SELECT jsonb_array_elements_text(jsonb_path_query_array("entry", '$.column[*]')))) AS "columns" "#,
                r#"FROM "_timescaledb_catalog"."compression_settings" AS "cs", "#,
                r#"jsonb_path_query("cs"."index", '$[*] ? (@.source == "config")') AS "entry" "#,
                r#"WHERE "cs"."relid" = ($2::regclass)"#
            )
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn the_chunk_interval_is_set_from_a_bound_day_count() {
        let (sql, values) = Query::select()
            .expr(set_chunk_time_interval("attribute_history", Expr::val(7_i32)))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("set_chunk_time_interval('attribute_history', make_interval(days => $1))"), "{sql}");
        assert_eq!(values.0.len(), 1, "{sql}");
    }

    #[test]
    fn time_bucket_takes_its_width_axis_and_origin_as_expressions() {
        use crate::timescale::time_bucket;
        use sea_query::Alias;
        let sql = Query::select()
            .expr(time_bucket(
                Expr::cust("'PT1H'::interval"),
                Expr::col(Alias::new("observed_at")),
                Expr::col(Alias::new("origin")),
            ))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains("time_bucket('PT1H'::interval, \"observed_at\", \"origin\")"), "{sql}");
    }

    #[test]
    fn the_chunk_span_reads_the_latest_converted_end_and_the_earliest_start_of_the_named_hypertables() {
        let sql = chunk_span_statement(&["attribute_history"]).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT MAX((CASE WHEN ("is_compressed") THEN "range_end" ELSE NULL END)) AS "compressed_until", MIN("range_start") AS "history_from" FROM "timescaledb_information"."chunks" WHERE "hypertable_name" IN ('attribute_history')"#
        );
    }

    #[test]
    fn the_largest_rowstore_chunk_is_read_per_hypertable_over_unconverted_chunks_of_columnstore_hypertables() {
        let (sql, values) = rowstore_chunk_size_statement(&["attribute_history", "scope_history"]).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "ch"."hypertable_name", MAX(pg_total_relation_size(format('%I.%I', "ch"."chunk_schema", "ch"."chunk_name")::regclass)) AS "largest_rowstore_bytes" "#,
                r#"FROM "timescaledb_information"."chunks" AS "ch" "#,
                r#"INNER JOIN "timescaledb_information"."hypertables" AS "ht" ON "ht"."hypertable_name" = "ch"."hypertable_name" "#,
                r#"WHERE "ch"."hypertable_name" IN ($1, $2) AND (NOT "ch"."is_compressed") AND "ht"."compression_enabled" "#,
                r#"GROUP BY "ch"."hypertable_name""#
            )
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn drop_chunks_names_its_hypertable_and_binds_the_cutoff() {
        let (sql, values) = Query::select()
            .expr(drop_chunks("attribute_history", Expr::val("2026-01-01T00:00:00Z")))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert!(sql.contains("drop_chunks('attribute_history', older_than => "), "{sql}");
        assert!(sql.contains("::timestamptz"), "{sql}");
        assert_eq!(values.0.len(), 1, "only the cutoff is bound: {sql}");
    }
}
