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

use sqlx::{error::DatabaseError, postgres::PgPoolOptions};
use std::{
    io::{Error as IoError, ErrorKind},
    time::Duration,
};

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

/// `PgBouncer`'s message when it disconnects a client whose transaction could
/// not be assigned a server backend within `query_wait_timeout`.
///
/// This is the pooled-deployment form of pool exhaustion: the fleet-wide backend
/// budget (`max_user_connections`) is spent, so the pooler queues the client and,
/// once the wait elapses, closes it with a FATAL carrying this message under
/// SQLSTATE `08P01`. That code is shared with genuine protocol violations, so the
/// message -- not the code -- is what identifies pooler backpressure.
const POOLER_QUERY_WAIT_TIMEOUT: &str = "query_wait_timeout";

/// Whether a [`sqlx::Error`] reports that a connection could not be obtained
/// because the pool -- the broker's own or a transaction pooler in front of
/// PostgreSQL -- is at capacity.
///
/// This is backpressure, not a fault: the request never ran and can be retried
/// unchanged, so callers turn it into a 503 with `Retry-After` rather than a 500.
/// Two shapes count as saturation:
///
/// - [`sqlx::Error::PoolTimedOut`]: the broker's own pool could not hand out a
///   connection within its acquire deadline.
/// - a [`sqlx::Error::Database`] carrying `PgBouncer`'s [`POOLER_QUERY_WAIT_TIMEOUT`]
///   message: the pooler could not assign a server backend within its
///   `query_wait_timeout` and closed the client. Without this, pooled-mode
///   saturation surfaces as a raw database error and is answered 500, defeating
///   the very backpressure contract the pooler tier exists to prove.
/// - a [`sqlx::Error::Io`] reporting that the host has no local socket to open
///   another connection ([`is_connection_resource_exhausted`]): a working set
///   larger than the runtime cache churns per-tenant pools fast enough to exhaust
///   the host's ephemeral ports, which is load, not a transport fault.
///
/// Every other `sqlx::Error` -- a closed pool, a crashed worker, any other
/// protocol or database error, or any other I/O fault -- is a genuine failure and
/// is not saturation.
#[must_use]
pub fn is_pool_saturated(error: &sqlx::Error) -> bool {
    if matches!(error, sqlx::Error::PoolTimedOut) {
        return true;
    }
    if let sqlx::Error::Database(database_error) = error {
        return is_pooler_backpressure(database_error.as_ref());
    }
    if let sqlx::Error::Io(io_error) = error {
        return is_connection_resource_exhausted(io_error);
    }
    false
}

/// Whether a database error is a transaction pooler's saturation signal.
///
/// Matches `PgBouncer`'s [`POOLER_QUERY_WAIT_TIMEOUT`] message rather than its
/// SQLSTATE, which it shares with unrelated protocol violations.
fn is_pooler_backpressure(error: &dyn DatabaseError) -> bool {
    error.message().contains(POOLER_QUERY_WAIT_TIMEOUT)
}

