//! Bounded waiting on the locks a schema change takes.
//!
//! A schema change (a migration, the storage-policy reconciler's `ALTER
//! TABLE`, an extension update) takes a strong lock on a relation the request
//! path is using. While it waits for that lock, every later statement on the
//! relation queues behind it, so an unbounded wait behind one long transaction
//! stalls the whole tenant. The sessions that change the schema therefore run
//! under [`SCHEMA_CHANGE_LOCK_TIMEOUT`]: a lock not granted within it fails
//! the statement with SQLSTATE `55P03` (`lock_not_available`), the queue behind
//! it drains, and [`SCHEMA_CHANGE_LOCK_RETRY`] tries again after a pause, a
//! bounded number of times.

use crate::sql_state::SqlState;
use andromeda_retry::{retry_policy::RetryPolicy, schedule::BackoffSchedule};
use sqlx::postgres::PgConnectOptions;
use std::{num::NonZeroU32, str::FromStr, time::Duration};

/// The longest a schema-change statement waits for one lock.
///
/// Short against the request deadline: the request path's statements on the
/// relation wait behind the queued lock request for at most this long.
pub const SCHEMA_CHANGE_LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// The retry every schema change runs under: five attempts two seconds apart,
/// so a lock held by a transaction shorter than about ten seconds is still
/// taken, and one held longer fails the change rather than the tenant's
/// requests.
pub const SCHEMA_CHANGE_LOCK_RETRY: RetryPolicy = RetryPolicy {
    attempts: NonZeroU32::MIN.saturating_add(4),
    schedule: BackoffSchedule::constant(Duration::from_secs(2)),
};

/// Whether a driver error is a lock wait cut short by `lock_timeout`
/// (SQLSTATE `55P03`, `lock_not_available`).
#[must_use]
pub fn is_lock_not_available(error: &sqlx::Error) -> bool {
    SqlState::of(error) == Some(SqlState::LockNotAvailable)
}

/// The connection options of a session that changes the schema, straight to
/// PostgreSQL at `url`.
///
/// Every lock waits at most [`SCHEMA_CHANGE_LOCK_TIMEOUT`], and no statement
/// is cut short: the database's `statement_timeout` bounds request work, and a
/// migration building an index on a populated table outlasts any request. The
/// settings travel as startup options, which only a direct connection accepts;
/// these sessions never pass through a transaction pooler.
///
/// # Errors
///
/// Returns [`sqlx::Error::Configuration`] when `url` is not a PostgreSQL
/// connection URL.
pub fn schema_change_connect_options(url: &str) -> Result<PgConnectOptions, sqlx::Error> {
    Ok(PgConnectOptions::from_str(url)?.options([
        ("lock_timeout", format!("{}ms", SCHEMA_CHANGE_LOCK_TIMEOUT.as_millis())),
        ("statement_timeout", "0".to_owned()),
    ]))
}

#[cfg(test)]
mod tests {
    use crate::{
        fake_database_error::FakeDatabaseError,
        schema_change_lock::{SCHEMA_CHANGE_LOCK_RETRY, is_lock_not_available, schema_change_connect_options},
    };
    use andromeda_retry::schedule::BackoffSchedule;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn only_lock_not_available_is_a_refused_lock() {
        assert!(is_lock_not_available(&FakeDatabaseError::new("lock timeout", "55P03").into_sqlx()));
        assert!(!is_lock_not_available(&FakeDatabaseError::new("deadlock detected", "40P01").into_sqlx()));
        assert!(!is_lock_not_available(&sqlx::Error::PoolTimedOut));
    }

    #[test]
    fn a_schema_change_runs_five_times_two_seconds_apart() {
        assert_eq!(SCHEMA_CHANGE_LOCK_RETRY.attempts.get(), 5);
        assert_eq!(SCHEMA_CHANGE_LOCK_RETRY.schedule, BackoffSchedule::constant(Duration::from_secs(2)));
    }

    #[test]
    fn a_schema_change_session_bounds_its_lock_waits_and_never_its_statements() {
        let options = schema_change_connect_options("postgres://broker@localhost:5432/tenant").expect("a PostgreSQL URL");
        assert_eq!(options.get_options(), Some("-c lock_timeout=2000ms -c statement_timeout=0"));
        assert_eq!(options.get_database(), Some("tenant"));
    }
}
