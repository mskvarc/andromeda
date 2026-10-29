//! The closed rowstore chunks of a hypertable whose indexes await their one
//! rebuild.
//!
//! A chunk's indexes reach the density of appends in write order, and a chunk
//! whose range has ended receives no more appends, so one rebuild after it
//! closes packs them for good. A chunk the columnstore is still to convert
//! drops its rowstore indexes on conversion, so where the edition carries the
//! columnstore only the chunks the converter keeps in the rowstore await a
//! rebuild; elsewhere every closed chunk does. Every hypertable name and floor
//! is bound; a chunk is named by its OID and by `format('%I.%I', ...)`, never
//! by interpolated text.

use crate::{
    cast::cast,
    clock::now,
    columnstore_chunk::{ChunkOidColumn, ListedChunks, chunk_relation, columnstore_licensed},
    maintenance::indexes_size,
    pg_type::PostgresType,
    timescale::{ChunkNameColumn, ChunkRangeStart, ChunkRelationColumn, ChunkSchemaColumn, Chunks, TimescaleInformation},
};
use sea_query::{Alias, DeleteStatement, Expr, ExprTrait, Iden, JoinType, Order, Query, SelectStatement};

/// The `index_bytes` projection of [`awaiting_rebuild_statement`]: the bytes
/// of every index of the chunk.
#[derive(Iden, Clone, Copy)]
#[iden = "index_bytes"]
pub struct ChunkIndexBytes;

/// The `awaiting_chunks` projection of [`awaiting_rebuild_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "awaiting_chunks"]
pub struct AwaitingChunks;

/// The oldest closed rowstore chunk of `hypertable` whose indexes await their
/// rebuild and hold at least `min_index_bytes`, at most one row.
///
/// A chunk awaits its rebuild when it is not converted, its range ended
/// before the statement's clock, `rebuilt` does not list it, and, where the
/// edition carries the columnstore, `kept` lists it. Projects the chunk's OID
/// (`chunk_oid`), its relation (`chunk`), its schema and name, its
/// `range_start`, the bytes of its indexes (`index_bytes`) and how many chunks
/// of the hypertable await a rebuild over the floor (`awaiting_chunks`), from
/// `timescaledb_information.chunks`. No row means none awaits one.
#[must_use]
pub fn awaiting_rebuild_statement(hypertable: &str, kept: &ListedChunks, rebuilt: &ListedChunks, min_index_bytes: i64) -> SelectStatement {
    let chunk = || cast(chunk_relation(), PostgresType::Regclass);
    Query::select()
        .expr_as(cast(cast(chunk(), PostgresType::Oid), PostgresType::Bigint), ChunkOidColumn)
        .expr_as(chunk_relation(), ChunkRelationColumn)
        .expr_as(Expr::col(Chunks::ChunkSchema), ChunkSchemaColumn)
        .expr_as(Expr::col(Chunks::ChunkName), ChunkNameColumn)
        .expr_as(Expr::col(Chunks::RangeStart), ChunkRangeStart)
        .expr_as(indexes_size(chunk()), ChunkIndexBytes)
        .expr_as(Expr::cust("count(*) OVER ()"), AwaitingChunks)
        .from((TimescaleInformation, Chunks::Table))
        .and_where(Expr::col(Chunks::HypertableName).eq(hypertable))
        .and_where(Expr::col(Chunks::IsCompressed).not())
        .and_where(Expr::col(Chunks::RangeEnd).lte(now()))
        .and_where(columnstore_licensed().not().or(kept.includes(Expr::col(Chunks::RangeStart))))
        .and_where(rebuilt.excludes(Expr::col(Chunks::RangeStart)))
        .and_where(Expr::expr(indexes_size(chunk())).gte(min_index_bytes))
        .order_by(Chunks::RangeStart, Order::Asc)
        .limit(1)
        .to_owned()
}

/// The bytes of every index of the chunk with OID `chunk_oid`, a bound value.
#[must_use]
pub fn chunk_index_bytes_statement(chunk_oid: i64) -> SelectStatement {
    Query::select()
        .expr(indexes_size(cast(cast(Expr::val(chunk_oid), PostgresType::Oid), PostgresType::Regclass)))
        .to_owned()
}

/// Remove from `listed` every chunk `hypertable` no longer holds: every
/// listed `range_start` the catalogue reports for none of its chunks. The
/// hypertable name is a bound value.
#[must_use]
pub fn departed_chunks_deletion(hypertable: &str, listed: &ListedChunks) -> DeleteStatement {
    let held = Query::select()
        .column(Chunks::RangeStart)
        .from((TimescaleInformation, Chunks::Table))
        .and_where(Expr::col(Chunks::HypertableName).eq(hypertable))
        .to_owned();
    Query::delete()
        .from_table(listed.table.clone()) // clone: the statement owns the relation it deletes from
        .and_where(Expr::col(listed.range_start.clone()).not_in_subquery(held)) // clone: the statement owns the column it compares
        .to_owned()
}

/// The `index_schema` projection of [`invalid_chunk_indexes_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "index_schema"]
pub struct InvalidIndexSchema;

/// The `index_name` projection of [`invalid_chunk_indexes_statement`].
#[derive(Iden, Clone, Copy)]
#[iden = "index_name"]
pub struct InvalidIndexName;

