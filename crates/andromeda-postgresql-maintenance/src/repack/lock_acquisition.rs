//! Taking a table lock under writes without stalling them.
//!
//! A lock request waits in the table's lock queue ahead of every later
//! request, so while an `ACCESS EXCLUSIVE` request waits, every writer of the
//! table queues behind it. Each attempt therefore waits at most
//! [`ATTEMPT_TIMEOUT`], which caps the stall a write sees, and attempts are
//! spaced by a growing back-off. Between attempts the log is replayed, so the
//! replay left for the swap, which runs under the lock, stays short. After
//! [`WAIT_BUDGET`] the run gives up: no other backend is ever cancelled to
//! make room for it.

use crate::repack::{log_replay, table_script::ReplayScript};
use andromeda_postgresql_pool::schema_change_lock::is_lock_not_available;
use andromeda_postgresql_seaquery::{
    command::Command,
    relation_lock::{LockAttempt, LockMode},
};
use sea_query::TableName;
use sqlx::{Acquire, PgConnection};
use std::time::{Duration, Instant};
use tokio::time;

/// The longest one attempt waits in the lock queue.
pub(crate) const ATTEMPT_TIMEOUT: Duration = Duration::from_millis(200);

/// How long a run keeps attempting before it gives up.
const WAIT_BUDGET: Duration = Duration::from_secs(60);

/// The back-off added per attempt.
const BACKOFF_STEP: Duration = Duration::from_millis(100);

/// The longest back-off between two attempts.
const BACKOFF_CEILING: Duration = Duration::from_secs(1);

/// What follows an attempt that timed out.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LockStep {
    /// Wait the back-off, then attempt again.
    Retry(Duration),
    /// Replay the log, then attempt again.
    Apply,
    /// Give up: the wait budget is spent.
    Abandon,
}

/// The step after attempt number `attempts` timed out, `elapsed` after the
/// first attempt began. Odd attempts back off, even ones replay the log.
pub(crate) fn next_lock_step(elapsed: Duration, attempts: u32) -> LockStep {
    if elapsed >= WAIT_BUDGET {
        return LockStep::Abandon;
    }
    if attempts.is_multiple_of(2) {
        LockStep::Apply
    } else {
        LockStep::Retry(BACKOFF_STEP.saturating_mul(attempts).min(BACKOFF_CEILING))
    }
}

/// How a run of attempts ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LockOutcome {
    /// The lock is held until the caller's transaction ends.
    Acquired,
    /// The wait budget is spent and nothing is held.
    Abandoned,
}

/// The log replayed between attempts, on the connection the replay runs on.
pub(crate) struct BetweenAttempts<'a> {
    /// The connection the replay runs on, outside the locking transaction.
    pub(crate) connection: &'a mut PgConnection,
    /// The table's replay statements.
    pub(crate) script: &'a ReplayScript,
}

/// Take `mode` on `relations` in the transaction `connection` has open, in
/// bounded attempts, each inside a savepoint so a timed-out attempt leaves
/// the transaction usable. `between`, when given, is replayed on the steps
/// that ask for it.
pub(crate) async fn acquire(
    connection: &mut PgConnection,
    relations: &[TableName],
    mode: LockMode,
    mut between: Option<BetweenAttempts<'_>>,
) -> Result<LockOutcome, sqlx::Error> {
    let started = Instant::now();
    let mut attempts = 0_u32;
    loop {
        attempts = attempts.saturating_add(1);
        let mut savepoint = connection.begin().await?;
        match LockAttempt::new(relations.to_vec(), mode, ATTEMPT_TIMEOUT)
            .to_query()
            .execute(&mut *savepoint)
            .await
        {
            Ok(_) => {
                savepoint.commit().await?;
                return Ok(LockOutcome::Acquired);
            }
            Err(error) if is_lock_not_available(&error) => savepoint.rollback().await?,
            Err(error) => return Err(error),
        }
        match next_lock_step(started.elapsed(), attempts) {
            LockStep::Retry(backoff) => time::sleep(backoff).await,
            LockStep::Apply => {
                if let Some(BetweenAttempts { connection, script }) = between.as_mut() {
                    log_replay::apply_batch(connection, script).await?;
                }
            }
            LockStep::Abandon => return Ok(LockOutcome::Abandoned),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::repack::lock_acquisition::{LockStep, next_lock_step};
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn timed_out_attempts_alternate_a_growing_back_off_with_a_replay() {
        let steps = (1..=4)
            .map(|attempt| next_lock_step(Duration::from_millis(u64::from(attempt) * 300), attempt))
            .collect::<Vec<_>>();
        assert_eq!(
            steps,
            [
                LockStep::Retry(Duration::from_millis(100)),
                LockStep::Apply,
                LockStep::Retry(Duration::from_millis(300)),
                LockStep::Apply,
            ]
        );
    }

    #[test]
    fn the_back_off_stops_growing_at_one_second() {
        assert_eq!(next_lock_step(Duration::from_secs(5), 15), LockStep::Retry(Duration::from_secs(1)));
    }

    #[test]
    fn the_run_gives_up_once_the_wait_budget_is_spent() {
        assert_eq!(next_lock_step(Duration::from_secs(61), 3), LockStep::Abandon);
        assert_eq!(next_lock_step(Duration::from_secs(60), 2), LockStep::Abandon);
        assert_eq!(next_lock_step(Duration::from_secs(59), 2), LockStep::Apply);
    }
}
