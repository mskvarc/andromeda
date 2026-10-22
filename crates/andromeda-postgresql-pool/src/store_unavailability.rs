//! Whether a driver error says the store cannot be reached right now.

use crate::{
    saturation::{is_connection_resource_exhausted, is_pooler_backpressure},
    sql_state::SqlState,
};
use sqlx::error::DatabaseError;

/// Whether a [`sqlx::Error`] reports that the store could not be reached, or
/// that the connection to it was lost, rather than a fault of the statement.
///
/// Nothing of the operation is known to have persisted, so it can be sent
/// again once the store is back. Saturation is decided before this, so a
/// pooler's `query_wait_timeout` and the host's socket exhaustion, which share
/// these shapes, are never reported here. The shapes are:
///
/// - a [`sqlx::Error::Database`] whose SQLSTATE says the server is going away
///   or not yet accepting work ([`is_server_going_away`]);
/// - a [`sqlx::Error::Database`] in SQLSTATE class `08`, a connection
///   exception, which is also what a transaction pooler reports while it cannot
///   log in to the server;
/// - a [`sqlx::Error::Io`] of any kind: the connection was refused, reset or
///   closed under the statement;
/// - [`sqlx::Error::Tls`] and [`sqlx::Error::Protocol`]: the connection could
///   not be established or was cut mid-message;
/// - [`sqlx::Error::PoolClosed`]: the pool is shutting down, so the request
///   belongs on another instance or a retry;
/// - [`sqlx::Error::WorkerCrashed`] and [`sqlx::Error::BeginFailed`]: the
///   connection's worker or its transaction start failed.
#[must_use]
pub fn is_store_unavailable(error: &sqlx::Error) -> bool {
    if let sqlx::Error::Database(database_error) = error {
        let database_error = database_error.as_ref();
        return !is_pooler_backpressure(database_error) && (is_server_going_away(database_error) || is_connection_exception(database_error));
    }
    if let sqlx::Error::Io(io_error) = error {
        return !is_connection_resource_exhausted(io_error);
    }
    matches!(
        error,
        sqlx::Error::Tls(_) | sqlx::Error::Protocol(_) | sqlx::Error::PoolClosed | sqlx::Error::WorkerCrashed | sqlx::Error::BeginFailed
    )
}

/// Whether a database error says the server cannot serve the request right now.
///
/// SQLSTATE 57P01 (`admin_shutdown`: the backend was terminated by an
/// administrator), 57P02 (`crash_shutdown`: another backend crashed and the
/// server is resetting) and 57P03 (`cannot_connect_now`: the server is starting
/// up or shutting down) all describe the server, not the request. The
/// backend's transaction was rolled back with it.
fn is_server_going_away(error: &dyn DatabaseError) -> bool {
    matches!(
        SqlState::of_database(error),
        Some(SqlState::AdminShutdown | SqlState::CrashShutdown | SqlState::CannotConnectNow)
    )
}

/// Whether a database error is in SQLSTATE class `08`, a connection exception.
fn is_connection_exception(error: &dyn DatabaseError) -> bool {
    error.code().is_some_and(|code| code.starts_with("08"))
}

#[cfg(test)]
mod tests {
    use crate::{fake_database_error::FakeDatabaseError, store_unavailability::is_store_unavailable};
    use std::io::{Error as IoError, ErrorKind};

    /// A database error reporting the given SQLSTATE.
    fn database_error(code: &str) -> sqlx::Error {
        FakeDatabaseError::new("connection failure", code).into_sqlx()
    }

    #[test]
    fn an_administrator_or_crash_shutdown_or_startup_is_the_store_being_unavailable() {
        for code in ["57P01", "57P02", "57P03"] {
            assert!(is_store_unavailable(&database_error(code)), "{code}");
        }
    }

    #[test]
    fn every_connection_exception_is_the_store_being_unavailable() {
        for code in ["08000", "08003", "08006", "08001", "08004"] {
            assert!(is_store_unavailable(&database_error(code)), "{code}");
        }
        let login_retry = FakeDatabaseError::new("server login has been failing, try again later (server_login_retry)", "08P01");
        assert!(is_store_unavailable(&login_retry.into_sqlx()));
    }

    #[test]
    fn a_pooler_query_wait_timeout_is_not_unavailability() {
        let wait_timeout = FakeDatabaseError::new("query_wait_timeout", "08P01");
        assert!(!is_store_unavailable(&wait_timeout.into_sqlx()));
    }

    #[test]
    fn a_transport_failure_is_the_store_being_unavailable() {
        for kind in [
            ErrorKind::ConnectionRefused,
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::BrokenPipe,
            ErrorKind::UnexpectedEof,
            ErrorKind::NotConnected,
            ErrorKind::TimedOut,
        ] {
            assert!(is_store_unavailable(&sqlx::Error::Io(IoError::from(kind))), "{kind}");
        }
        assert!(is_store_unavailable(&sqlx::Error::PoolClosed));
        assert!(is_store_unavailable(&sqlx::Error::WorkerCrashed));
        assert!(is_store_unavailable(&sqlx::Error::BeginFailed));
    }

    #[test]
    fn local_socket_exhaustion_is_not_unavailability() {
        assert!(!is_store_unavailable(&sqlx::Error::Io(IoError::from(ErrorKind::AddrNotAvailable))));
        assert!(!is_store_unavailable(&sqlx::Error::Io(IoError::from(ErrorKind::AddrInUse))));
    }

    #[test]
    fn a_statement_timeout_or_constraint_is_not_unavailability() {
        for code in ["57014", "23505", "40P01"] {
            assert!(!is_store_unavailable(&database_error(code)), "{code}");
        }
        assert!(!is_store_unavailable(&sqlx::Error::RowNotFound));
    }
}