/// Every invalid index on a chunk of `hypertable`, the oldest chunk first:
/// what an interrupted `REINDEX TABLE CONCURRENTLY` of a chunk leaves behind.
///
/// Projects the chunk's OID (`chunk_oid`) and relation (`chunk`), and the
/// index's schema (`index_schema`) and name (`index_name`). The hypertable
/// name is a bound value.
#[must_use]
pub fn invalid_chunk_indexes_statement(hypertable: &str) -> SelectStatement {
    let catalog = |alias: &'static str, column: &'static str| Expr::col((Alias::new(alias), Alias::new(column)));
    let chunk = || cast(chunk_relation(), PostgresType::Regclass);
    Query::select()
        .expr_as(cast(cast(chunk(), PostgresType::Oid), PostgresType::Bigint), ChunkOidColumn)
        .expr_as(chunk_relation(), ChunkRelationColumn)
        .expr_as(catalog("n", "nspname"), InvalidIndexSchema)
        .expr_as(catalog("c", "relname"), InvalidIndexName)
        .from((TimescaleInformation, Chunks::Table))
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_index"),
            Alias::new("i"),
            catalog("i", "indrelid").eq(chunk()),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_class"),
            Alias::new("c"),
            catalog("c", "oid").equals((Alias::new("i"), Alias::new("indexrelid"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_namespace"),
            Alias::new("n"),
            catalog("n", "oid").equals((Alias::new("c"), Alias::new("relnamespace"))),
        )
        .and_where(Expr::col(Chunks::HypertableName).eq(hypertable))
        .and_where(catalog("i", "indisvalid").eq(false))
        .order_by(Chunks::RangeStart, Order::Asc)
        .order_by((Alias::new("c"), Alias::new("relname")), Order::Asc)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{
        columnstore_chunk::ListedChunks,
        rowstore_chunk::{awaiting_rebuild_statement, chunk_index_bytes_statement, departed_chunks_deletion, invalid_chunk_indexes_statement},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, IntoIden, PostgresQueryBuilder};

    fn listed(table: &str) -> ListedChunks {
        ListedChunks {
            table: Alias::new(table).into_iden(),
            range_start: Alias::new("range_start").into_iden(),
        }
    }

    /// A closed, unconverted chunk not yet rebuilt and over the floor awaits a
    /// rebuild; under a columnstore edition only if the converter keeps it.
    #[test]
    fn the_oldest_closed_unconverted_chunk_not_yet_rebuilt_awaits_its_rebuild() {
        let (sql, values) = awaiting_rebuild_statement("attribute_history", &listed("kept"), &listed("rebuilt"), 1_048_576).build(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"SELECT format('%I.%I', "chunk_schema", "chunk_name")::regclass::oid::bigint AS "chunk_oid""#),
            "{sql}"
        );
        for clause in [
            r#"FROM "timescaledb_information"."chunks" WHERE "hypertable_name" = $"#,
            r#"AND (NOT "is_compressed") AND "range_end" <= NOW()"#,
            r#"OR "range_start" IN (SELECT "range_start" FROM "kept"))"#,
            r#"AND "range_start" NOT IN (SELECT "range_start" FROM "rebuilt")"#,
            r#"AS "index_bytes", count(*) OVER () AS "awaiting_chunks""#,
            r#"ORDER BY "range_start" ASC LIMIT $"#,
        ] {
            assert!(sql.contains(clause), "{clause}: {sql}");
        }
        let bound = format!("{values:?}");
        assert!(bound.contains("attribute_history") && bound.contains("1048576"), "{bound}");
    }

    #[test]
    fn a_chunk_s_index_bytes_are_read_by_its_oid() {
        let (sql, values) = chunk_index_bytes_statement(16_384).build(PostgresQueryBuilder);
        assert_eq!(sql, "SELECT pg_indexes_size($1::oid::regclass)");
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn a_listed_chunk_the_hypertable_no_longer_holds_is_removed() {
        let (sql, values) = departed_chunks_deletion("attribute_history", &listed("rebuilt")).build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"DELETE FROM "rebuilt" WHERE "range_start" NOT IN (SELECT "range_start" FROM "timescaledb_information"."chunks" WHERE "hypertable_name" = $1)"#
        );
        assert_eq!(values.0.len(), 1);
    }

    /// The invalid indexes are those PostgreSQL marks invalid on a chunk of
    /// the bound hypertable, each beside its chunk.
    #[test]
    fn an_invalid_index_is_read_beside_the_chunk_it_was_built_on() {
        let (sql, values) = invalid_chunk_indexes_statement("attribute_history").build(PostgresQueryBuilder);
        for clause in [
            r#"AS "chunk_oid""#,
            r#""n"."nspname" AS "index_schema", "c"."relname" AS "index_name""#,
            r#"INNER JOIN "pg_index" AS "i" ON "i"."indrelid" = (format('%I.%I', "chunk_schema", "chunk_name")::regclass)"#,
            r#"WHERE "hypertable_name" = $1 AND "i"."indisvalid" = $2"#,
            r#"ORDER BY "range_start" ASC, "c"."relname" ASC"#,
        ] {
            assert!(sql.contains(clause), "{clause}: {sql}");
        }
        assert_eq!(values.0.len(), 2);
    }
}
