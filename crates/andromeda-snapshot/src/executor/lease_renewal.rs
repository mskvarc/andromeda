//! Keeping a claimed preparation's lease alive while its execution runs.
//!
//! A preparation holds its claim through a lease it renews every half lease.
//! The renewal is an UPDATE of the claimed preparation row, and the execution
//! itself may hold a lock on that row inside an open transaction: a capture
//! while it publishes, a clone while it copies. The renewal therefore has to be
//! polled beside the execution, never in its place.

use crate::{
    error::SnapshotError,
    executor::{attempts::PreparationEnd, lease_deadline},
    store::SnapshotStore,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::{future::Future, time::Duration};
use tokio::time::{Instant, MissedTickBehavior, interval_at};

/// The claim's lease ran out of this worker's hands: another worker took the
/// claim over, or the Snapshot left `preparing` or was deleted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LeaseLost;

/// The renewal of one preparation claim, driven side by side with its execution.
pub(crate) struct LeaseRenewal<'a> {
    /// The store the claim is held in.
    pub(crate) store: &'a dyn SnapshotStore,
    /// The Snapshot being prepared.
    pub(crate) id: &'a Identifier,
    /// The worker holding the claim.
    pub(crate) worker: LeaseHolder,
    /// How long each renewal extends the claim for.
    pub(crate) lease: Duration,
}

impl LeaseRenewal<'_> {
    /// Drive `execution` to its end while renewing the claim every half lease.
    ///
    /// The execution and the renewal are polled concurrently. An execution can
    /// hold a row lock the renewal's UPDATE waits on; a renewal awaited in place
    /// of the execution would wait on a transaction that is never resumed. That
    /// wait cycle runs through a Rust future, so PostgreSQL's deadlock detector
    /// cannot see it, and only the server's idle-in-transaction timeout would
    /// end it. When both are ready together the execution's result wins.
    ///
    /// # Errors
    ///
    /// Returns the execution's own error, the store's failure when a renewal
    /// does not complete, and [`SnapshotError::Internal`] when the lease cannot
    /// be turned into a wall-clock deadline. A claim found lost by a renewal is
    /// [`PreparationEnd::ClaimLost`], not an error.
    pub(crate) async fn drive(&self, execution: impl Future<Output = Result<PreparationEnd, SnapshotError>>) -> Result<PreparationEnd, SnapshotError> {
        let renewal = self.hold();
        tokio::pin!(execution, renewal);
        tokio::select! {
            biased;
            end = &mut execution => end,
            lost = &mut renewal => lost.map(|LeaseLost| PreparationEnd::ClaimLost),
        }
    }

    /// Renew the claim every half lease until a renewal finds it lost or fails.
    async fn hold(&self) -> Result<LeaseLost, SnapshotError> {
        let period = self.lease / 2;
        let mut renewals = interval_at(Instant::now() + period, period);
        renewals.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            renewals.tick().await;
            if !self.store.renew_preparation_lease(self.id, self.worker, lease_deadline(self.lease)?).await? {
                return Ok(LeaseLost);
            }
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
