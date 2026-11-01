//! Reading a tenant's own database for its operational usage.
//!
//! Implements the tenancy crate's [`TenantUsageProbe`] port over a transient
//! one-connection pool on the tenant's request URL, so a usage read adds no
//! permanent backend and never makes the tenant resident -- the expensive
//! `?options=usage` half of tenant introspection stays off the dormancy path.

use crate::store_fault::persistence_failure;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_pool::{database_url::DatabaseUrl, pool_options::configure_pool_options};
use andromeda_postgresql_schema::{current::EntitySearch, snapshot::Snapshots};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    introspection::{current_database, current_database_size},
    statement::Statement,
};
use andromeda_tenancy::{error::TenancyError, tenant_usage::TenantUsage, usage_probe::TenantUsageProbe};
use async_trait::async_trait;
use sea_query::{Alias, Expr, ExprTrait, Func, Query, SelectStatement};
use sqlx::PgPool;
use std::{num::TryFromIntError, time::Duration};

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

/// A usage count the tenant database answered below zero.
#[derive(Debug, thiserror::Error)]
#[error("a tenant usage count read {value}, below zero")]
struct NegativeUsageCount {
    /// The count as the database answered it.
    value: i64,
    /// Why it does not widen to an unsigned count.
    #[source]
    source: TryFromIntError,
}

/// Run one single-scalar `bigint` count over the probe pool and widen it.
async fn fetch_count(pool: &PgPool, statement: SelectStatement) -> Result<u64, TenancyError> {
    let value: i64 = statement.to_query_scalar().fetch_one(pool).await.map_err(|error| usage_read_failure(&error))?;
    usage_count(value)
}

/// Widen one usage count.
///
/// A size, a backend count and a row count are never negative, so a negative
/// answer is the store breaking its contract, not an empty tenant: reporting
/// it as zero would hide the fault behind a plausible usage.
fn usage_count(value: i64) -> Result<u64, TenancyError> {
    u64::try_from(value).map_err(|source| TenancyError::RegistryUnavailable(PersistenceFailure::ContractBroken.record(&NegativeUsageCount { value, source })))
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
    use crate::tenant_usage::{backend_count_query, database_size_query, entity_count_query, snapshot_count_query, usage_count};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_tenancy::error::TenancyError;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn a_usage_count_widens_unchanged() {
        assert!(matches!(usage_count(0), Ok(0)));
        assert!(matches!(usage_count(4_096), Ok(4_096)));
    }

    #[test]
    fn a_negative_usage_count_is_a_broken_contract_not_zero() {
        assert!(matches!(
            usage_count(-1),
            Err(TenancyError::RegistryUnavailable(PersistenceFailure::ContractBroken))
        ));
    }

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
