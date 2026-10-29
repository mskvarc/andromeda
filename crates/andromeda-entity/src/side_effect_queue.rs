//! The durable side-effect queue port.
//!
//! Andromeda lets a batch operation answer before every consequence of the
//! write has been produced, which is what `Prefer: respond-async` opts into.
//! The consequences still have to survive a crash, so they are written inside
//! the same atomic write as the entities and drained afterwards.
//!
//! Leasing rather than reading is what makes the drain at-least-once: a lease
//! that expires before its job was settled by a replay or released for a retry
//! becomes claimable again.

use crate::error::EntityError;
use andromeda_context_commit::{
    batch_side_effect_job::BatchSideEffectJob,
    lease_acknowledgement::LeaseAcknowledgement,
    lease_holder::LeaseHolder,
    side_effect_job_id::SideEffectJobId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use std::num::NonZeroU32;

/// One side-effect job leased for execution.
#[derive(Debug)]
pub struct LeasedSideEffect {
    /// Queue entry identifier, used to settle or reschedule the job.
    pub id: SideEffectJobId,
    /// How many times this job has already been attempted.
    ///
    /// Counts every attempt that ended without settling the job: one that
    /// failed and was released for a retry, and one that ended without a word
    /// (a crash, or a lease that lapsed before the replay could lock the job),
    /// which the claim that reclaimed the job counted.
    ///
    /// Decoded from `pending_side_effects.attempt_count`, an `INTEGER` column, so this is `i32` to match its SQL width.
    pub attempts: i32,
    /// The work to perform.
    pub job: BatchSideEffectJob,
    /// The identity and ingestion instant the job's enqueueing commit shares
    /// with its other consequences.
    ///
    /// A worker delivering this job replays under `record` rather than minting
    /// a fresh identity, so the deferred consequences belong to the write that
    /// caused them.
    pub record: WriteRecord,
    /// First ordinal free in the enqueueing commit's consequence numbering.
    ///
    /// A worker delivering this job resumes the commit's running ordinal
    /// counter at this value rather than restarting it at zero, so the deferred
    /// consequences are numbered after the ones the commit already wrote.
    pub base_ordinal: WriteOrdinal,
}

/// Draining the durable side-effect queue.
pub trait SideEffectQueue: Send + Sync + 'static {
    /// Lease up to `batch_size` due jobs for `lease_seconds`.
    ///
    /// Reclaiming a job whose previous lease lapsed counts the attempt that
    /// lease belonged to, since nothing else recorded it.
    ///
    /// A job whose stored payload cannot be read back is dead-lettered by the
    /// implementation rather than returned, since no replay can ever apply
    /// it, so one unreadable entry cannot stall the queue behind it; the
    /// implementation claims again in its place, so a result shorter than
    /// `batch_size` means the queue held no more due jobs. That is the only
    /// way a job leaves the queue other than its own replay: a job that fails
    /// is released for a retry however often it failed.
    fn lease_batch<'life0, 'life1, 'async_trait>(
        &'life0 self,
        worker_id: &'life1 LeaseHolder,
        batch_size: NonZeroU32,
        lease_seconds: i32,
    ) -> BoxFuture<'async_trait, Result<Vec<LeasedSideEffect>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Release a failed job for another attempt at `next_attempt_at`, counting
    /// the attempt.
    ///
    /// Guarded by `owner` still being the job's claimant, not by its lease
    /// still being live. A replay may outlive its lease, its settlement locks
    /// the job for as long as it runs, and one that fails afterwards still
    /// owes the job its diagnostic and its backoff. Every claim rewrites the
    /// claimant, so a job another worker reclaimed in the meantime is no longer
    /// `owner`'s and the release reports [`LeaseAcknowledgement::Lost`] rather
    /// than overwriting that worker's claim; `owner` itself reclaims nothing
    /// until this release is done, because a drain holds a claimant identity of
    /// its own and runs one pass at a time. A job that succeeded is not
    /// released here: its replay commit removes it, under a live lease, as the
    /// first statement of its own transaction.
    fn nack<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: SideEffectJobId,
        owner: &'life1 LeaseHolder,
        error: &'life2 str,
        next_attempt_at: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<LeaseAcknowledgement, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Remove jobs dead-lettered before `cutoff`, returning how many were
    /// removed.
    ///
    /// A dead job, one whose stored payload could not be read back, is kept
    /// as evidence of a batch whose deferred side effects never landed; this
    /// is the retention sweep that bounds how long, counted from the instant
    /// the job was dead-lettered rather than from when it was enqueued. It
    /// touches no live row and holds no lease.
    fn purge_dead_before<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
