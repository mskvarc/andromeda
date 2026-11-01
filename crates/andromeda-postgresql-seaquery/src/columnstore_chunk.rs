//! Where a hypertable's chunks stand in the hypercore columnstore.
//!
//! The reads an observer and the converter of the columnstore need: whether
//! the edition carries it at all, which chunks are still in the rowstore past a
//! cutoff, which converted chunks a later write left partial, how many rows
//! and segments a chunk holds, and whether a conversion truncated the chunk's
//! rowstore. Every hypertable name and cutoff is bound; a chunk is named by
//! its OID, by `format('%I.%I', ...)` or by its schema and name as
//! identifiers, never by interpolated text.

use crate::{
    cast::cast,
    introspection::current_setting_missing_ok,
    maintenance::{indexes_size, relation_size},
    pg_type::PostgresType,
    setting::Setting,
    timescale::{ChunkRelationColumn, Chunks, TimescaleInformation, aged_chunks_statement},
};
use sea_query::{Alias, Asterisk, DynIden, Expr, ExprTrait, Func, Iden, IntoIden, Order, Query, SelectStatement, SimpleExpr, TableName};

/// The `timescaledb.license` value of the edition that carries the hypercore
/// columnstore.
const TIMESCALE_LICENSE: &str = "timescale";

/// Whether the server's `TimescaleDB` edition carries the hypercore
/// columnstore: `coalesce(current_setting('timescaledb.license', true) =
/// 'timescale', false)`.
///
/// The license setting is the only evidence: the Apache-2.0 edition registers
/// every columnstore procedure in `pg_proc` and refuses each call with
/// "functionality not supported under the current "apache" license". An
/// undefined setting, on a server without `TimescaleDB`, reads as false.
#[must_use]
pub fn columnstore_licensed() -> SimpleExpr {
    Func::coalesce([current_setting_missing_ok(Setting::TimescaledbLicense).eq(TIMESCALE_LICENSE), Expr::val(false)]).into()
}

/// The `overdue_chunks` projection of [`overdue_rowstore_chunks_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "overdue_chunks"]
pub struct OverdueChunks;

/// The `oldest_range_end` projection of [`overdue_rowstore_chunks_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "oldest_range_end"]
pub struct OldestRangeEnd;

/// A relation listing chunks by their start on the time dimension, such as
/// the chunks the converter keeps in the rowstore.
#[derive(Clone, Debug)]
pub struct ListedChunks {
    /// The relation.
    pub table: DynIden,
    /// Its column holding a kept chunk's `range_start`.
    pub range_start: DynIden,
}

impl ListedChunks {
    /// `range_start NOT IN (SELECT <range_start> FROM <table>)`: the chunk
    /// whose `range_start` the view reports is not listed.
    pub(crate) fn excludes(&self, range_start: SimpleExpr) -> SimpleExpr {
        range_start.not_in_subquery(self.starts())
    }

    /// `range_start IN (SELECT <range_start> FROM <table>)`: the chunk whose
    /// `range_start` the view reports is listed.
    pub(crate) fn includes(&self, range_start: SimpleExpr) -> SimpleExpr {
        range_start.in_subquery(self.starts())
    }

    /// `SELECT <range_start> FROM <table>`.
    fn starts(&self) -> SelectStatement {
        Query::select()
            .column(self.range_start.clone()) // clone: the subquery owns the column it projects
            .from(self.table.clone()) // clone: the subquery owns the relation it reads
            .to_owned()
    }
}

/// How many chunks of `hypertable` are still in the rowstore although their
/// range ended at or before `cutoff` and `kept` does not list them, and the
/// earliest such end.
///
/// Projects `overdue_chunks` and `oldest_range_end` (NULL when there is none)
/// from `timescaledb_information.chunks`. The hypertable name is a fixed
/// catalogue name bound as a value; the cutoff is a bound value.
#[must_use]
pub fn overdue_rowstore_chunks_statement(hypertable: &str, cutoff: SimpleExpr, kept: &ListedChunks) -> SelectStatement {
    aged_chunks_statement(hypertable, cutoff)
        .and_where(Expr::col(Chunks::IsCompressed).not())
        .and_where(kept.excludes(Expr::col(Chunks::RangeStart)))
        .to_owned()
}

/// The `segment_count` projection of [`segment_census_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "segment_count"]
pub struct SegmentCount;

/// The `row_count` projection of [`segment_census_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "row_count"]
pub struct RowCount;

/// The `n` projection of one segment's rows inside [`segment_census_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
struct SegmentRows;

/// The `segment` alias of the grouped rows inside [`segment_census_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "segment"]
struct Segment;

