//! Cheap catalog sampling, bounded physical probes and execution safety gates.

use crate::{
    artifact::{ArtifactIndexName, ArtifactKind},
    candidate::{HeapTarget, IndexTarget, MaintenanceTarget},
    error::MaintenanceError,
    policy::{DegradationSample, PolicyProfile, SafetyGates},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::current_setting,
    maintenance::{
        backend_pid,
        current_user_has_function_privilege,
        current_user_has_table_privilege,
        pgstattuple,
        relation_size,
        session_settings,
        to_regclass,
        to_regprocedure,
        total_relation_size,
    },
    pg_type::PostgresType,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, JoinType, PostgresQueryBuilder, Query, SelectStatement};
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
    pub(crate) fn recovery_artifact(&self) -> Option<(ArtifactIndexName, ArtifactKind)> {
        ArtifactIndexName::parse(&self.name)
    }

    /// Borrow the catalog name for structured reporting.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}

/// Per-database capability validation result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CapabilityStatus {
    /// Required extension, function execution and optional mutation privileges exist.
    Ready,
    /// The optional supplied extension is not installed.
    MissingPgstattuple,
    /// The role cannot execute required physical inspection functions.
    MissingPgstattupleExecute,
    /// Automatic mode lacks `MAINTAIN` on at least one allowlisted heap.
    MissingMaintain,
}

/// Whether capability validation includes mutation privileges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MutationPrivilege {
    /// Inspection privileges only.
    NotRequired,
    /// PostgreSQL 18 `MAINTAIN` is required on all allowlisted heaps.
    Required,
}

/// Configure one short-lived maintenance session.
pub(crate) async fn configure_session(connection: &mut PgConnection) -> Result<(), MaintenanceError> {
    for statement in session_settings() {
        connection.execute(sqlx::AssertSqlSafe(statement.as_sql())).await?;
    }
    Ok(())
}

/// Validate the optional diagnostic extension and role privileges.
pub(crate) async fn validate_capabilities(connection: &mut PgConnection, mutation: MutationPrivilege) -> Result<CapabilityStatus, MaintenanceError> {
    if !select_bool(connection, extension_present_statement()).await? {
        return Ok(CapabilityStatus::MissingPgstattuple);
    }
    if !select_bool(connection, inspection_privilege_statement()).await? {
        return Ok(CapabilityStatus::MissingPgstattupleExecute);
    }
    if mutation == MutationPrivilege::Required && !select_bool(connection, maintain_privilege_statement()).await? {
        return Ok(CapabilityStatus::MissingMaintain);
    }
    Ok(CapabilityStatus::Ready)
}

