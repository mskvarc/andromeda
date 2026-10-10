//! Cheap catalog sampling and bounded physical probes.

use crate::{
    action_measurement::TupleCounts,
    artifact::RecoveryArtifact,
    candidate::{HeapTarget, IndexTarget},
    catalog_counter::{CatalogCounter, non_negative, percent},
    error::MaintenanceError,
    policy::{DegradationSample, PolicyProfile},
    scalar_query::select_i64,
};
use andromeda_postgresql_seaquery::maintenance::{pgstattuple, relation_size, session_settings, to_regclass, total_relation_size};
use sea_query::{Alias, Expr, ExprTrait, JoinType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Executor, PgConnection, Row};
use std::str::FromStr;

/// An allowlisted index's cheap catalog size.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct IndexSize {
    /// Compile-time target recovered from the exact catalog name.
    pub(crate) target: IndexTarget,
    /// Current physical bytes.
    pub(crate) bytes: u64,
}

/// One heap's cheap dead-tuple estimate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapSample {
    /// Compile-time heap target.
    pub(crate) target: HeapTarget,
    /// Threshold-ready degradation sample.
    pub(crate) degradation: DegradationSample,
}

/// An invalid index reported by PostgreSQL.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InvalidIndex {
    /// Catalog name, retained for reporting and validated artifact recovery.
    name: String,
}

impl InvalidIndex {
    /// Return a recovery artifact only when the name belongs to the allowlist.
    pub(crate) fn recovery_artifact(&self) -> Option<RecoveryArtifact> {
        RecoveryArtifact::parse(&self.name)
    }

    /// Borrow the catalog name for structured reporting.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// Configure one short-lived maintenance session.
pub(crate) async fn configure_session(connection: &mut PgConnection) -> Result<(), MaintenanceError> {
    for statement in session_settings() {
        connection.execute(sqlx::AssertSqlSafe(statement.as_sql())).await?;
    }
    Ok(())
}

/// Inspect all allowlisted index sizes from cheap catalogs.
pub(crate) async fn index_sizes(connection: &mut PgConnection) -> Result<Vec<IndexSize>, MaintenanceError> {
    let (sql, values) = index_sizes_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let name = row.try_get::<String, _>(CatalogColumn::Indexrelname.as_str())?;
            let target = IndexTarget::from_str(&name).map_err(MaintenanceError::UnknownTarget)?;
            Ok(IndexSize {
                target,
                bytes: non_negative(row.try_get(CatalogColumn::Bytes.as_str())?, CatalogCounter::IndexBytes)?,
            })
        })
        .collect()
}

/// Inspect all allowlisted heap dead-tuple estimates from cheap statistics.
pub(crate) async fn heap_samples(connection: &mut PgConnection, profile: PolicyProfile) -> Result<Vec<HeapSample>, MaintenanceError> {
    let (sql, values) = heap_samples_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let name = row.try_get::<String, _>(CatalogColumn::Relname.as_str())?;
            let target = HeapTarget::from_str(&name).map_err(MaintenanceError::UnknownTarget)?;
            let live = non_negative(row.try_get(CatalogColumn::LiveTuples.as_str())?, CatalogCounter::LiveTuples)?;
            let dead = non_negative(row.try_get(CatalogColumn::DeadTuples.as_str())?, CatalogCounter::DeadTuples)?;
            let bytes = non_negative(row.try_get(CatalogColumn::Bytes.as_str())?, CatalogCounter::HeapBytes)?;
            let counts = TupleCounts { dead, live };
            Ok(HeapSample {
                target,
                degradation: DegradationSample {
                    relation_bytes: bytes,
                    reclaimable_bytes: bytes.saturating_mul(dead).checked_div(live.saturating_add(dead)).unwrap_or_default(),
                    degraded_percent: counts.dead_percent().unwrap_or_default(),
                    threshold_percent: profile.heap_dead_percent,
                },
            })
        })
        .collect()
}

/// Perform the one detailed physical probe allowed in a pass.
pub(crate) async fn probe_index(connection: &mut PgConnection, size: IndexSize, profile: PolicyProfile) -> Result<DegradationSample, MaintenanceError> {
    let qualified = format!("public.{}", size.target.relation_name());
    let (sql, values) = pgstattuple_statement(&qualified).build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?;
    let reclaimable_bytes = non_negative(row.try_get(CatalogColumn::FreeSpace.as_str())?, CatalogCounter::IndexFreeBytes)?;
    let physical_bytes = non_negative(row.try_get(CatalogColumn::TableLength.as_str())?, CatalogCounter::IndexPhysicalBytes)?;
    Ok(DegradationSample {
        relation_bytes: size.bytes,
        reclaimable_bytes,
        // `pgstattuple` derives `free_percent` as `100 * free_space / table_len`;
        // deriving it from the two byte counters keeps the sample an exact
        // integer instead of narrowing the extension's own float.
        degraded_percent: percent(reclaimable_bytes, physical_bytes),
        threshold_percent: profile.index_free_percent,
    })
}