/// How many distinct values of `segment_by` the chunk `schema.name` holds
/// (`segment_count`) and how many rows (`row_count`), both `bigint`.
///
/// The rows are grouped by `segment_by`, which a B-tree leading with it
/// answers by an index-only scan, so the count reads each row once; a
/// `count(DISTINCT ...)` sorts every value instead. The chunk is named by its
/// schema and name as identifiers.
#[must_use]
pub fn segment_census_statement(schema: &str, name: &str, segment_by: DynIden) -> SelectStatement {
    let segments = Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), SegmentRows)
        .from(TableName::from((Alias::new(schema).into_iden(), Alias::new(name).into_iden())))
        .group_by_col(segment_by)
        .to_owned();
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), SegmentCount)
        .expr_as(
            cast(
                Func::coalesce([SimpleExpr::from(Func::sum(Expr::col((Segment, SegmentRows)))), Expr::val(0)]).into(),
                PostgresType::Bigint,
            ),
            RowCount,
        )
        .from_subquery(segments, Segment)
        .to_owned()
}

/// The `chunk_schema` projection of [`emptied_rowstore_indexes_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "chunk_schema"]
pub struct EmptiedChunkSchema;

/// The `chunk_name` projection of [`emptied_rowstore_indexes_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "chunk_name"]
pub struct EmptiedChunkName;

/// The converted chunks of `hypertable` whose rowstore heap holds no page but
/// whose rowstore indexes take more than `min_index_bytes`, oldest first.
///
/// A conversion that deleted its rowstore rows instead of truncating the
/// relation, or a recompression of a partial chunk, leaves the indexes at the
/// size they reached; vacuum empties and truncates the heap but returns no
/// index page to the file system. Projects `chunk_schema` and `chunk_name`
/// from `timescaledb_information.chunks`. The hypertable name and the byte
/// floor are bound values.
#[must_use]
pub fn emptied_rowstore_indexes_statement(hypertable: &str, min_index_bytes: i64) -> SelectStatement {
    let chunk = || cast(chunk_relation(), PostgresType::Regclass);
    Query::select()
        .expr_as(Expr::col(Chunks::ChunkSchema), EmptiedChunkSchema)
        .expr_as(Expr::col(Chunks::ChunkName), EmptiedChunkName)
        .from((TimescaleInformation, Chunks::Table))
        .and_where(Expr::col(Chunks::HypertableName).eq(hypertable))
        .and_where(Expr::col(Chunks::IsCompressed))
        .and_where(Expr::cust_with_exprs("pg_relation_size($1, 'main') = 0", [chunk()]))
        .and_where(Expr::expr(indexes_size(chunk())).gt(min_index_bytes))
        .order_by(Chunks::RangeStart, Order::Asc)
        .to_owned()
}

/// The `chunk_oid` projection of [`partial_chunks_statement`]: the chunk
/// relation's OID as a `bigint`.
#[derive(Iden, Clone, Copy)]
#[iden = "chunk_oid"]
pub struct ChunkOidColumn;

/// The `uncompressed_bytes` projection of [`partial_chunks_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "uncompressed_bytes"]
pub struct UncompressedBytes;

/// The `partial_chunks` projection of [`partial_chunks_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "partial_chunks"]
pub struct PartialChunks;

/// `'PARTIAL' = ANY(_timescaledb_functions.chunk_status_text(chunk))`:
/// whether a converted chunk holds rows outside its compressed batches, which
/// any `INSERT`, `UPDATE` or `DELETE` reaching it leaves there.
fn chunk_is_partial(chunk: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("'PARTIAL' = ANY(_timescaledb_functions.chunk_status_text($1))", [chunk])
}

/// `format('%I.%I', chunk_schema, chunk_name)`: a chunk's relation, quoted
/// where its schema or name needs it.
pub(crate) fn chunk_relation() -> SimpleExpr {
    Expr::cust_with_exprs("format('%I.%I', $1, $2)", [Expr::col(Chunks::ChunkSchema), Expr::col(Chunks::ChunkName)])
}

/// The oldest partially converted chunk of `hypertable`, at most one row.
///
/// Projects the chunk's OID (`chunk_oid`), its relation (`chunk`), the bytes
/// of its own heap, where the rows outside its batches live
/// (`uncompressed_bytes`), and how many chunks of the hypertable are partial
/// (`partial_chunks`), from `timescaledb_information.chunks`. No row means
/// none is partial. The hypertable name is a fixed catalogue name bound as a
/// value.
#[must_use]
pub fn partial_chunks_statement(hypertable: &str) -> SelectStatement {
    let chunk = || cast(chunk_relation(), PostgresType::Regclass);
    Query::select()
        .expr_as(cast(cast(chunk(), PostgresType::Oid), PostgresType::Bigint), ChunkOidColumn)
        .expr_as(chunk_relation(), ChunkRelationColumn)
        .expr_as(relation_size(chunk()), UncompressedBytes)
        .expr_as(Expr::cust("count(*) OVER ()"), PartialChunks)
        .from((TimescaleInformation, Chunks::Table))
        .and_where(Expr::col(Chunks::HypertableName).eq(hypertable))
        .and_where(Expr::col(Chunks::IsCompressed))
        .and_where(chunk_is_partial(chunk()))
        .order_by(Chunks::RangeStart, Order::Asc)
        .limit(1)
        .to_owned()
}

