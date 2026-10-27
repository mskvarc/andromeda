//! The notification delivery store port.
//!
//! Draining is a lease, not a read: `claim_pending` marks the deliveries it
//! returns with the claiming worker and a lease deadline, so a second worker
//! cannot pick them up until that deadline passes. Every later update names the
//! same worker, so an implementation refuses an acknowledgement from a worker
//! whose lease has expired instead of letting it overwrite the state of the
//! worker that reclaimed the delivery.
//!
//! Claiming touches deliveries only; the event payload each claimed delivery
//! carries is joined in afterwards, so the queue scan never reads a snapshot it
//! does not end up delivering, and an event the claiming pass already holds is
//! not read at all.
//!
//! Deliveries live per context branch: the live branch and every Snapshot
//! branch keep their own delivery queue. `claim_pending` drains across branches
//! and reaping sweeps every branch, so neither names one; but an acknowledgement
//! addresses one delivery, whose identifier is unique only within its branch, so
//! `mark_delivered`/`mark_failed`/`mark_terminal` take the delivery's
//! [`ContextTarget`] to name the branch its row lives in.

use crate::{
    delivery_claim::DeliveryClaim,
    delivery_entry::DeliveryKey,
    delivery_lease::{DeliveryLeaseError, LeaseTerm},
    error::NotificationError,
    held_events::HeldEvents,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::context_target::ContextTarget;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use std::num::NonZeroU32;

/// Leasing, completing and reaping notification deliveries.
pub trait DeliveryStore: Send + Sync + 'static {
    /// Claim up to `batch_size` deliveries that are still queued — neither
    /// delivered nor exhausted — and claimable, meaning due and either unleased
    /// or held under an expired lease; lease each claimed delivery to
    /// `worker_id` for `lease`, and return it joined to its event.
    ///
    /// An event `held` already holds for the delivery's branch is handed over
    /// as it is rather than read again, and every event the claim does read is
    /// added to `held`, so the deliveries of one event share one payload across
    /// the claims of a pass.
    ///
    /// The attempt ceiling is not the store's concern: the worker acknowledges
    /// the final failed attempt through [`DeliveryStore::mark_terminal`], which
    /// is what takes a delivery out of the claimable set.
    ///
    /// A delivery whose stored row cannot be read back is returned as
    /// [`DeliveryClaim::Unreadable`], still leased, rather than failing the
    /// claim or being left out of it: one unreadable row must not stall every
    /// other pending notification, and the worker must be able to acknowledge
    /// it so it is not claimed again at every lease expiry.
    ///
    /// # Errors
    ///
    /// An `Err` means the queue itself could not be claimed from -- the
    /// database unreachable, a statement refused -- and ends the worker's pass.
    fn claim_pending<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        worker_id: &'life1 LeaseHolder,
        lease: LeaseTerm,
        batch_size: NonZeroU32,
        held: &'life2 HeldEvents,
    ) -> BoxFuture<'async_trait, Result<Vec<DeliveryClaim>, NotificationError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Mark a delivery delivered, provided `worker_id` still holds its lease.
    ///
    /// `target` names the branch the delivery's row lives in, since its
    /// identifier is unique only within that branch.
    fn mark_delivered<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        key: DeliveryKey,
        worker_id: &'life1 LeaseHolder,
        target: &'life2 ContextTarget,
    ) -> BoxFuture<'async_trait, Result<(), DeliveryLeaseError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Record a failed attempt, release the lease and schedule the next attempt,
    /// provided `worker_id` still holds that lease.
    ///
    /// `error` arrives already truncated to the bound the schema accepts, so an
    /// implementation stores it verbatim rather than trimming it again.
    fn mark_failed<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        key: DeliveryKey,
        worker_id: &'life1 LeaseHolder,
        error: &'life2 str,
        next_attempt_at: DateTime<Utc>,
        target: &'life3 ContextTarget,
    ) -> BoxFuture<'async_trait, Result<(), DeliveryLeaseError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Record a terminal failure — a failure no retry can fix, or the failure
    /// of the attempt that reached the configured ceiling — stamp the delivery
    /// exhausted, release the lease and make it permanently ineligible for
    /// claims, provided `worker_id` still holds that lease.
    ///
    /// `error` arrives already truncated to the bound the schema accepts.
    fn mark_terminal<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        key: DeliveryKey,
        worker_id: &'life1 LeaseHolder,
        error: &'life2 str,
        target: &'life3 ContextTarget,
    ) -> BoxFuture<'async_trait, Result<(), DeliveryLeaseError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;

    /// Reap deliveries acknowledged before `cutoff`, then the events no
    /// surviving delivery still references. Returns the count of deliveries
    /// removed.
    fn delete_delivered_before<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, NotificationError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Reap deliveries stamped exhausted before `cutoff`. Returns the count of
    /// deliveries removed.
    fn delete_exhausted_before<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, NotificationError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Reap events created before `cutoff` that no delivery references,
    /// whether that delivery was never inserted or was removed by any other
    /// path. Returns the count of events removed.
    fn delete_orphan_events_before<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, NotificationError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
