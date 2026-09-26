//! Re-running a commit PostgreSQL rolled back to resolve a conflict.
//!
//! PostgreSQL breaks a deadlock (SQLSTATE 40P01) or a serialization conflict
//! (40001) by rolling one contender's whole transaction back. For a commit that
//! is not a fault of the write: nothing of the attempt survives -- dictionary
//! keys it minted are withheld from the name cache until a commit publishes
//! them, every consequence it wrote is a row of the same transaction, and the
//! drains are woken only after a commit -- so the identical write can simply
//! run again. Clause 5.6.1.4 leaves a create exactly one failure,
//! `AlreadyExists`; a conflict with an unrelated transaction must not become a
//! second one, so the committer absorbs it rather than reporting it.

use andromeda_context_commit::{error::CommitError, write_retry::backoff_after_lost_attempts};
use std::future::Future;
use tokio::time::sleep;

/// How many times a commit is run before its contention is reported.
///
/// A victim clears on the next attempt unless the same cycle forms again, so a
/// handful of attempts absorbs every conflict that is not pathological, and the
/// bound caps what a pathological one adds to the request before the client is
/// told to retry (503 with `Retry-After`).
pub(crate) const CONTENDED_COMMIT_ATTEMPTS: u32 = 5;

/// Run `attempt` until it ends in anything but
/// [`CommitError::TransactionContended`], at most
/// [`CONTENDED_COMMIT_ATTEMPTS`] times, with a jittered backoff between runs so
/// the contenders that collided do not restart in lockstep.
pub(crate) async fn rerun_while_contended<T, F, Attempt>(mut attempt: F) -> Result<T, CommitError>
where
    F: FnMut() -> Attempt,
    Attempt: Future<Output = Result<T, CommitError>>,
{
    let mut lost_attempts = 0;
    loop {
        match attempt().await {
            Err(CommitError::TransactionContended) if lost_attempts + 1 < CONTENDED_COMMIT_ATTEMPTS => {
                lost_attempts += 1;
                tracing::debug!(lost_attempts, "atomic context commit rolled back by a concurrent transaction; running it again");
                sleep(backoff_after_lost_attempts(lost_attempts)).await;
            }
            outcome => return outcome,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::contention_retry::{CONTENDED_COMMIT_ATTEMPTS, rerun_while_contended};
    use andromeda_context_commit::error::CommitError;
    use std::{
        cell::Cell,
        future::{Ready, ready},
    };

    /// An attempt that reports contention on its first `contended` runs and
    /// `settled` afterwards, counting every run.
    fn attempt_contended_for(
        contended: u32,
        runs: &Cell<u32>,
        settled: fn() -> Result<u32, CommitError>,
    ) -> impl FnMut() -> Ready<Result<u32, CommitError>> + '_ {
        move || {
            runs.set(runs.get() + 1);
            ready(if runs.get() <= contended {
                Err(CommitError::TransactionContended)
            } else {
                settled()
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_commit_rolled_back_twice_lands_on_its_third_run() {
        let runs = Cell::new(0);
        let outcome = rerun_while_contended(attempt_contended_for(2, &runs, || Ok(7))).await;
        assert!(matches!(outcome, Ok(7)), "{outcome:?}");
        pretty_assertions::assert_eq!(runs.get(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_commit_rolled_back_on_every_run_reports_contention_after_the_budget() {
        let runs = Cell::new(0);
        let outcome = rerun_while_contended(attempt_contended_for(u32::MAX, &runs, || Ok(7))).await;
        assert!(matches!(outcome, Err(CommitError::TransactionContended)), "{outcome:?}");
        pretty_assertions::assert_eq!(runs.get(), CONTENDED_COMMIT_ATTEMPTS);
    }

    /// Only a rolled-back transaction is known to have left nothing behind; any
    /// other failure is reported from the run that raised it.
    #[tokio::test(start_paused = true)]
    async fn a_failure_other_than_contention_is_never_run_again() {
        let runs = Cell::new(0);
        let outcome = rerun_while_contended(attempt_contended_for(0, &runs, || {
            Err(CommitError::Persistence {
                detail: "connection reset".to_string(),
            })
        }))
        .await;
        assert!(matches!(outcome, Err(CommitError::Persistence { .. })), "{outcome:?}");
        pretty_assertions::assert_eq!(runs.get(), 1);
        let saturated = rerun_while_contended(|| ready(Err::<u32, CommitError>(CommitError::StoreSaturated))).await;
        assert!(matches!(saturated, Err(CommitError::StoreSaturated)), "{saturated:?}");
    }
}
