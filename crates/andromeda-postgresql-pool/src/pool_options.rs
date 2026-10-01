//! The broker's shared pool tuning.

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
///
/// [`is_pool_saturated`]: crate::saturation::is_pool_saturated
#[must_use]
pub fn configure_pool_options(max_connections: u32, acquire_timeout: Duration) -> PgPoolOptions {
    PgPoolOptions::new()
        .min_connections(0)
        .max_connections(max_connections)
        .idle_timeout(POOL_IDLE_TIMEOUT)
        .acquire_timeout(acquire_timeout)
}

#[cfg(test)]
mod tests {
    use crate::pool_options::{POOL_IDLE_TIMEOUT, configure_pool_options};
    use std::time::Duration;

    #[test]
    fn pool_options_reclaim_idle_connections_keep_none_warm_and_bound_the_acquire() {
        let options = configure_pool_options(7, Duration::from_secs(5));
        assert_eq!(options.get_max_connections(), 7);
        assert_eq!(options.get_min_connections(), 0);
        assert_eq!(options.get_idle_timeout(), Some(POOL_IDLE_TIMEOUT));
        assert_eq!(options.get_acquire_timeout(), Duration::from_secs(5));
    }
}
