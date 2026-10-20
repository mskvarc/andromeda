//! Returning a connection to its pool after its request let go of it.
//!
//! A request whose future is dropped mid-statement, at the request deadline or
//! because its client went away, hands its connection back with the statement
//! still running on the server, and the pool would wait for that statement to
//! end before lending the connection again. The driver keeps the backend's
//! cancel key to itself, so a connection that is not ready again within
//! [`RELEASE_READINESS_DEADLINE`] is closed instead. The server ends the
//! statement of a session whose client closed at its next
//! `client_connection_check_interval` check; a transaction-pooling proxy closes
//! the server session of a client that disconnects while that session is busy,
//! so the statement ends behind one as well.

use std::{future::Future, time::Duration};
use tokio::time::timeout;

/// How long a returned connection may take to be ready for another statement
/// before it is closed.
///
/// A connection whose statement finished answers the readiness probe in one
/// round trip; one that misses this bound still runs a statement its request
/// no longer waits for.
pub const RELEASE_READINESS_DEADLINE: Duration = Duration::from_secs(1);

/// What a returned connection is fit for.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleasedConnection {
    /// Ready for the next statement: the pool lends it again.
    Ready,
    /// Still running a statement past the deadline: the pool closes it.
    Busy,
}

impl ReleasedConnection {
    /// Whether the pool keeps the connection, as its release hook answers.
    #[must_use]
    pub const fn is_kept(self) -> bool {
        match self {
            ReleasedConnection::Ready => true,
            ReleasedConnection::Busy => false,
        }
    }
}

/// Wait at most `deadline` for `readiness`, the connection's readiness probe.
///
/// # Errors
///
/// Returns the probe's error: the connection is broken and the pool drops it.
pub async fn released_connection(readiness: impl Future<Output = Result<(), sqlx::Error>>, deadline: Duration) -> Result<ReleasedConnection, sqlx::Error> {
    match timeout(deadline, readiness).await {
        Ok(ready) => ready.map(|()| ReleasedConnection::Ready),
        Err(_elapsed) => Ok(ReleasedConnection::Busy),
    }
}

#[cfg(test)]
mod tests {
    use crate::connection_release::{ReleasedConnection, released_connection};
    use std::{
        future::{pending, ready},
        time::Duration,
    };

    #[tokio::test]
    async fn a_connection_ready_within_the_deadline_is_kept() {
        let released = released_connection(ready(Ok(())), Duration::from_secs(1)).await;
        assert!(matches!(released, Ok(ReleasedConnection::Ready)), "{released:?}");
        assert!(ReleasedConnection::Ready.is_kept());
    }

    #[tokio::test]
    async fn a_connection_still_running_a_statement_past_the_deadline_is_closed() {
        let released = released_connection(pending(), Duration::from_millis(10)).await;
        assert!(matches!(released, Ok(ReleasedConnection::Busy)), "{released:?}");
        assert!(!ReleasedConnection::Busy.is_kept());
    }

    #[tokio::test]
    async fn a_connection_whose_probe_fails_is_reported_broken() {
        let released = released_connection(ready(Err(sqlx::Error::PoolClosed)), Duration::from_secs(1)).await;
        assert!(matches!(released, Err(sqlx::Error::PoolClosed)), "{released:?}");
    }
}
