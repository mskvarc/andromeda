//! Where a hypertable's chunks stand in the hypercore columnstore.
//!
//! The reads an observer of the columnstore needs: whether the edition carries
//! it at all, which chunks are still in the rowstore past a cutoff, and which
//! converted chunks a later write left partial. Every hypertable name and
//! cutoff is bound; a chunk is named by its OID or by `format('%I.%I', ...)`,
//! never by interpolated text.

use crate::{
    cast::cast,
    introspection::current_setting_missing_ok,
    maintenance::relation_size,
    pg_type::PostgresType,
    setting::Setting,
    timescale::{ChunkRelationColumn, Chunks, TimescaleInformation},
};
use sea_query::{Asterisk, Expr, ExprTrait, Func, Iden, Order, Query, SelectStatement, SimpleExpr};

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

/// How many chunks of `hypertable` are still in the rowstore although their
/// range ended at or before `cutoff`, and the earliest such end.
///
/// Projects `overdue_chunks` and `oldest_range_end` (NULL when there is none)
/// from `timescaledb_information.chunks`. The hypertable name is a fixed
/// catalogue name bound as a value; the cutoff is a bound value.
#[must_use]
pub fn overdue_rowstore_chunks_statement(hypertable: &str, cutoff: SimpleExpr) -> SelectStatement {
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), OverdueChunks)
        .expr_as(Func::min(Expr::col(Chunks::RangeEnd)), OldestRangeEnd)
        .from((TimescaleInformation, Chunks::Table))
        .and_where(Expr::col(Chunks::HypertableName).eq(hypertable))
        .and_where(Expr::col(Chunks::IsCompressed).not())
        .and_where(Expr::col(Chunks::RangeEnd).lte(cast(cutoff, PostgresType::Timestamptz)))
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
fn chunk_relation() -> SimpleExpr {
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
    use crate::columnstore_chunk::{chunk_partiality_statement, columnstore_licensed, overdue_rowstore_chunks_statement, partial_chunks_statement};
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn the_columnstore_is_read_from_the_license_and_an_undefined_setting_reads_as_false() {
        let (sql, values) = Query::select().expr(columnstore_licensed()).to_owned().build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT COALESCE(current_setting($1, $2) = $3, $4)");
        assert_eq!(values.0.len(), 4, "{sql}");
    }

    #[test]
    fn overdue_chunks_are_the_rowstore_chunks_ending_by_the_bound_cutoff() {
        let (sql, values) = overdue_rowstore_chunks_statement("attribute_history", Expr::val("2026-09-01T00:00:00Z")).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT COUNT(*) AS "overdue_chunks", MIN("range_end") AS "oldest_range_end" "#,
                r#"FROM "timescaledb_information"."chunks" "#,
                r#"WHERE "hypertable_name" = $1 AND (NOT "is_compressed") AND "range_end" <= ($2::timestamptz)"#
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