/// Read every invalid user index so unrelated corruption is still visible.
pub(crate) async fn invalid_indexes(connection: &mut PgConnection) -> Result<Vec<InvalidIndex>, MaintenanceError> {
    let (sql, values) = invalid_indexes_statement().build_sqlx(PostgresQueryBuilder);
    let names = sqlx::query_scalar_with::<_, String, _>(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_all(connection)
        .await?;
    Ok(names.into_iter().map(|name| InvalidIndex { name }).collect())
}

/// Read an index's current physical bytes, compared across its rebuild.
pub(crate) async fn index_bytes(connection: &mut PgConnection, index: IndexTarget) -> Result<u64, MaintenanceError> {
    let statement = Query::select()
        .expr(total_relation_size(to_regclass(Expr::val(format!("public.{}", index.relation_name())))))
        .to_owned();
    non_negative(select_i64(connection, statement).await?, CatalogCounter::RelationBytes)
}

/// Read a heap's current tuple counts, compared across its vacuum.
///
/// The read runs as its own statement outside any transaction, so it never
/// sees the statistics snapshot an earlier read cached; a vacuum publishes its
/// remaining dead tuples as `n_dead_tup` when it finishes.
pub(crate) async fn heap_tuple_counts(connection: &mut PgConnection, heap: HeapTarget) -> Result<TupleCounts, MaintenanceError> {
    let (sql, values) = heap_tuple_counts_statement(heap).build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?;
    Ok(TupleCounts {
        dead: non_negative(row.try_get(CatalogColumn::DeadTuples.as_str())?, CatalogCounter::DeadTuples)?,
        live: non_negative(row.try_get(CatalogColumn::LiveTuples.as_str())?, CatalogCounter::LiveTuples)?,
    })
}

/// Cheap catalog size query for every allowlisted index.
fn index_sizes_statement() -> SelectStatement {
    Query::select()
        .column(Alias::new(CatalogColumn::Indexrelname.as_str()))
        .expr_as(relation_size(Expr::col(Alias::new("indexrelid"))), Alias::new(CatalogColumn::Bytes.as_str()))
        .from(Alias::new("pg_stat_user_indexes"))
        .and_where(Expr::col(Alias::new("schemaname")).eq("public"))
        .and_where(Expr::col(Alias::new("relname")).eq("current_attributes"))
        .and_where(Expr::col(Alias::new("indexrelname")).is_in(IndexTarget::ALL.map(IndexTarget::relation_name)))
        .to_owned()
}

/// Cheap dead-tuple estimate query for every allowlisted heap.
fn heap_samples_statement() -> SelectStatement {
    Query::select()
        .column(Alias::new(CatalogColumn::Relname.as_str()))
        .expr_as(Expr::col(Alias::new("n_live_tup")), Alias::new(CatalogColumn::LiveTuples.as_str()))
        .expr_as(Expr::col(Alias::new("n_dead_tup")), Alias::new(CatalogColumn::DeadTuples.as_str()))
        .expr_as(total_relation_size(Expr::col(Alias::new("relid"))), Alias::new(CatalogColumn::Bytes.as_str()))
        .from(Alias::new("pg_stat_user_tables"))
        .and_where(Expr::col(Alias::new("schemaname")).eq("public"))
        .and_where(Expr::col(Alias::new("relname")).is_in(HeapTarget::ALL.map(HeapTarget::relation_name)))
        .to_owned()
}

/// Dead and live tuple counts of one allowlisted heap.
fn heap_tuple_counts_statement(heap: HeapTarget) -> SelectStatement {
    Query::select()
        .expr_as(Expr::col(Alias::new("n_dead_tup")), Alias::new(CatalogColumn::DeadTuples.as_str()))
        .expr_as(Expr::col(Alias::new("n_live_tup")), Alias::new(CatalogColumn::LiveTuples.as_str()))
        .from(Alias::new("pg_stat_user_tables"))
        .and_where(Expr::col(Alias::new("schemaname")).eq("public"))
        .and_where(Expr::col(Alias::new("relname")).eq(heap.relation_name()))
        .to_owned()
}

/// Detailed physical tuple query against one bound `regclass`.
fn pgstattuple_statement(relation: &str) -> SelectStatement {
    Query::select()
        .columns([Alias::new(CatalogColumn::FreeSpace.as_str()), Alias::new(CatalogColumn::TableLength.as_str())])
        .from_function(pgstattuple(to_regclass(Expr::val(relation))), Alias::new("physical"))
        .to_owned()
}

/// Every invalid index in the application schema.
fn invalid_indexes_statement() -> SelectStatement {
    Query::select()
        .column((Alias::new("c"), Alias::new("relname")))
        .from_as(Alias::new("pg_index"), Alias::new("i"))
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_class"),
            Alias::new("c"),
            Expr::col((Alias::new("c"), Alias::new("oid"))).equals((Alias::new("i"), Alias::new("indexrelid"))),
        )
        .join_as(
            JoinType::InnerJoin,
            Alias::new("pg_namespace"),
            Alias::new("n"),
            Expr::col((Alias::new("n"), Alias::new("oid"))).equals((Alias::new("c"), Alias::new("relnamespace"))),
        )
        .and_where(Expr::col((Alias::new("n"), Alias::new("nspname"))).eq("public"))
        .and_where(Expr::col((Alias::new("i"), Alias::new("indisvalid"))).eq(false))
        .order_by((Alias::new("c"), Alias::new("relname")), sea_query::Order::Asc)
        .to_owned()
}

