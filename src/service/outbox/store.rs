//! The outbox store port.
//!
//! Draining is a lease, not a read: `claim_pending` marks the entries it
//! returns so a second worker cannot pick them up, which is what lets more than
//! one broker instance share an outbox.

use crate::{
    core::error::AndromedaError,
    service::outbox::entry::{OutboxEntry, OutboxEntryId, WorkerInstanceId},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};

/// Leasing, completing and reaping notification outbox entries.
#[async_trait]
pub trait OutboxStore: Send + Sync + 'static {
    /// Claim up to `batch_size` entries that are due and below `max_attempts`.
    ///
    /// Entries whose stored shape cannot be read back are skipped by the
    /// implementation rather than failing the claim: one unreadable row must
    /// not stall every other pending notification.
    async fn claim_pending(&self, worker_id: &WorkerInstanceId, batch_size: i32, max_attempts: i32) -> Result<Vec<OutboxEntry>, AndromedaError>;

    /// Mark an entry delivered. Returns rows affected.
    async fn mark_delivered(&self, id: OutboxEntryId) -> Result<u64, AndromedaError>;

    /// Record a failed attempt and schedule the next one. Returns rows affected.
    async fn mark_failed(&self, id: OutboxEntryId, error: &str, next_attempt_at: DateTime<Utc>) -> Result<u64, AndromedaError>;

    /// Reap entries delivered before `cutoff`. Returns the count removed.
    async fn delete_delivered_before(&self, cutoff: DateTime<Utc>) -> Result<u64, AndromedaError>;
}
