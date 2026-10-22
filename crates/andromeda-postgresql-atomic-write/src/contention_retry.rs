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

use crate::attempt_failure::AttemptFailure;
use andromeda_context_commit::{error::CommitError, write_retry::backoff_after_lost_attempts};
use andromeda_postgresql_pool::contention_retry::{ContentionRetry, RetryPause};
use std::{num::NonZeroU32, time::Duration};

/// The jittered backoff between runs of a contended commit, so the contenders
/// that collided do not restart in lockstep.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CommitBackoff;

impl RetryPause for CommitBackoff {
    fn after(&self, lost_attempts: NonZeroU32) -> Duration {
        tracing::debug!(lost_attempts, "atomic context commit rolled back by a concurrent transaction; running it again");
        backoff_after_lost_attempts(lost_attempts.get())
    }
}

/// The retry a contended commit runs under: five runs in all.
///
/// A victim clears on the next attempt unless the same cycle forms again, so a
/// handful of attempts absorbs every conflict that is not pathological, and the
/// bound caps what a pathological one adds to the request before the client is
/// told to retry (503 with `Retry-After`).
pub(crate) const CONTENDED_COMMIT_RETRY: ContentionRetry<CommitBackoff> = ContentionRetry {
    attempts: NonZeroU32::MIN.saturating_add(4),
    pause: CommitBackoff,
};

/// Run `attempt` until it ends in anything but
/// [`CommitError::TransactionContended`], at most as often as
/// [`CONTENDED_COMMIT_RETRY`] allows.
pub(crate) async fn rerun_while_contended<T>(attempt: impl AsyncFnMut() -> Result<T, AttemptFailure>) -> Result<T, AttemptFailure> {
    CONTENDED_COMMIT_RETRY
        .run(|failure| matches!(failure, AttemptFailure::Failed(CommitError::TransactionContended)), attempt)
        .await
}

#[cfg(test)]
mod tests {
    use crate::{
        attempt_failure::{AttemptFailure, RefusedWrite},
        contention_retry::{CONTENDED_COMMIT_RETRY, rerun_while_contended},
    };
    use andromeda_context_commit::{error::CommitError, submission_refusal::SubmissionRefusal};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use std::cell::Cell;

    /// One run of an attempt that reports contention on its first `contended`
    /// runs and `settled` afterwards, counting every run.
    fn run_contended_for(contended: u32, runs: &Cell<u32>, settled: fn() -> Result<u32, AttemptFailure>) -> Result<u32, AttemptFailure> {
        runs.set(runs.get() + 1);
        if runs.get() <= contended {
            Err(AttemptFailure::Failed(CommitError::TransactionContended))
        } else {
            settled()
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_commit_rolled_back_twice_lands_on_its_third_run() {
        let runs = Cell::new(0);
        let outcome = rerun_while_contended(async || run_contended_for(2, &runs, || Ok(7))).await;
        assert!(matches!(outcome, Ok(7)), "{outcome:?}");
        pretty_assertions::assert_eq!(runs.get(), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn a_commit_rolled_back_on_every_run_reports_contention_after_the_budget() {
        let runs = Cell::new(0);
        let outcome = rerun_while_contended(async || run_contended_for(u32::MAX, &runs, || Ok(7))).await;
        assert!(matches!(outcome, Err(AttemptFailure::Failed(CommitError::TransactionContended))), "{outcome:?}");
        pretty_assertions::assert_eq!(runs.get(), CONTENDED_COMMIT_RETRY.attempts.get());
    }

    /// Only a rolled-back transaction is known to have left nothing behind; any
    /// other failure is reported from the run that raised it.
    #[tokio::test(start_paused = true)]
    async fn a_failure_other_than_contention_is_never_run_again() {
        let runs = Cell::new(0);
        let outcome = rerun_while_contended(async || {
            run_contended_for(0, &runs, || {
                Err(AttemptFailure::Failed(CommitError::Persistence(PersistenceFailure::StatementRejected)))
            })
        })
        .await;
        assert!(matches!(outcome, Err(AttemptFailure::Failed(CommitError::Persistence(_)))), "{outcome:?}");
        pretty_assertions::assert_eq!(runs.get(), 1);
        let saturated = rerun_while_contended(async || Err::<u32, AttemptFailure>(AttemptFailure::Failed(CommitError::StoreSaturated))).await;
        assert!(matches!(saturated, Err(AttemptFailure::Failed(CommitError::StoreSaturated))), "{saturated:?}");
    }

    /// A connection lost while awaiting `COMMIT` leaves the outcome unknown, so
    /// the committer reports it rather than run the write again behind the
    /// client's back.
    #[tokio::test(start_paused = true)]
    async fn an_unreachable_store_is_not_re_run() {
        let runs = Cell::new(0);
        let outcome = rerun_while_contended(async || run_contended_for(0, &runs, || Err(AttemptFailure::Failed(CommitError::StoreUnavailable)))).await;
        assert!(matches!(outcome, Err(AttemptFailure::Failed(CommitError::StoreUnavailable))), "{outcome:?}");
        pretty_assertions::assert_eq!(runs.get(), 1);
    }

    /// A refused write is the committer's to set aside, not a conflict to run
    /// again unchanged: the identical attempt would be refused identically.
    #[tokio::test(start_paused = true)]
    async fn a_refused_write_is_handed_back_from_its_first_run() {
        let runs = Cell::new(0);
        let outcome = rerun_while_contended(async || {
            run_contended_for(0, &runs, || {
                Err(AttemptFailure::Refused(Box::new(RefusedWrite {
                    entity_id: "urn:ngsi-ld:A".parse().expect("a valid entity URI"),
                    refusal: SubmissionRefusal::RelationshipMissingObject,
                })))
            })
        })
        .await;
        assert!(matches!(outcome, Err(AttemptFailure::Refused(_))), "{outcome:?}");
        pretty_assertions::assert_eq!(runs.get(), 1);
    }

    #[test]
    fn a_contended_commit_runs_five_times() {
        pretty_assertions::assert_eq!(CONTENDED_COMMIT_RETRY.attempts.get(), 5);
    }
}