/// Whether the chunk with OID `chunk_oid` is partially converted. The OID is a
/// bound value.
#[must_use]
pub fn chunk_partiality_statement(chunk_oid: i64) -> SelectStatement {
    Query::select()
        .expr(chunk_is_partial(cast(cast(Expr::val(chunk_oid), PostgresType::Oid), PostgresType::Regclass)))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::columnstore_chunk::{
        ListedChunks,
        chunk_partiality_statement,
        columnstore_licensed,
        emptied_rowstore_indexes_statement,
        overdue_rowstore_chunks_statement,
        partial_chunks_statement,
        segment_census_statement,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, IntoIden, PostgresQueryBuilder, Query};

    fn kept() -> ListedChunks {
        ListedChunks {
            table: Alias::new("kept").into_iden(),
            range_start: Alias::new("since").into_iden(),
        }
    }

    #[test]
    fn the_columnstore_is_read_from_the_license_and_an_undefined_setting_reads_as_false() {
        let (sql, values) = Query::select().expr(columnstore_licensed()).to_owned().build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT COALESCE(current_setting($1, $2) = $3, $4)");
        assert_eq!(values.0.len(), 4, "{sql}");
    }

    #[test]
    fn overdue_chunks_are_the_rowstore_chunks_ending_by_the_bound_cutoff_and_not_kept() {
        let (sql, values) = overdue_rowstore_chunks_statement("attribute_history", Expr::val("2026-09-01T00:00:00Z"), &kept()).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT COUNT(*) AS "overdue_chunks", MIN("range_end") AS "oldest_range_end" "#,
                r#"FROM "timescaledb_information"."chunks" "#,
                r#"WHERE "hypertable_name" = $1 AND "range_end" <= ($2::timestamptz) AND (NOT "is_compressed") "#,
                r#"AND "range_start" NOT IN (SELECT "since" FROM "kept")"#
            )
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn a_census_counts_the_rows_and_segments_of_the_named_chunk_through_one_grouping() {
        let (sql, values) =
            segment_census_statement("_timescaledb_internal", "_hyper_1_2_chunk", Alias::new("stream_key").into_iden()).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT COUNT(*) AS "segment_count", COALESCE(SUM("segment"."n"), $1)::bigint AS "row_count" "#,
                r#"FROM (SELECT COUNT(*) AS "n" FROM "_timescaledb_internal"."_hyper_1_2_chunk" GROUP BY "stream_key") AS "segment""#
            )
        );
        assert_eq!(values.0.len(), 1, "{sql}");
    }

    #[test]
    fn emptied_rowstore_indexes_are_those_of_converted_chunks_with_an_empty_heap_past_the_byte_floor() {
        let (sql, values) = emptied_rowstore_indexes_statement("attribute_history", 1_048_576).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "chunk_schema" AS "chunk_schema", "chunk_name" AS "chunk_name" "#,
                r#"FROM "timescaledb_information"."chunks" "#,
                r#"WHERE "hypertable_name" = $1 AND "is_compressed" "#,
                r#"AND (pg_relation_size(format('%I.%I', "chunk_schema", "chunk_name")::regclass, 'main') = 0) "#,
                r#"AND pg_indexes_size(format('%I.%I', "chunk_schema", "chunk_name")::regclass) > $2 "#,
                r#"ORDER BY "range_start" ASC"#
            )
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn the_oldest_partial_chunk_is_named_by_oid_and_counted_among_its_peers() {
        let (sql, values) = partial_chunks_statement("attribute_history").build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT format('%I.%I', "chunk_schema", "chunk_name")::regclass::oid::bigint AS "chunk_oid", "#,
                r#"format('%I.%I', "chunk_schema", "chunk_name") AS "chunk", "#,
                r#"pg_relation_size(format('%I.%I', "chunk_schema", "chunk_name")::regclass) AS "uncompressed_bytes", "#,
                r#"count(*) OVER () AS "partial_chunks" "#,
                r#"FROM "timescaledb_information"."chunks" "#,
                r#"WHERE "hypertable_name" = $1 AND "is_compressed" "#,
                r#"AND ('PARTIAL' = ANY(_timescaledb_functions.chunk_status_text(format('%I.%I', "chunk_schema", "chunk_name")::regclass))) "#,
                r#"ORDER BY "range_start" ASC LIMIT $2"#
            )
        );
        assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn a_chunk_partiality_is_read_by_its_bound_oid() {
        let (sql, values) = chunk_partiality_statement(16_384).build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT 'PARTIAL' = ANY(_timescaledb_functions.chunk_status_text($1::oid::regclass))");
        assert_eq!(values.0.len(), 1, "{sql}");
    }
}
