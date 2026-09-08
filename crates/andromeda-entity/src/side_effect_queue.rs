//! The durable side-effect queue port.
//!
//! Andromeda lets a batch operation answer before every consequence of the
//! write has been produced, which is what `Prefer: respond-async` opts into.
//! The consequences still have to survive a crash, so they are written inside
//! the same atomic write as the entities and drained afterwards.
//!
//! Leasing rather than reading is what makes the drain at-least-once: a lease
//! that expires unacknowledged becomes claimable again.

use crate::error::EntityError;
use andromeda_context_commit::{
    batch_side_effect_job::BatchSideEffectJob,
    lease_acknowledgement::LeaseAcknowledgement,
    side_effect_job_id::SideEffectJobId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use chrono::{DateTime, Utc};
use derive_more::{AsRef, Display};
use futures::future::BoxFuture;
use uuid::Uuid;

/// Stable identity recorded while a side-effect worker owns a queue lease.
#[derive(Clone, Debug, Eq, PartialEq, AsRef, Display)]
#[as_ref(str)]
pub struct SideEffectLeaseOwner(String);

impl SideEffectLeaseOwner {
    /// Construct a unique lease owner for one tenant worker process.
    #[must_use]
    pub fn for_tenant(source_id: &Uri, nonce: Uuid) -> SideEffectLeaseOwner {
        SideEffectLeaseOwner(format!("{source_id}-{nonce}"))
    }
}

/// One side-effect job leased for execution.
#[derive(Debug)]
pub struct LeasedSideEffect {
    /// Queue entry identifier, used to acknowledge or reschedule the job.
    pub id: SideEffectJobId,
    /// How many times this job has already been attempted.
    pub attempts: i16,
    /// The work to perform.
    pub job: BatchSideEffectJob,
    /// The identity and ingestion instant the job's enqueueing commit shares
    /// with its other consequences.
    ///
    /// A worker delivering this job replays under `record` rather than minting
    /// a fresh identity, so a crash-retried delivery reproduces exactly the
    /// identity its first attempt would have produced.
    pub record: WriteRecord,
    /// First ordinal free in the enqueueing commit's consequence numbering.
    ///
    /// A worker delivering this job resumes the commit's running ordinal
    /// counter at this value rather than restarting it at zero, so a
    /// crash-retried delivery reproduces exactly the ordinals its first
    /// attempt would have produced.
    pub consequence_base: WriteOrdinal,
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
        worker_id: &'life1 SideEffectLeaseOwner,
        batch_size: i32,
        lease_seconds: i32,
    ) -> BoxFuture<'async_trait, Result<Vec<LeasedSideEffect>, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Acknowledge a completed job, removing it.
    ///
    /// Guarded by `owner` holding a still-live lease: an acknowledgement whose
    /// lease has since expired and been reclaimed by another worker reports
    /// [`LeaseAcknowledgement::Lost`] rather than deleting a row that worker
    /// now owns.
    fn ack<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: SideEffectJobId,
        owner: &'life1 SideEffectLeaseOwner,
    ) -> BoxFuture<'async_trait, Result<LeaseAcknowledgement, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;

    /// Release a failed job for another attempt at `next_attempt_at`.
    ///
    /// Guarded by `owner` holding a still-live lease, exactly as [`Self::ack`].
    fn nack<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: SideEffectJobId,
        owner: &'life1 SideEffectLeaseOwner,
        error: &'life2 str,
        next_attempt_at: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<LeaseAcknowledgement, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Dead-letter a job that has exhausted its attempts.
    ///
    /// Guarded by `owner` holding a still-live lease, exactly as [`Self::ack`].
    fn mark_dead<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        id: SideEffectJobId,
        owner: &'life1 SideEffectLeaseOwner,
        error: &'life2 str,
    ) -> BoxFuture<'async_trait, Result<LeaseAcknowledgement, EntityError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;

    /// Remove dead-lettered jobs created before `cutoff`, returning how many
    /// were removed.
    ///
    /// A dead job is kept as evidence of a batch whose deferred side effects
    /// never landed; this is the retention sweep that bounds how long. It
    /// touches no live row and holds no lease.
    fn purge_dead_before<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, EntityError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
}
