//! Cleaning up between tier runs: evicting cached runtimes and dropping the
//! tenant databases they refer to.
//!
//! A tenant database cannot be dropped while a broker holds its pool open, so
//! cleanup first asks every broker to evict the runtimes whose registry row is
//! gone, then force-drops the databases. Every step is best-effort: a run's
//! setup calls it to start from a clean slate and its teardown to leave one.

use reqwest::Client;
use sqlx::PgPool;

/// The default per-tenant database name prefix the broker provisions under.
pub const DEFAULT_TENANT_DB_PREFIX: &str = "andromeda_tenant";

/// Ask one broker to evict every runtime whose registry row was removed, so it
/// closes those tenants' pools. Best-effort: the outcome is ignored.
pub async fn evict_orphans(client: &Client, broker_base: &str) {
    let _ = client.post(format!("{broker_base}tenants/evictOrphans")).send().await;
}

/// Ask every broker to evict its orphaned runtimes.
pub async fn evict_orphans_on(client: &Client, broker_bases: &[&str]) {
    for broker_base in broker_bases {
        evict_orphans(client, broker_base).await;
    }
}

/// Remove every named tenant's platform registry row so the brokers treat its
/// runtime as an orphan and evict it, releasing the pool that would otherwise
/// keep the tenant's database from being dropped.
///
/// Dropping a tenant database out from under a live broker (as the tests do) does
/// not touch the registry, so the broker keeps a dead runtime whose pool still
/// counts against the pooler's backend budget. Deleting the registry row first is
/// what lets [`evict_orphans_on`] close that pool. The delete cascades to the
/// wake-schedule and maintenance rows that reference the tenant. The unnamed
/// default is never removed.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] the delete reported.
pub async fn deprovision_registry(pool: &PgPool, prefix: &str) -> Result<(), sqlx::Error> {
    let pattern = format!("{prefix}%");
    sqlx::query("DELETE FROM tenants WHERE is_unnamed_default = FALSE AND db_name LIKE $1")
        .bind(&pattern)
        .execute(pool)
        .await?;
    Ok(())
}

/// Drop every database whose name begins with `prefix`, forcing off any
/// remaining backends. The control database is never dropped.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] from listing the databases; individual drops are
/// best-effort and their failures are ignored (a database a backend still holds
/// is retried on the next run's cleanup).
pub async fn drop_tenant_databases(pool: &PgPool, prefix: &str) -> Result<(), sqlx::Error> {
    let current: String = sqlx::query_scalar("SELECT current_database()").fetch_one(pool).await?;
    let pattern = format!("{prefix}%");
    let databases: Vec<String> = sqlx::query_scalar("SELECT datname FROM pg_database WHERE datname LIKE $1 AND datname <> $2")
        .bind(&pattern)
        .bind(&current)
        .fetch_all(pool)
        .await?;
    for database in databases {
        let statement = format!("DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)");
        let _ = sqlx::query(sqlx::AssertSqlSafe(statement)).execute(pool).await;
    }
    Ok(())
}

/// Deprovision the tenant registry, evict the now-orphaned runtimes on every
/// broker, then drop every tenant database under the default prefix. The one call
/// a run makes on entry and exit.
///
/// The order matters: removing the registry rows before asking the brokers to
/// evict is what makes them close the tenants' pools, so the databases can then
/// be dropped without a backend still holding them and without leaving dead
/// runtimes whose pools accumulate against the pooler's budget across a run.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] from the registry deprovision or the database drop
/// step.
pub async fn reset(client: &Client, broker_bases: &[&str], pool: &PgPool) -> Result<(), sqlx::Error> {
    deprovision_registry(pool, DEFAULT_TENANT_DB_PREFIX).await?;
    evict_orphans_on(client, broker_bases).await;
    drop_tenant_databases(pool, DEFAULT_TENANT_DB_PREFIX).await
}
