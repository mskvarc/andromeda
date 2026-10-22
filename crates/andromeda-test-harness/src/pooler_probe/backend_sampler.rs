//! Background sampling of `PostgreSQL` client-backend counts.
//!
//! The transaction-pooling tier's central claim is about how many `PostgreSQL`
//! backends a broker holds open. This samples `pg_stat_activity` over a direct
//! connection (never through the pooler) at a fixed cadence for the life of a
//! workload, then reports the peak and median so a test can assert the fleet
//! stayed under a connection budget.
//!
//! Every broker on the tier shares one `PostgreSQL`, so counts are attributed by
//! `application_name`: each broker connects under its own (see
//! [`endpoints`](crate::pooler_probe::endpoints)), and `PgBouncer` carries it onto
//! the server backend it leases. A backend that renames its own session, such as
//! the maintenance controller's, is not attributed to any broker.

use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    pg_array::{any, text_array},
    statement::Statement,
    system_catalog::PgStatActivity,
};
use sea_query::{Expr, ExprTrait, Query, SimpleExpr};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{task::JoinHandle, time::sleep};

/// How often the sampler reads `pg_stat_activity`.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// The peak and median client-backend counts observed across a sampling run.
#[derive(Debug, Clone, Copy)]
pub struct BackendSeries {
    /// The highest client-backend count seen at any sample.
    pub max: i64,
    /// The median client-backend count across all samples.
    pub p50: i64,
    /// How many samples were taken.
    pub samples: usize,
}

/// A running background sampler of `PostgreSQL` client-backend counts.
///
/// Started with [`BackendSampler::start`] before a workload and consumed with
/// [`BackendSampler::stop`] after it, which returns the observed [`BackendSeries`].
pub struct BackendSampler {
    /// Set to end the sampling loop.
    stop: Arc<AtomicBool>,
    /// The sampling task, which yields the raw sample series when it ends.
    handle: JoinHandle<Vec<i64>>,
}

impl BackendSampler {
    /// Begin sampling, over a direct connection to `database_url`, the client
    /// backends connected under any of `application_names`.
    ///
    /// # Errors
    ///
    /// Returns the [`sqlx::Error`] from opening the sampling connection.
    pub async fn start(database_url: &str, application_names: &'static [&'static str]) -> Result<BackendSampler, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await?;
        let stop = Arc::new(AtomicBool::new(false));
        let task_stop = Arc::clone(&stop);
        let handle = tokio::spawn(async move {
            let mut samples = Vec::new();
            while !task_stop.load(Ordering::Relaxed) {
                if let Ok(count) = client_backends(&pool, application_names).await {
                    samples.push(count);
                }
                sleep(SAMPLE_INTERVAL).await;
            }
            samples
        });
        Ok(BackendSampler { stop, handle })
    }

    /// Stop sampling and report the peak and median client-backend counts.
    pub async fn stop(self) -> BackendSeries {
        self.stop.store(true, Ordering::Relaxed);
        let mut samples = self.handle.await.unwrap_or_default();
        let count = samples.len();
        let max = samples.iter().copied().max().unwrap_or(0);
        samples.sort_unstable();
        let p50 = samples.get(count / 2).copied().unwrap_or(0);
        BackendSeries { max, p50, samples: count }
    }
}

/// The number of client backends connected under any of `application_names`,
/// over `pool`.
///
/// Counts only `client backend` activity, excluding the autovacuum launcher,
/// the background writer, and the `TimescaleDB` scheduler workers, which are not
/// what a connection-budget assertion is about.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] the count query reported.
pub async fn client_backends(pool: &PgPool, application_names: &[&str]) -> Result<i64, sqlx::Error> {
    client_backend_count(pool, [connected_under(application_names)]).await
}

/// The number of client backends bound to database `datname`, over `pool`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] the count query reported.
pub async fn client_backends_for(pool: &PgPool, datname: &str) -> Result<i64, sqlx::Error> {
    client_backend_count(pool, [bound_to(datname)]).await
}

/// The number of client backends bound to database `datname` and connected
/// under any of `application_names`, over `pool`.
///
/// Attributes a tenant database's backends to the brokers that opened them:
/// every broker on the tier shares the registry, so another broker's wake probe
/// or its pooler's server connections may touch the same database.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] the count query reported.
pub async fn client_backends_of(pool: &PgPool, datname: &str, application_names: &[&str]) -> Result<i64, sqlx::Error> {
    client_backend_count(pool, [bound_to(datname), connected_under(application_names)]).await
}

/// The number of client backends `filters` all hold for.
async fn client_backend_count(pool: &PgPool, filters: impl IntoIterator<Item = SimpleExpr>) -> Result<i64, sqlx::Error> {
    let mut statement = Query::select();
    statement
        .expr(count_all())
        .from(PgStatActivity::Table)
        .and_where(Expr::col(PgStatActivity::BackendType).eq("client backend"));
    for filter in filters {
        statement.and_where(filter);
    }
    statement.to_query_scalar().fetch_one(pool).await
}

/// Whether the session is bound to database `datname`.
fn bound_to(datname: &str) -> SimpleExpr {
    Expr::col(PgStatActivity::Datname).eq(datname)
}

/// Whether the session runs under any of `application_names`.
fn connected_under(application_names: &[&str]) -> SimpleExpr {
    Expr::col(PgStatActivity::ApplicationName).eq(any(text_array(application_names)))
}