/// Inspect all allowlisted index sizes from cheap catalogs.
pub(crate) async fn index_sizes(connection: &mut PgConnection) -> Result<Vec<IndexSize>, MaintenanceError> {
    let (sql, values) = index_sizes_statement().build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.into_iter()
        .map(|row| {
            let name = row.try_get::<String, _>(CatalogColumn::Indexrelname.as_str())?;
            let target = IndexTarget::from_str(&name).map_err(|_| MaintenanceError::UnknownTarget(name))?;
            Ok(IndexSize {
                target,
                bytes: non_negative(row.try_get(CatalogColumn::Bytes.as_str())?, "index bytes")?,
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
            let target = HeapTarget::from_str(&name).map_err(|_| MaintenanceError::UnknownTarget(name))?;
            let live = non_negative(row.try_get(CatalogColumn::LiveTuples.as_str())?, "live tuples")?;
            let dead = non_negative(row.try_get(CatalogColumn::DeadTuples.as_str())?, "dead tuples")?;
            let bytes = non_negative(row.try_get(CatalogColumn::Bytes.as_str())?, "heap bytes")?;
            let tuples = live.saturating_add(dead);
            Ok(HeapSample {
                target,
                degradation: DegradationSample {
                    relation_bytes: bytes,
                    reclaimable_bytes: bytes.saturating_mul(dead).checked_div(tuples).unwrap_or_default(),
                    degraded_percent: if tuples == 0 { 0 } else { percent(dead, tuples) },
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
    let reclaimable_bytes = non_negative(row.try_get(CatalogColumn::FreeSpace.as_str())?, "index free bytes")?;
    let measured = row.try_get::<f64, _>(CatalogColumn::FreePercent.as_str())?.clamp(0.0, 100.0);
    Ok(DegradationSample {
        relation_bytes: size.bytes,
        reclaimable_bytes,
        degraded_percent: measured as u8,
        threshold_percent: profile.index_free_percent,
    })
}

/// Measure load, long transactions, active maintenance and waiting locks.
pub(crate) async fn safety_gates(connection: &mut PgConnection, target: MaintenanceTarget) -> Result<SafetyGates, MaintenanceError> {
    let heap = format!("public.{}", target.heap_name());
    let relation = match target {
        MaintenanceTarget::Index(index) => Some(format!("public.{}", index.relation_name())),
        MaintenanceTarget::Heap(_) => None,
    };
    let active = select_i64(connection, active_backend_statement()).await?;
    let maximum = select_i64(connection, maximum_connections_statement()).await?;
    let oldest = select_optional_instant(connection, oldest_transaction_statement()).await?;
    let active_maintenance = select_bool(connection, active_maintenance_statement(&heap)).await?;
    let waiting_relation_lock = select_bool(connection, waiting_lock_statement(&heap, relation.as_deref())).await?;
    let oldest_transaction_seconds = oldest.map_or(0, |started| {
        let seconds = Utc::now().signed_duration_since(started).num_seconds();
        u64::try_from(seconds).unwrap_or(0)
    });
    let active = non_negative(active, "active backends")?;
    let maximum = non_negative(maximum, "maximum connections")?;
    Ok(SafetyGates {
        active_backend_percent: if maximum == 0 { 100 } else { percent(active, maximum) },
        oldest_transaction_seconds,
        active_maintenance,
        waiting_relation_lock,
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

/// Read current physical bytes for before/after action reporting.
pub(crate) async fn relation_bytes(connection: &mut PgConnection, target: MaintenanceTarget) -> Result<u64, MaintenanceError> {
    let relation = match target {
        MaintenanceTarget::Index(index) => index.relation_name(),
        MaintenanceTarget::Heap(heap) => heap.relation_name(),
    };
    let statement = Query::select()
        .expr(total_relation_size(to_regclass(Expr::val(format!("public.{relation}")))))
        .to_owned();
    non_negative(select_i64(connection, statement).await?, "relation bytes")
}

/// `pg_extension` presence query.
fn extension_present_statement() -> SelectStatement {
    let inner = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_extension"))
        .and_where(Expr::col(Alias::new("extname")).eq("pgstattuple"))
        .to_owned();
    Query::select().expr(Expr::exists(inner)).to_owned()
}

/// Required `pgstattuple` function execution privileges.
fn inspection_privilege_statement() -> SelectStatement {
    let exact = current_user_has_function_privilege(to_regprocedure(Expr::val("pgstattuple(regclass)")), Expr::val("EXECUTE"));
    let approximate = current_user_has_function_privilege(to_regprocedure(Expr::val("pgstattuple_approx(regclass)")), Expr::val("EXECUTE"));
    Query::select().expr(exact.and(approximate)).to_owned()
}

/// Required PostgreSQL 18 `MAINTAIN` privileges.
fn maintain_privilege_statement() -> SelectStatement {
    let cav = current_user_has_table_privilege(Expr::val("public.current_attribute_values"), Expr::val("MAINTAIN"));
    let documents = current_user_has_table_privilege(Expr::val("public.entity_documents"), Expr::val("MAINTAIN"));
    let delivery = current_user_has_table_privilege(Expr::val("public.subscription_delivery_state"), Expr::val("MAINTAIN"));
    Query::select().expr(cav.and(documents).and(delivery)).to_owned()
}

/// Cheap catalog size query for every allowlisted index.
fn index_sizes_statement() -> SelectStatement {
    Query::select()
        .column(Alias::new(CatalogColumn::Indexrelname.as_str()))
        .expr_as(relation_size(Expr::col(Alias::new("indexrelid"))), Alias::new(CatalogColumn::Bytes.as_str()))
        .from(Alias::new("pg_stat_user_indexes"))
        .and_where(Expr::col(Alias::new("schemaname")).eq("public"))
        .and_where(Expr::col(Alias::new("relname")).eq("current_attribute_values"))
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

/// Detailed physical tuple query against one bound `regclass`.
fn pgstattuple_statement(relation: &str) -> SelectStatement {
    Query::select()
        .columns([Alias::new(CatalogColumn::FreeSpace.as_str()), Alias::new(CatalogColumn::FreePercent.as_str())])
        .from_function(pgstattuple(to_regclass(Expr::val(relation))), Alias::new("physical"))
        .to_owned()
}

/// Active foreground backend count.
fn active_backend_statement() -> SelectStatement {
    Query::select()
        .expr(Func::count(Expr::col(Asterisk)))
        .from(Alias::new("pg_stat_activity"))
        .and_where(Expr::col(Alias::new("state")).eq("active"))
        .and_where(Expr::col(Alias::new("pid")).ne(backend_pid()))
        .to_owned()
}

/// Oldest foreground transaction start.
fn oldest_transaction_statement() -> SelectStatement {
    Query::select()
        .expr(Func::min(Expr::col(Alias::new("xact_start"))))
        .from(Alias::new("pg_stat_activity"))
        .and_where(Expr::col(Alias::new("xact_start")).is_not_null())
        .and_where(Expr::col(Alias::new("pid")).ne(backend_pid()))
        .to_owned()
}

/// Configured maximum backend count.
fn maximum_connections_statement() -> SelectStatement {
    Query::select().expr(cast(current_setting("max_connections"), PostgresType::Bigint)).to_owned()
}

/// Whether PostgreSQL reports a build or vacuum on the relation.
fn active_maintenance_statement(relation: &str) -> SelectStatement {
    let build = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_stat_progress_create_index"))
        .and_where(Expr::col(Alias::new("relid")).eq(to_regclass(Expr::val(relation))))
        .to_owned();
    let vacuum = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_stat_progress_vacuum"))
        .and_where(Expr::col(Alias::new("relid")).eq(to_regclass(Expr::val(relation))))
        .to_owned();
    Query::select().expr(Expr::exists(build).or(Expr::exists(vacuum))).to_owned()
}

/// Whether another backend waits for a relation lock.
fn waiting_lock_statement(heap: &str, index: Option<&str>) -> SelectStatement {
    let waiting = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("pg_locks"))
        .and_where(match index {
            Some(index) => Expr::col(Alias::new("relation")).is_in([to_regclass(Expr::val(heap)), to_regclass(Expr::val(index))]),
            None => Expr::col(Alias::new("relation")).eq(to_regclass(Expr::val(heap))),
        })
        .and_where(Expr::col(Alias::new("granted")).eq(false))
        .to_owned();
    Query::select().expr(Expr::exists(waiting)).to_owned()
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

/// Execute a SeaQuery-built boolean scalar query.
async fn select_bool(connection: &mut PgConnection, statement: SelectStatement) -> Result<bool, MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?)
}

/// Execute a SeaQuery-built signed integer scalar query.
async fn select_i64(connection: &mut PgConnection, statement: SelectStatement) -> Result<i64, MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?)
}

/// Execute a SeaQuery-built optional instant scalar query.
async fn select_optional_instant(connection: &mut PgConnection, statement: SelectStatement) -> Result<Option<DateTime<Utc>>, MaintenanceError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    Ok(sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?)
}

/// Convert one PostgreSQL non-negative counter.
fn non_negative(value: i64, field: &'static str) -> Result<u64, MaintenanceError> {
    u64::try_from(value).map_err(|_| MaintenanceError::NegativeCatalogValue { field, value })
}

/// Compute a saturating integer percentage with bounded cardinality.
fn percent(numerator: u64, denominator: u64) -> u8 {
    let value = numerator.saturating_mul(100).checked_div(denominator).unwrap_or_default();
    u8::try_from(Ord::min(value, 100)).unwrap_or(100)
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
    /// Detailed index free-space percentage projection.
    FreePercent,
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
            CatalogColumn::FreePercent => "free_percent",
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::inspection::{index_sizes_statement, invalid_indexes_statement, percent};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn percentage_is_integer_and_bounded() {
        assert_eq!(percent(0, 10), 0);
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(20, 10), 100);
    }

    #[test]
    fn catalog_queries_are_seaquery_statements_with_closed_filters() {
        let index_sql = index_sizes_statement().to_string(PostgresQueryBuilder);
        assert!(index_sql.contains("pg_stat_user_indexes"), "{index_sql}");
        assert!(index_sql.contains("idx_cav_attr_string_trgm"), "{index_sql}");
        let invalid_sql = invalid_indexes_statement().to_string(PostgresQueryBuilder);
        assert!(invalid_sql.contains("pg_index"), "{invalid_sql}");
        assert!(invalid_sql.contains("indisvalid"), "{invalid_sql}");
    }
}
