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
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use futures::{StreamExt, future::BoxFuture, stream};
use std::time::Duration;
use tracing::{info, warn};

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
        self.drain_after(0, prepare).await
    }

    /// [`DrainWorker::drain`] for a pass that already moved `moved`
    /// preparations out of `preparing` before it started claiming, returning
    /// the total. A claim failing after that ends the drain with `moved`
    /// rather than an error.
    ///
    /// # Errors
    ///
    /// As [`DrainWorker::drain`], once nothing was moved.
    async fn drain_after<'f>(
        &self,
        mut moved: usize,
        prepare: impl Fn(SnapshotPreparationJob) -> BoxFuture<'f, Option<PreparationEnd>> + Sync,
    ) -> Result<usize, SnapshotError> {
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

    /// Claim and run the Snapshot `id` a request just created, then drain as
    /// [`DrainWorker::drain`] does, returning the total moved out of
    /// `preparing`.
    ///
    /// When the created Snapshot cannot be claimed (a worker holds it, or it
    /// clones a Snapshot still preparing) that is logged: its holder, the pass
    /// ending its source, or a later sweep prepares it.
    ///
    /// # Errors
    ///
    /// As [`DrainWorker::drain`], and the store's own failure when the created
    /// Snapshot's claim does not complete.
    pub(crate) async fn drain_created<'f>(
        &self,
        id: &Identifier,
        prepare: impl Fn(SnapshotPreparationJob) -> BoxFuture<'f, Option<PreparationEnd>> + Sync,
    ) -> Result<usize, SnapshotError> {
        let moved = if let Some(job) = self.store.claim_created(id, self.worker, lease_deadline(self.lease)?).await? {
            usize::from(prepare(job).await.is_some_and(PreparationEnd::left_preparing))
        } else {
            info!(
                snapshot_id = %id,
                "Created Snapshot is not claimable on create: a worker holds its preparation or the Snapshot it clones is still preparing"
            );
            0
        };
        self.drain_after(moved, prepare).await
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