/// Whether an I/O error is the host running out of the local sockets a new
/// connection needs, rather than a genuine transport fault.
///
/// A working set larger than the runtime cache opens and closes per-tenant pools
/// continuously; the churn can exhaust the host's ephemeral ports
/// (`EADDRNOTAVAIL`) or transiently collide on one (`EADDRINUSE`). Both are
/// load-induced and clear as the churn subsides and sockets leave `TIME_WAIT`, so
/// the request is shed as retryable backpressure rather than reported an internal
/// fault. A persistent misconfiguration fails every connection alike at startup;
/// only load makes this appear intermittently mid-flight. Any other I/O error --
/// a reset, a broken pipe, an unexpected EOF -- may be a real transport failure
/// and is left a fault.
fn is_connection_resource_exhausted(error: &IoError) -> bool {
    matches!(error.kind(), ErrorKind::AddrNotAvailable | ErrorKind::AddrInUse)
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
    use crate::{POOL_IDLE_TIMEOUT, POOLER_QUERY_WAIT_TIMEOUT, configure_pool_options, is_pool_saturated, is_pooler_backpressure};
    use sqlx::error::{DatabaseError, ErrorKind as SqlxErrorKind};
    use std::{
        borrow::Cow,
        error::Error as StdError,
        fmt,
        io::{Error as IoError, ErrorKind},
        time::Duration,
    };

    /// A minimal [`DatabaseError`] carrying a fixed message and SQLSTATE, so the
    /// pooler-backpressure classification can be exercised without a live driver
    /// connection (a real `PgDatabaseError` cannot be constructed without one).
    #[derive(Debug)]
    struct FakeDatabaseError {
        /// The error message the classifier inspects.
        message: String,
        /// The SQLSTATE the error reports.
        code: String,
    }

    impl fmt::Display for FakeDatabaseError {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(formatter, "{}: {}", self.code, self.message)
        }
    }

    impl StdError for FakeDatabaseError {}

    impl DatabaseError for FakeDatabaseError {
        fn message(&self) -> &str {
            &self.message
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(&self.code))
        }

        fn as_error(&self) -> &(dyn StdError + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn StdError + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn StdError + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> SqlxErrorKind {
            SqlxErrorKind::Other
        }
    }

    /// A PgBouncer-shaped database error: SQLSTATE `08P01` with the given message.
    fn pgbouncer_error(message: &str) -> sqlx::Error {
        sqlx::Error::Database(Box::new(FakeDatabaseError {
            message: message.to_string(),
            code: "08P01".to_string(),
        }))
    }

    #[test]
    fn pool_options_reclaim_idle_connections_keep_none_warm_and_bound_the_acquire() {
        let options = configure_pool_options(7, Duration::from_secs(5));
        assert_eq!(options.get_max_connections(), 7);
        assert_eq!(options.get_min_connections(), 0);
        assert_eq!(options.get_idle_timeout(), Some(POOL_IDLE_TIMEOUT));
        assert_eq!(options.get_acquire_timeout(), Duration::from_secs(5));
    }

    #[test]
    fn a_pool_acquire_timeout_is_a_saturation_signal() {
        assert!(is_pool_saturated(&sqlx::Error::PoolTimedOut));
    }

    #[test]
    fn a_pgbouncer_query_wait_timeout_is_a_saturation_signal() {
        // PgBouncer closes a client it queued past query_wait_timeout with this
        // message under SQLSTATE 08P01: pooled-mode pool exhaustion, answered 503,
        // not the 500 an unrecognised database error would produce.
        let error = FakeDatabaseError {
            message: POOLER_QUERY_WAIT_TIMEOUT.to_string(),
            code: "08P01".to_string(),
        };
        assert!(is_pooler_backpressure(&error));
        assert!(is_pool_saturated(&pgbouncer_error(POOLER_QUERY_WAIT_TIMEOUT)));
    }

    #[test]
    fn a_database_error_sharing_the_sqlstate_is_not_saturation() {
        // 08P01 (protocol_violation) is shared with genuine protocol faults, so a
        // database error under that code with an unrelated message is a real
        // failure, not backpressure: the message is what distinguishes them.
        assert!(!is_pool_saturated(&pgbouncer_error("insufficient data left in message")));
    }

    #[test]
    fn ephemeral_port_exhaustion_is_a_saturation_signal() {
        // The host cannot allocate a local socket for another connection under the
        // pool churn of a working set larger than the cache: load, not a fault.
        assert!(is_pool_saturated(&sqlx::Error::Io(IoError::from(ErrorKind::AddrNotAvailable))));
        assert!(is_pool_saturated(&sqlx::Error::Io(IoError::from(ErrorKind::AddrInUse))));
    }

    #[test]
    fn an_ordinary_io_fault_is_not_saturation() {
        // A reset, a broken pipe or an unexpected EOF may be a genuine transport
        // failure, so it stays a fault rather than being shed as retryable.
        assert!(!is_pool_saturated(&sqlx::Error::Io(IoError::from(ErrorKind::ConnectionReset))));
        assert!(!is_pool_saturated(&sqlx::Error::Io(IoError::from(ErrorKind::UnexpectedEof))));
        assert!(!is_pool_saturated(&sqlx::Error::Io(IoError::from(ErrorKind::BrokenPipe))));
    }

    #[test]
    fn a_closed_pool_or_row_absence_is_not_saturation() {
        assert!(!is_pool_saturated(&sqlx::Error::PoolClosed));
        assert!(!is_pool_saturated(&sqlx::Error::RowNotFound));
        assert!(!is_pool_saturated(&sqlx::Error::WorkerCrashed));
    }
}
