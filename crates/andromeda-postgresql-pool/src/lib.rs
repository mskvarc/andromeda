//! Shared PostgreSQL connection-pool tuning and saturation detection.
//!
//! Every pool the broker opens — the main request pool, per-tenant serving
//! pools, and the one-connection migration and maintenance pools — is built
//! from [`configure_pool_options`] so durability-neutral tuning (idle reclaim,
//! no warm minimum, the acquire deadline) is decided once rather than per call
//! site. The crate is a leaf below the context and temporal stores: it knows
//! `sqlx` and nothing else, so both stores can share it without either
//! depending on the other.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

/// How long an idle connection is kept before the pool closes it.
///
/// Tenant pools outlive their bursts by design (the runtime cache holds them
/// for minutes), so without this every tenant a broker ever touched would pin
/// its full pool against the server's `max_connections`.
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Build a [`PgPoolOptions`] with the broker's shared pool tuning.
///
/// Centralised so every pool the broker opens reclaims idle connections on the
/// same schedule, keeps none warm, and fails a checkout that cannot be served
/// within `acquire_timeout` rather than blocking until the request deadline.
/// The acquire deadline is what turns pool exhaustion into a fast, typed
/// backpressure signal ([`is_pool_saturated`]) instead of a stalled request:
/// the caller sets it below the HTTP request timeout so a saturated pool
/// surfaces as a 503 with `Retry-After` before the request itself times out.
///
/// Durability (`synchronous_commit`) is not set on the connection: it is a
/// per-database default installed once with `ALTER DATABASE ... SET`, so a
/// connection borrowed from a transaction-pooling proxy inherits it without any
/// session `SET` that the proxy could leak to the next client.
#[must_use]
pub fn configure_pool_options(max_connections: u32, acquire_timeout: Duration) -> PgPoolOptions {
    PgPoolOptions::new()
        .min_connections(0)
        .max_connections(max_connections)
        .idle_timeout(POOL_IDLE_TIMEOUT)
        .acquire_timeout(acquire_timeout)
}

/// Whether a [`sqlx::Error`] reports that no connection could be acquired from
/// the pool within its acquire deadline.
///
/// This is backpressure, not a fault: the pool is at capacity and the request
/// was shed before it ran. Callers turn it into a 503 with `Retry-After` rather
/// than a 500, because the request is well-formed and can be retried unchanged.
/// Every other `sqlx::Error` — a closed pool, a crashed worker, a protocol or
/// database error — is a genuine failure and is not saturation.
#[must_use]
pub const fn is_pool_saturated(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::PoolTimedOut)
}

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["sqlx"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();

#[cfg(test)]
mod tests {
    use crate::{POOL_IDLE_TIMEOUT, configure_pool_options, is_pool_saturated};
    use std::time::Duration;

    #[test]
    fn pool_options_reclaim_idle_connections_keep_none_warm_and_bound_the_acquire() {
        let options = configure_pool_options(7, Duration::from_secs(5));
        assert_eq!(options.get_max_connections(), 7);
        assert_eq!(options.get_min_connections(), 0);
        assert_eq!(options.get_idle_timeout(), Some(POOL_IDLE_TIMEOUT));
        assert_eq!(options.get_acquire_timeout(), Duration::from_secs(5));
    }

    #[test]
    fn a_pool_acquire_timeout_is_the_one_saturation_signal() {
        assert!(is_pool_saturated(&sqlx::Error::PoolTimedOut));
    }

    #[test]
    fn a_closed_pool_or_row_absence_is_not_saturation() {
        assert!(!is_pool_saturated(&sqlx::Error::PoolClosed));
        assert!(!is_pool_saturated(&sqlx::Error::RowNotFound));
        assert!(!is_pool_saturated(&sqlx::Error::WorkerCrashed));
    }
}
