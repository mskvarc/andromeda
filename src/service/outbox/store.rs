//! The outbox store port.
//!
//! Draining is a lease, not a read: `claim_pending` marks the entries it
//! returns so a second worker cannot pick them up, which is what lets more than
//! one broker instance share an outbox.

use crate::{
    error::AndromedaError,
    service::outbox::entry::{OutboxEntry, OutboxEntryId, WorkerInstanceId},
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;

/// Leasing, completing and reaping notification outbox entries.
pub trait OutboxStore: Send + Sync + 'static {
    /// Claim up to `batch_size` entries that are due and below `max_attempts`.
    ///
    /// Entries whose stored shape cannot be read back are skipped by the
    /// implementation rather than failing the claim: one unreadable row must
    /// not stall every other pending notification.
    fn claim_pending<'life0, 'life1, 'async_trait>(
        &'life0 self,
        worker_id: &'life1 WorkerInstanceId,
        batch_size: i32,
        max_attempts: i32,
    ) -> BoxFuture<'async_trait, Result<Vec<OutboxEntry>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Mark an entry delivered. Returns rows affected.
    fn mark_delivered<'life0, 'async_trait>(&'life0 self, id: OutboxEntryId) -> BoxFuture<'async_trait, Result<u64, AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Record a failed attempt and schedule the next one. Returns rows affected.
    fn mark_failed<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: OutboxEntryId,
        error: &'life1 str,
        next_attempt_at: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<u64, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Reap entries delivered before `cutoff`. Returns the count removed.
    fn delete_delivered_before<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
