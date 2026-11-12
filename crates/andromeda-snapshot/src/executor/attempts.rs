//! Bounded re-attempts of one claimed Snapshot preparation.
//!
//! Clause 5.16.1.4 ends every preparation in `success`, `partial`, `empty` or
//! `failure` (Table 5.2.41-2). A preparation that loses a deadlock or
//! serialization race, finds the pool saturated or loses its connection has not
//! failed: it restarts under the same claim from an empty staging branch,
//! promptly and a bounded number of times, so the Snapshot never waits out its
//! claim's lease. Only a failure that outlasts the budget, or one a fresh
//! attempt would repeat, records `failure`.

use crate::{
    error::{Recurrence, SnapshotError},
    executor::{execution_failure::ExecutionFailure, status::failure_detail},
    store::{SnapshotPreparationJob, SnapshotStore},
};
use andromeda_context_commit::{lease_holder::LeaseHolder, write_retry::WRITE_BACKOFF};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, SnapshotStatus};
use andromeda_ngsi_ld_problem_details::problem_detail::ProblemDetail;
use andromeda_retry::{
    retry_policy::{RetryAttempt, RetryPolicy},
    tokio_retry_delay::TokioRetryDelay,
};
use futures::future::BoxFuture;
use std::{iter::repeat_with, num::NonZeroU32, time::Duration};
use tracing::warn;

/// Attempts one claimed preparation is given before it records `failure`.
///
/// Each attempt re-runs the whole capture, so the budget is small: a lost race
/// clears on the next attempt once the winner commits, and the jittered backoff
/// between attempts separates contenders rather than waiting anything out.
pub(crate) const PREPARATION_ATTEMPTS: u32 = 5;

/// The budget and the jittered wait between attempts of one preparation.
const PREPARATION_RETRY: RetryPolicy = RetryPolicy {
    attempts: NonZeroU32::MIN.saturating_add(PREPARATION_ATTEMPTS - 1),
    schedule: WRITE_BACKOFF,
};

/// Why one run of a claimed preparation did not complete.
enum PreparationFailure {
    /// The attempt itself failed.
    Attempt(SnapshotError),
    /// Restarting the claim before the attempt did not complete.
    Restart(SnapshotError),
    /// The claim was no longer this worker's when it was restarted.
    ClaimLost,
}

/// How one claimed preparation ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PreparationEnd {
    /// An attempt left `preparing` on its own terms.
    Completed,
    /// No attempt could, so the Snapshot was moved to `failure`.
    Failed,
    /// The claim was no longer this worker's: another worker took it over, or
    /// the Snapshot was deleted or ended under it. Its outcome is not this
    /// worker's to record.
    ClaimLost,
}

impl PreparationEnd {
    /// Whether this worker moved the Snapshot out of `preparing`.
    pub(crate) const fn left_preparing(self) -> bool {
        match self {
            PreparationEnd::Completed | PreparationEnd::Failed => true,
            PreparationEnd::ClaimLost => false,
        }
    }
}

/// How many queries a preparation executes; a recorded `failure` reports one
/// execution result per query, in query order (Table 5.2.41-2).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct QueryCounts {
    /// Entries of `snapshotQueries`.
    entity: usize,
    /// Entries of `snapshotTemporalQueries`.
    temporal: usize,
}

impl QueryCounts {
    /// The queries a claimed job executes: none for a clone, which copies its
    /// source (clause 5.16.2.4), and its own for a capture.
    pub(crate) fn of(job: &SnapshotPreparationJob) -> QueryCounts {
        if job.clone_source_id.is_some() {
            return QueryCounts::default();
        }
        QueryCounts {
            entity: job.snapshot.snapshot_queries.as_ref().map_or(0, Vec::len),
            temporal: job.snapshot.snapshot_temporal_queries.as_ref().map_or(0, Vec::len),
        }
    }
}

