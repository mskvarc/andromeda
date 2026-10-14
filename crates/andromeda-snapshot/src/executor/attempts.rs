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
    executor::{execution_failure::ExecutionFailure, lease_deadline, status::failure_detail},
    store::{SnapshotPreparationJob, SnapshotStore},
};
use andromeda_context_commit::{lease_holder::LeaseHolder, write_retry::backoff_after_lost_attempts};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::snapshot::{ExecutionResultDetails, SnapshotStatus};
use futures::future::BoxFuture;
use std::{iter::repeat_with, time::Duration};
use tokio::time::sleep;
use tracing::warn;

/// Attempts one claimed preparation is given before it records `failure`.
///
/// Each attempt re-runs the whole capture, so the budget is small: a lost race
/// clears on the next attempt once the winner commits, and the jittered backoff
/// between attempts separates contenders rather than waiting anything out.
pub(crate) const PREPARATION_ATTEMPTS: u32 = 5;

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
        let mut lost_attempts = 0;
        loop {
            let Err(error) = attempt().await else {
                return Ok(PreparationEnd::Completed);
            };
            lost_attempts += 1;
            match error.recurrence() {
                Recurrence::Transient if lost_attempts < PREPARATION_ATTEMPTS => {
                    warn!(snapshot_id = %self.id, attempt = lost_attempts, %error, "Snapshot preparation attempt failed transiently; restarting it");
                    sleep(backoff_after_lost_attempts(lost_attempts)).await;
                    if !self.store.restart_preparation(self.id, self.worker, lease_deadline(self.lease)?).await? {
                        return Ok(PreparationEnd::ClaimLost);
                    }
                }
                Recurrence::Transient | Recurrence::Persistent => return self.record_failure(&error).await,
            }
        }
    }

    /// Move the Snapshot to `failure`, reporting `error` for every query.
    async fn record_failure(&self, error: &SnapshotError) -> Result<PreparationEnd, SnapshotError> {
        warn!(snapshot_id = %self.id, %error, "Snapshot preparation failed; recording its failure status");
        let detail = ExecutionFailure::Preparation { cause: error }.to_string();
        let entity = failure_details(self.queries.entity, &detail);
        let temporal = failure_details(self.queries.temporal, &detail);
        let recorded = self
            .store
            .update_status(self.id, self.worker, SnapshotStatus::Failure, entity.as_deref(), temporal.as_deref())
            .await?;
        Ok(if recorded == 0 { PreparationEnd::ClaimLost } else { PreparationEnd::Failed })
    }
}

/// One failed execution result per query, or none when there are no queries.
fn failure_details(count: usize, detail: &str) -> Option<Vec<ExecutionResultDetails>> {
    (count > 0).then(|| repeat_with(|| failure_detail(detail)).take(count).collect())
}

#[cfg(test)]
mod tests;
