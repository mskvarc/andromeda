//! Reading a tenant's own database for its operational usage.
//!
//! Implements the tenancy crate's [`TenantUsageProbe`] port over a transient
//! one-connection pool on the tenant's request URL, so a usage read adds no
//! permanent backend and never makes the tenant resident -- the expensive
//! `?options=usage` half of tenant introspection stays off the dormancy path.

use crate::{store_fault::persistence_failure, tenant_database::DatabaseUrl};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_pool::pool_options::configure_pool_options;
use andromeda_postgresql_schema::{current::EntitySearch, snapshot::Snapshots};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    introspection::{current_database, current_database_size},
};
use andromeda_tenancy::{database_name::TenantDatabaseName, error::TenancyError, tenant_usage::TenantUsage, usage_probe::TenantUsageProbe};
use async_trait::async_trait;
use sea_query::{Alias, Expr, ExprTrait, Func, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::PgPool;
use std::time::Duration;

/// Reads a tenant's own database for its operational usage.
///
/// Holds the request base URL -- the pooler when one is configured -- from which
/// each tenant's connection string is derived, the acquire deadline the transient
/// probe pool gives up after, and the configured snapshot-count ceiling reported
/// as `snapshotQuota`.
pub struct PostgresTenantUsageProbe {
    /// The request base URL every per-tenant probe connection is derived from.
    request_url: DatabaseUrl,
    /// How long the transient probe pool waits for a connection before giving up.
    acquire_timeout: Duration,
    /// The configured ceiling on a tenant's snapshot count, reported verbatim.
    snapshot_count_quota: Option<u64>,
}

impl PostgresTenantUsageProbe {
    /// A usage probe over the given request base URL, acquire deadline and
    /// snapshot-count ceiling.
    #[must_use]
    pub const fn new(request_url: DatabaseUrl, acquire_timeout: Duration, snapshot_count_quota: Option<u64>) -> PostgresTenantUsageProbe {
        PostgresTenantUsageProbe {
            request_url,
            acquire_timeout,
            snapshot_count_quota,
        }
    }

    /// Run the four usage reads over the transient pool and assemble the result.
    async fn read_over(&self, pool: &PgPool) -> Result<TenantUsage, TenancyError> {
        let database_size = fetch_count(pool, database_size_query()).await?;
        let pool_connections = fetch_count(pool, backend_count_query()).await?;
        let entity_count = fetch_count(pool, entity_count_query()).await?;
        let snapshot_count = fetch_count(pool, snapshot_count_query()).await?;
        Ok(TenantUsage {
            pool_connections,
            database_size,
            entity_count,
            snapshot_count,
            snapshot_quota: self.snapshot_count_quota,
        })
    }
}

#[async_trait]
impl TenantUsageProbe for PostgresTenantUsageProbe {
    async fn usage(&self, database: &TenantDatabaseName) -> Result<TenantUsage, TenancyError> {
        let url = self.request_url.with_database_name(database);
        let pool = configure_pool_options(1, self.acquire_timeout)
            .connect(url.as_str())
            .await
            .map_err(|error| usage_read_failure(&error))?;
        let usage = self.read_over(&pool).await;
        pool.close().await;
        usage
    }
}

/// Run one single-scalar `bigint` count over the probe pool and widen it.
///
/// A negative value cannot arise from any of these counts, so a value that does
/// not fit `u64` clamps to zero rather than propagating a nonsensical size.
async fn fetch_count(pool: &PgPool, statement: SelectStatement) -> Result<u64, TenancyError> {
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let value: i64 = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(pool)
        .await
        .map_err(|error| usage_read_failure(&error))?;
    Ok(u64::try_from(value).unwrap_or(0))
}

/// The failure a usage read becomes.
#[track_caller]
fn usage_read_failure(error: &sqlx::Error) -> TenancyError {
    TenancyError::RegistryUnavailable(persistence_failure(error))
}

/// The total on-disk size of the tenant's database, in bytes.
fn database_size_query() -> SelectStatement {
    Query::select().expr(current_database_size()).to_owned()
}

/// The number of PostgreSQL backends currently attached to the tenant's database.
fn backend_count_query() -> SelectStatement {
    Query::select()
        .expr(count_all())
        .from(Alias::new("pg_stat_activity"))
        .and_where(Expr::col(Alias::new("datname")).eq(current_database()))
        .to_owned()
}

/// The number of current entities the tenant holds.
fn entity_count_query() -> SelectStatement {
    Query::select()
        .expr(count_all())
        .from(table_of(&ContextTarget::Live, EntitySearch::Table))
        .to_owned()
}

/// The number of snapshots the tenant holds.
fn snapshot_count_query() -> SelectStatement {
    Query::select()
        .expr(Func::count(Expr::col(Snapshots::SnapshotKey)))
        .from(Snapshots::Table)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::tenant_usage::{backend_count_query, database_size_query, entity_count_query, snapshot_count_query};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_database_size_reads_the_current_database() {
        let sql = database_size_query().to_string(PostgresQueryBuilder);
        assert!(sql.contains("pg_database_size(current_database())"), "{sql}");
    }

    #[test]
    fn the_backend_count_filters_pg_stat_activity_by_current_database() {
        let sql = backend_count_query().to_string(PostgresQueryBuilder);
        assert!(sql.contains("COUNT(*)"), "{sql}");
        assert!(sql.contains("\"pg_stat_activity\""), "{sql}");
        assert!(sql.contains("\"datname\""), "{sql}");
        assert!(sql.contains("current_database()"), "{sql}");
    }

    #[test]
    fn the_entity_count_reads_the_live_entity_search_table() {
        let sql = entity_count_query().to_string(PostgresQueryBuilder);
        assert!(sql.contains("COUNT(*)"), "{sql}");
        assert!(sql.contains("\"entity_search\""), "{sql}");
    }

    #[test]
    fn the_snapshot_count_reads_the_snapshots_table() {
        let sql = snapshot_count_query().to_string(PostgresQueryBuilder);
        assert!(sql.contains("COUNT"), "{sql}");
        assert!(sql.contains("\"snapshots\""), "{sql}");
    }
}
