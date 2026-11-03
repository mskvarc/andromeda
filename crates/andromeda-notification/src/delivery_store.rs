//! The notification delivery store port.
//!
//! Draining is a lease, not a read: a claim marks the deliveries it returns
//! with the claiming worker and a lease deadline, so a second worker cannot
//! pick them up until that deadline passes. Every later update names the same
//! worker, so an implementation refuses an acknowledgement from a worker whose
//! lease has expired instead of letting it overwrite the state of the worker
//! that reclaimed the delivery.
//!
//! Claiming touches deliveries only; the event payload each claimed delivery
//! carries is joined in afterwards, so the queue scan never reads a snapshot it
//! does not end up delivering, and an event the claiming pass already holds is
//! not read at all.
//!
//! Deliveries live per context branch: the live branch and every Snapshot
//! branch keep their own delivery queue. A claim drains across branches and
//! reaping sweeps every branch, so neither names one; but a settlement
//! addresses one delivery, whose identifier is unique only within its branch, so
//! each [`DeliverySettlement`] names the branch its row lives in.
//!
//! The worker goes to the queue in rounds: `settle_and_claim` records a group
//! of settlements and claims the slots they free in one transaction. The group
//! either lands whole or not at all, and a round asked again after a failure
//! records no delivery twice and claims nothing twice.
//!
//! An endpoint that fails holds its Subscription's queue: a retryable failure
//! makes the failed delivery the Subscription's probe, the one delivery of the
//! Subscription a claim still returns, and sets the others aside with their
//! attempt counts unchanged. A delivered attempt releases them in the order
//! they were queued, and a probe that is exhausted hands the hold to the
//! delivery set aside first. However long the backlog behind an endpoint that
//! never answers, it costs one attempt per retry step, and the deliveries of
//! every other Subscription keep the slots.

use crate::{
    delivery_round::{ClaimRequest, SettledRound},
    delivery_settlement::DeliverySettlement,
    error::NotificationError,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;

/// Leasing, completing and reaping notification deliveries.
pub trait DeliveryStore: Send + Sync + 'static {
    /// Record every settlement of `settlements` that `worker_id` still holds
    /// the lease of, then claim what `refill` asks for, in one transaction.
    ///
    /// Each settlement moves the hold of its Subscription with it: a
    /// delivered settlement releases its Subscription's hold, a failed one
    /// takes it and sets the rest of the Subscription's queue aside, and an
    /// exhausted probe hands the hold to the delivery set aside first. The
    /// settlements of one group were in flight together, so no order between
    /// them is owed: the queue may record them in any order that gives each
    /// the effect it would have alone. The claim comes after them and sees
    /// the holds as they left them.
    ///
    /// A failure diagnostic arrives already truncated to the bound the schema
    /// accepts, so an implementation stores it verbatim rather than trimming
    /// it again. The attempt ceiling is not the store's concern: the worker
    /// settles the final failed attempt as exhausted, which is what takes a
    /// delivery out of the claimable set, so a claim need not know the
    /// ceiling either.
    ///
    /// The claim takes up to `refill.width` deliveries that are still queued,
    /// neither delivered nor exhausted, and claimable, meaning due and either
    /// unleased or held under an expired lease; it leases each to `worker_id`
    /// for `refill.lease` and returns it joined to its event. A delivery
    /// whose stored row cannot be read back is returned as
    /// [`DeliveryClaim::Unreadable`](crate::delivery_claim::DeliveryClaim::Unreadable),
    /// still leased, rather than failing the claim or being left out of it:
    /// one unreadable row must not stall every other pending notification,
    /// and the worker must be able to acknowledge it so it is not claimed
    /// again at every lease expiry.
    ///
    /// A round that asks for no claim acknowledges only, and one with no
    /// settlements claims only.
    ///
    /// # Errors
    ///
    /// [`NotificationError::Persistence`] when the settlements could not be
    /// recorded; none of them is then, and nothing is claimed. A claim that
    /// fails once the settlements are recorded is answered in
    /// [`SettledRound::claims`], with the settlements kept.
    fn settle_and_claim<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        worker_id: &'life1 LeaseHolder,
        settlements: &'life2 [DeliverySettlement],
        refill: Option<ClaimRequest<'life3>>,
    ) -> BoxFuture<'async_trait, Result<SettledRound, NotificationError>>
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