/// Stable projected field names decoded from catalog rows.
#[derive(Clone, Copy)]
enum CatalogColumn {
    /// `pg_stat_user_indexes.indexrelname` projection.
    Indexrelname,
    /// Heap relation-name projection.
    Relname,
    /// Estimated live tuple count projection.
    LiveTuples,
    /// Estimated dead tuple count projection.
    DeadTuples,
    /// Physical relation byte projection.
    Bytes,
    /// Detailed index free-byte projection.
    FreeSpace,
    /// Detailed index physical-length projection.
    TableLength,
}

impl CatalogColumn {
    /// Render the exact projected field name.
    const fn as_str(self) -> &'static str {
        match self {
            CatalogColumn::Indexrelname => "indexrelname",
            CatalogColumn::Relname => "relname",
            CatalogColumn::LiveTuples => "live_tuples",
            CatalogColumn::DeadTuples => "dead_tuples",
            CatalogColumn::Bytes => "bytes",
            CatalogColumn::FreeSpace => "free_space",
            CatalogColumn::TableLength => "table_len",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::{HeapTarget, IndexTarget},
        inspection::{heap_samples_statement, heap_tuple_counts_statement, index_sizes_statement, invalid_indexes_statement},
    };
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_action_tuple_count_read_names_one_heap() {
        let sql = heap_tuple_counts_statement(HeapTarget::Entities).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""n_dead_tup" AS "dead_tuples""#), "{sql}");
        assert!(sql.contains(r#""n_live_tup" AS "live_tuples""#), "{sql}");
        assert!(sql.contains(r#""relname" = 'entities'"#), "{sql}");
    }

    #[test]
    fn catalog_queries_are_seaquery_statements_with_closed_filters() {
        let index_sql = index_sizes_statement().to_string(PostgresQueryBuilder);
        assert!(index_sql.contains("pg_stat_user_indexes"), "{index_sql}");
        assert!(index_sql.contains("idx_ca_value_string_trgm"), "{index_sql}");
        let invalid_sql = invalid_indexes_statement().to_string(PostgresQueryBuilder);
        assert!(invalid_sql.contains("pg_index"), "{invalid_sql}");
        assert!(invalid_sql.contains("indisvalid"), "{invalid_sql}");
    }

    /// The size sample reads the GIN indexes too, so a canary's queued GIN
    /// reports its bytes before and after its rebuild.
    #[test]
    fn the_index_size_sample_reads_every_current_attributes_index_the_key_gin_included() {
        let sql = index_sizes_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("\"relname\" = 'current_attributes'"), "{sql}");
        for target in IndexTarget::ALL {
            assert!(sql.contains(&format!("'{}'", target.relation_name())), "{target}: {sql}");
        }
        assert!(sql.contains("'idx_ca_sub_eq_keys'"), "{sql}");
    }

    #[test]
    fn the_heap_sample_reads_current_attributes_and_entities() {
        let sql = heap_samples_statement().to_string(PostgresQueryBuilder);
        assert!(sql.contains("pg_stat_user_tables"), "{sql}");
        for target in HeapTarget::ALL {
            assert!(sql.contains(&format!("'{}'", target.relation_name())), "{target}: {sql}");
        }
        assert!(sql.contains("'current_attributes'"), "{sql}");
    }
}
