//! Whether a driver error is the pool or the pooler at capacity.

use sqlx::error::DatabaseError;
use std::io::{Error as IoError, ErrorKind};

/// `PgBouncer`'s message when it disconnects a client whose transaction could
/// not be assigned a server backend within `query_wait_timeout`.
///
/// This is the pooled-deployment form of pool exhaustion: the fleet-wide backend
/// budget (`max_user_connections`) is spent, so the pooler queues the client and,
/// once the wait elapses, closes it with a FATAL carrying this message under
/// SQLSTATE `08P01`. That code is shared with genuine protocol violations and
/// with the pooler's other connection failures, so the message -- not the code
/// -- is what identifies pooler backpressure.
const POOLER_QUERY_WAIT_TIMEOUT: &str = "query_wait_timeout";

/// Whether a [`sqlx::Error`] reports that a connection could not be obtained
/// because the pool -- the broker's own or a transaction pooler in front of
/// PostgreSQL -- is at capacity.
///
/// This is backpressure, not a fault: the request never ran and can be retried
/// unchanged, so callers turn it into a 503 with `Retry-After` rather than a 500.
/// Three shapes count as saturation:
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
/// A server going away or not yet accepting work is not load but the store
/// being unreachable, which [`is_store_unavailable`] recognises. Every other
/// `sqlx::Error` is not saturation.
///
/// [`is_store_unavailable`]: crate::store_unavailability::is_store_unavailable
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
pub(crate) fn is_pooler_backpressure(error: &dyn DatabaseError) -> bool {
    error.message().contains(POOLER_QUERY_WAIT_TIMEOUT)
}

/// Whether an I/O error is the host running out of the local sockets a new
/// connection needs, rather than a transport failure.
///
/// A working set larger than the runtime cache opens and closes per-tenant pools
/// continuously; the churn can exhaust the host's ephemeral ports
/// (`EADDRNOTAVAIL`) or transiently collide on one (`EADDRINUSE`). Both are
/// load-induced and clear as the churn subsides and sockets leave `TIME_WAIT`, so
/// the request is shed as load rather than reported as the store being
/// unreachable. Any other I/O error -- a reset, a broken pipe, an unexpected EOF
/// -- is the connection to the store failing.
pub(crate) fn is_connection_resource_exhausted(error: &IoError) -> bool {
    matches!(error.kind(), ErrorKind::AddrNotAvailable | ErrorKind::AddrInUse)
}

#[cfg(test)]
mod tests {
    use crate::{
        fake_database_error::FakeDatabaseError,
        saturation::{POOLER_QUERY_WAIT_TIMEOUT, is_pool_saturated, is_pooler_backpressure},
    };
    use std::io::{Error as IoError, ErrorKind};

    /// A PgBouncer-shaped database error: SQLSTATE `08P01` with the given message.
    fn pgbouncer_error(message: &str) -> sqlx::Error {
        FakeDatabaseError::new(message, "08P01").into_sqlx()
    }

    /// A database error reporting the given SQLSTATE.
    fn database_error(code: &str) -> sqlx::Error {
        FakeDatabaseError::new("going away", code).into_sqlx()
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
        let error = FakeDatabaseError::new(POOLER_QUERY_WAIT_TIMEOUT, "08P01");
        assert!(is_pooler_backpressure(&error));
        assert!(is_pool_saturated(&pgbouncer_error(POOLER_QUERY_WAIT_TIMEOUT)));
    }

    #[test]
    fn a_database_error_sharing_the_sqlstate_is_not_saturation() {
        // 08P01 (protocol_violation) is shared with genuine protocol faults, so a
        // database error under that code with an unrelated message is not
        // backpressure: the message is what distinguishes them.
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
        // A reset, a broken pipe or an unexpected EOF is the connection failing,
        // which is the store being unreachable rather than load.
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

    #[test]
    fn a_server_going_away_is_no_longer_saturation() {
        // A backend an administrator terminated, a crash reset and a server
        // starting or stopping are the store being unreachable, not load.
        for code in ["57P01", "57P02", "57P03"] {
            assert!(!is_pool_saturated(&database_error(code)), "{code}");
        }
        assert!(!is_pool_saturated(&database_error("57014")));
        assert!(!is_pool_saturated(&database_error("23505")));
    }
}
