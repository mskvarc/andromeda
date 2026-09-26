//! Draining every claimable preparation in rounds.
//!
//! A clone copies all data of the Snapshot it clones (clause 5.16.2.4), so the
//! store does not hand out a clone job while its source is still preparing. The
//! pass that ends the source is what makes its clones claimable, so a pass does
//! not stop after one round: it claims again as long as its last round moved a
//! preparation on, and a clone waiting on a source this pass ended is claimed
//! in the round after. A clone created after that round was claimed is
//! prepared by the pass its own creation kicks, which already sees the source
//! ended.

use crate::{
    error::SnapshotError,
    executor::{attempts::PreparationEnd, lease_deadline},
    store::{SnapshotPreparationJob, SnapshotStore},
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use futures::{StreamExt, future::BoxFuture, stream};
use std::time::Duration;
use tracing::warn;

/// The worker a drain claims preparations for, and how many it runs at once.
pub(crate) struct DrainWorker<'a> {
    /// The store the preparations are claimed from.
    pub(crate) store: &'a dyn SnapshotStore,
    /// The worker the claims are held by.
    pub(crate) worker: LeaseHolder,
    /// How long a claim is leased for.
    pub(crate) lease: Duration,
    /// How many preparations one round claims and runs concurrently.
    pub(crate) max_per_round: usize,
}

impl DrainWorker<'_> {
    /// Claim and run preparations in rounds until a round claims nothing or
    /// ends none of its jobs, returning how many preparations this worker moved
    /// out of `preparing`.
    ///
    /// A round in which every job gave its claim back after a store failure
    /// ends the drain: claiming again would only hand the same jobs back
    /// against the same failing store. Every other round moved at least one
    /// preparation beyond this worker's reach, so the drain terminates.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Internal`] when the configured lease cannot be
    /// turned into a wall-clock deadline, and the store's own failure when a
    /// claim fails before any preparation was moved on. A claim failing after
    /// that ends the drain with what it moved, so its callers still wake the
    /// status delivery those preparations enqueued.
    pub(crate) async fn drain<'f>(
        &self,
        prepare: impl Fn(SnapshotPreparationJob) -> BoxFuture<'f, Option<PreparationEnd>> + Sync,
    ) -> Result<usize, SnapshotError> {
        let mut moved = 0;
        loop {
            let jobs = match self.claim_round().await {
                Ok(jobs) => jobs,
                Err(error) if moved > 0 => {
                    warn!(%error, moved, "Claiming further Snapshot preparations failed; ending the pass with what it prepared");
                    return Ok(moved);
                }
                Err(error) => return Err(error),
            };
            if jobs.is_empty() {
                return Ok(moved);
            }
            let ends: Vec<Option<PreparationEnd>> = stream::iter(jobs).map(&prepare).buffer_unordered(self.max_per_round).collect().await;
            moved += ends.iter().flatten().filter(|end| end.left_preparing()).count();
            if ends.iter().all(Option::is_none) {
                return Ok(moved);
            }
        }
    }

    /// Claim up to one round of preparations.
    async fn claim_round(&self) -> Result<Vec<SnapshotPreparationJob>, SnapshotError> {
        let mut jobs = Vec::with_capacity(self.max_per_round);
        while jobs.len() < self.max_per_round {
            let Some(job) = self.store.next_preparing(self.worker, lease_deadline(self.lease)?).await? else {
                break;
            };
            jobs.push(job);
        }
        Ok(jobs)
    }
}

#[cfg(test)]
mod tests;
