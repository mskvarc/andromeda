//! Bounded waiting on the locks a schema change takes.
//!
//! A schema change (a migration, the storage-policy reconciler's `ALTER
//! TABLE`, an extension update) takes a strong lock on a relation the request
//! path is using. While it waits for that lock, every later statement on the
//! relation queues behind it, so an unbounded wait behind one long transaction
//! stalls the whole tenant. The sessions that change the schema therefore run
//! under [`SCHEMA_CHANGE_LOCK_TIMEOUT`]: a lock not granted within it fails
//! the statement with SQLSTATE `55P03` (`lock_not_available`), the queue behind
//! it drains, and [`LockRetry::run`] tries again after a pause, a bounded
//! number of times.

use sqlx::{error::DatabaseError, postgres::PgConnectOptions};
use std::{future::Future, num::NonZeroU32, str::FromStr, time::Duration};
use tokio::time::sleep;

/// The longest a schema-change statement waits for one lock.
///
/// Short against the request deadline: the request path's statements on the
/// relation wait behind the queued lock request for at most this long.
pub const SCHEMA_CHANGE_LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// The retry every schema change runs under: five attempts two seconds apart,
/// so a lock held by a transaction shorter than about ten seconds is still
/// taken, and one held longer fails the change rather than the tenant's
/// requests.
pub const SCHEMA_CHANGE_LOCK_RETRY: LockRetry = LockRetry {
    attempts: NonZeroU32::MIN.saturating_add(4),
    pause: Duration::from_secs(2),
};

/// How often, and how far apart, an operation refused a lock is run again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LockRetry {
    /// Runs in all, the first included.
    pub attempts: NonZeroU32,
    /// The pause before each run after the first.
    pub pause: Duration,
}

impl LockRetry {
    /// Run `operation`, running it again after [`pause`](Self::pause) while it
    /// fails with an error `is_lock_not_available` recognises and attempts
    /// remain.
    ///
    /// # Errors
    ///
    /// Returns the operation's error when it is not a refused lock, or the
    /// last refused lock once every attempt is spent.
    pub async fn run<T, E, F, Fut>(self, is_lock_not_available: impl Fn(&E) -> bool, mut operation: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let mut attempt = 1;
        loop {
            match operation().await {
                Err(error) if attempt < self.attempts.get() && is_lock_not_available(&error) => {
                    attempt += 1;
                    sleep(self.pause).await;
                }
                outcome => return outcome,
            }
        }
    }
}

/// Whether a driver error is a lock wait cut short by `lock_timeout`
/// (SQLSTATE `55P03`, `lock_not_available`).
#[must_use]
pub fn is_lock_not_available(error: &sqlx::Error) -> bool {
    error.as_database_error().and_then(DatabaseError::code).as_deref() == Some("55P03")
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
        lock_retry::{LockRetry, SCHEMA_CHANGE_LOCK_RETRY, is_lock_not_available, schema_change_connect_options},
    };
    use pretty_assertions::assert_eq;
    use std::{cell::Cell, num::NonZeroU32, time::Duration};

    /// Three attempts with no pause between them.
    const THREE_IMMEDIATE: LockRetry = LockRetry {
        attempts: NonZeroU32::MIN.saturating_add(2),
        pause: Duration::ZERO,
    };

    #[tokio::test]
    async fn a_refused_lock_is_run_again_until_it_succeeds() {
        let runs = Cell::new(0);
        let outcome = THREE_IMMEDIATE
            .run(is_lock_not_available, || {
                runs.set(runs.get() + 1);
                let run = runs.get();
                async move {
                    if run < 3 {
                        Err(FakeDatabaseError::new("canceling statement due to lock timeout", "55P03").into_sqlx())
                    } else {
                        Ok(run)
                    }
                }
            })
            .await;
        assert_eq!(outcome.ok(), Some(3));
        assert_eq!(runs.get(), 3);
    }

    #[tokio::test]
    async fn a_lock_refused_on_every_attempt_stops_at_the_bound_with_the_refusal() {
        let runs = Cell::new(0);
        let outcome: Result<(), sqlx::Error> = THREE_IMMEDIATE
            .run(is_lock_not_available, || {
                runs.set(runs.get() + 1);
                async { Err(FakeDatabaseError::new("canceling statement due to lock timeout", "55P03").into_sqlx()) }
            })
            .await;
        assert!(outcome.as_ref().is_err_and(is_lock_not_available), "{outcome:?}");
        assert_eq!(runs.get(), 3);
    }

    #[tokio::test]
    async fn any_other_error_is_returned_without_another_run() {
        for code in ["40P01", "57014", "42P01", "23505"] {
            let runs = Cell::new(0);
            let outcome: Result<(), sqlx::Error> = THREE_IMMEDIATE
                .run(is_lock_not_available, || {
                    runs.set(runs.get() + 1);
                    async move { Err(FakeDatabaseError::new("refused", code).into_sqlx()) }
                })
                .await;
            assert!(outcome.is_err(), "{code}");
            assert_eq!(runs.get(), 1, "{code}");
        }
    }

    #[test]
    fn only_lock_not_available_is_a_refused_lock() {
        assert!(is_lock_not_available(&FakeDatabaseError::new("lock timeout", "55P03").into_sqlx()));
        assert!(!is_lock_not_available(&FakeDatabaseError::new("deadlock detected", "40P01").into_sqlx()));
        assert!(!is_lock_not_available(&sqlx::Error::PoolTimedOut));
    }

    #[test]
    fn a_schema_change_runs_five_times_two_seconds_apart() {
        assert_eq!(SCHEMA_CHANGE_LOCK_RETRY.attempts.get(), 5);
        assert_eq!(SCHEMA_CHANGE_LOCK_RETRY.pause, Duration::from_secs(2));
    }

    #[test]
    fn a_schema_change_session_bounds_its_lock_waits_and_never_its_statements() {
        let options = schema_change_connect_options("postgres://broker@localhost:5432/tenant").expect("a PostgreSQL URL");
        assert_eq!(options.get_options(), Some("-c lock_timeout=2000ms -c statement_timeout=0"));
        assert_eq!(options.get_database(), Some("tenant"));
    }
}
