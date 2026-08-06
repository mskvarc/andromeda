//! The durable side-effect queue port.
//!
//! Clause 5.6.7 lets a batch operation answer before every consequence of the
//! write has been produced, which is what `Prefer: respond-async` opts into.
//! The consequences still have to survive a crash, so they are written inside
//! the same atomic write as the entities and drained afterwards.
//!
//! Leasing rather than reading is what makes the drain at-least-once: a lease
//! that expires unacknowledged becomes claimable again.

use crate::{core::error::AndromedaError, service::entity::BatchSideEffectJob};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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
#[async_trait]
pub trait SideEffectQueue: Send + Sync + 'static {
    /// Lease up to `batch_size` due jobs for `lease_seconds`.
    ///
    /// A job whose stored payload cannot be read back is dead-lettered by the
    /// implementation rather than returned, so one unreadable entry cannot
    /// stall the queue behind it.
    async fn lease_batch(&self, worker_id: &str, batch_size: i32, lease_seconds: i32) -> Result<Vec<LeasedSideEffect>, AndromedaError>;

    /// Acknowledge a completed job, removing it. Returns whether it existed.
    async fn ack(&self, id: Uuid) -> Result<bool, AndromedaError>;

    /// Release a failed job for another attempt at `next_attempt_at`.
    async fn nack(&self, id: Uuid, error: &str, next_attempt_at: DateTime<Utc>) -> Result<bool, AndromedaError>;

    /// Dead-letter a job that has exhausted its attempts. Returns whether it existed.
    async fn mark_dead(&self, id: Uuid, error: &str) -> Result<bool, AndromedaError>;
}
