//! The durable side-effect queue port.
//!
//! Clause 5.6.7 lets a batch operation answer before every consequence of the
//! write has been produced, which is what `Prefer: respond-async` opts into.
//! The consequences still have to survive a crash, so they are written inside
//! the same atomic write as the entities and drained afterwards.
//!
//! Leasing rather than reading is what makes the drain at-least-once: a lease
//! that expires unacknowledged becomes claimable again.

use crate::{error::AndromedaError, service::entity::BatchSideEffectJob};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use uuid::Uuid;

/// One side-effect job leased for execution.
#[derive(Debug)]
pub struct LeasedSideEffect {
    /// Queue entry identifier, used to acknowledge or reschedule the job.
    pub id: Uuid,
    /// How many times this job has already been attempted.
    pub attempts: i16,
    /// The work to perform.
    pub job: BatchSideEffectJob,
}

/// Draining the durable side-effect queue.
pub trait SideEffectQueue: Send + Sync + 'static {
    /// Lease up to `batch_size` due jobs for `lease_seconds`.
    ///
    /// A job whose stored payload cannot be read back is dead-lettered by the
    /// implementation rather than returned, so one unreadable entry cannot
    /// stall the queue behind it.
    fn lease_batch<'life0, 'life1, 'async_trait>(
        &'life0 self,
        worker_id: &'life1 str,
        batch_size: i32,
        lease_seconds: i32,
    ) -> BoxFuture<'async_trait, Result<Vec<LeasedSideEffect>, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Acknowledge a completed job, removing it. Returns whether it existed.
    fn ack<'life0, 'async_trait>(&'life0 self, id: Uuid) -> BoxFuture<'async_trait, Result<bool, AndromedaError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;

    /// Release a failed job for another attempt at `next_attempt_at`.
    fn nack<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: Uuid,
        error: &'life1 str,
        next_attempt_at: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<bool, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Dead-letter a job that has exhausted its attempts. Returns whether it existed.
    fn mark_dead<'life0, 'life1, 'async_trait>(&'life0 self, id: Uuid, error: &'life1 str) -> BoxFuture<'async_trait, Result<bool, AndromedaError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
}