/// One preparation claimed by this worker.
pub(crate) struct PreparationClaim<'a> {
    /// The store the claim is held in.
    pub(crate) store: &'a dyn SnapshotStore,
    /// The Snapshot being prepared.
    pub(crate) id: &'a Identifier,
    /// The worker holding the claim.
    pub(crate) worker: LeaseHolder,
    /// How long a restarted claim is leased for.
    pub(crate) lease: Duration,
    /// The queries a recorded `failure` reports on.
    pub(crate) queries: QueryCounts,
}

impl PreparationClaim<'_> {
    /// Run `attempt` until one leaves `preparing`, restarting the claim after
    /// each transient failure and recording `failure` once the budget is spent
    /// or a failure would recur.
    ///
    /// # Errors
    ///
    /// Returns the store's own failure when restarting the claim or recording
    /// `failure` does not complete. A claim lost before a restart or before
    /// `failure` could be recorded is [`PreparationEnd::ClaimLost`], not an
    /// error.
    pub(crate) async fn run<'f>(&self, mut attempt: impl FnMut() -> BoxFuture<'f, Result<(), SnapshotError>>) -> Result<PreparationEnd, SnapshotError> {
        let outcome = PREPARATION_RETRY
            .run(
                &TokioRetryDelay,
                |failure, retry| match failure {
                    PreparationFailure::Attempt(error) => match error.recurrence() {
                        Recurrence::Transient => {
                            warn!(snapshot_id = %self.id, attempt = retry.get(), %error, "Snapshot preparation attempt failed transiently; restarting it");
                            true
                        }
                        Recurrence::Persistent => false,
                    },
                    PreparationFailure::Restart(_) | PreparationFailure::ClaimLost => false,
                },
                async |run| {
                    match run {
                        RetryAttempt::First => (),
                        RetryAttempt::Retry(_) => self.restart().await?,
                    }
                    attempt().await.map_err(PreparationFailure::Attempt)
                },
            )
            .await;
        match outcome {
            Ok(()) => Ok(PreparationEnd::Completed),
            Err(PreparationFailure::Attempt(error)) => self.record_failure(&error).await,
            Err(PreparationFailure::Restart(error)) => Err(error),
            Err(PreparationFailure::ClaimLost) => Ok(PreparationEnd::ClaimLost),
        }
    }

    /// Restart the claim from an empty staging branch before another attempt.
    async fn restart(&self) -> Result<(), PreparationFailure> {
        match restart_staging(self.store, self.id, self.worker, self.lease).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(PreparationFailure::ClaimLost),
            Err(error) => Err(PreparationFailure::Restart(error)),
        }
    }

    /// Move the Snapshot to `failure`, reporting `error` for every query.
    async fn record_failure(&self, error: &SnapshotError) -> Result<PreparationEnd, SnapshotError> {
        warn!(snapshot_id = %self.id, %error, "Snapshot preparation failed; recording its failure status");
        let problem = ExecutionFailure::Preparation { cause: error }.problem();
        let entity = failure_details(self.queries.entity, &problem);
        let temporal = failure_details(self.queries.temporal, &problem);
        let recorded = self
            .store
            .update_status(self.id, self.worker, SnapshotStatus::Failure, entity.as_deref(), temporal.as_deref())
            .await?;
        Ok(if recorded == 0 { PreparationEnd::ClaimLost } else { PreparationEnd::Failed })
    }
}

/// Empty the staging branch of the preparation `id` still claimed by `worker`,
/// leasing it for `lease` from the restart; false when the claim was lost or
/// the Snapshot is no longer preparing.
///
/// # Errors
///
/// Returns the store's own failure when the restart does not complete.
pub(super) async fn restart_staging(store: &dyn SnapshotStore, id: &Identifier, worker: LeaseHolder, lease: Duration) -> Result<bool, SnapshotError> {
    store.restart_preparation(id, worker, lease).await
}

/// One failed execution result per query, or none when there are no queries.
fn failure_details(count: usize, problem: &ProblemDetail) -> Option<Vec<ExecutionResultDetails>> {
    (count > 0).then(|| repeat_with(|| failure_detail(problem.clone())).take(count).collect())
}

#[cfg(test)]
mod tests;
