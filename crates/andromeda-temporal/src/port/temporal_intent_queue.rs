//! Durable claim, retry and pending state for temporal intent delivery.
//!
//! Only the live branch queues its temporal history, so the queue is the live
//! branch's and no operation names a branch.

use crate::{append_intent::TemporalIntentKey, error::TemporalError};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, write_id::WriteId};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{num::NonZeroU32, time::Duration};

/// Non-zero upper bound for one temporal intent claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalIntentBatchSize(NonZeroU32);

impl TemporalIntentBatchSize {
    /// Construct a non-zero bounded claim size.
    #[must_use]
    pub const fn new(value: NonZeroU32) -> TemporalIntentBatchSize {
        TemporalIntentBatchSize(value)
    }

    /// Return the SQL-compatible claim limit.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// One leased temporal append intent, named rather than read: the queue
/// applies its entries itself, so the drain never holds them.
#[derive(Clone, Debug)]
pub struct ClaimedTemporalIntent {
    /// Database-generated intent key, monotonic in commit order.
    pub key: TemporalIntentKey,
    /// The write the intent's entries are recorded under.
    pub write_id: WriteId,
    /// Entity whose Temporal Evolution the entries extend.
    pub entity_id: Identifier,
    /// Number of earlier delivery attempts.
    pub attempts: u32,
}

/// Retry metadata recorded after a failed application.
#[derive(Clone, Debug)]
pub struct TemporalIntentRetry {
    /// Next eligible claim time.
    pub eligible_at: DateTime<Utc>,
    /// Human-readable application failure summary for operators.
    pub summary: String,
}

/// Whether the queued intents of one entity up to a high-water mark are free
/// for a mutation barrier to claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityRelease {
    /// None of them is queued any more.
    Clear,
    /// Every one still queued is unleased, or its lease has lapsed.
    Claimable,
    /// At least one is held under another owner's live lease, and a claim
    /// takes nothing behind it.
    Held {
        /// The time until the last of those leases lapses, on the queue's own
        /// clock.
        remaining_lease: Duration,
    },
}

/// Durable queue operations consumed by the temporal policy drain.
#[async_trait]
pub trait TemporalIntentQueue: Send + Sync {
    /// Claim the oldest intents whose retry delay has passed, for one bounded
    /// lease interval.
    ///
    /// An intent is claimed only when no earlier intent of its Entity is
    /// leased or waiting for its retry delay, so a claim takes the head of an
    /// Entity's queued intents and the run after it.
    #[must_use = "temporal intent claims must be awaited"]
    async fn claim(&self, owner: LeaseHolder, limit: TemporalIntentBatchSize, lease_until: DateTime<Utc>) -> Result<Vec<ClaimedTemporalIntent>, TemporalError>;

    /// Claim the oldest unleased intents of `entity_id` keyed at most
    /// `through`, before an immediate mutation, whether or not their retry
    /// delay has passed.
    ///
    /// The retry delay paces the background drain; a client waiting on its
    /// own mutation is better served by an attempt now. An intent behind an
    /// earlier one of the Entity under a live lease is not claimed.
    #[must_use = "entity-scoped temporal intent claims must be awaited"]
    async fn claim_for_entity(
        &self,
        owner: LeaseHolder,
        entity_id: &Identifier,
        through: TemporalIntentKey,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalIntent>, TemporalError>;

    /// The key of the newest intent the queue holds for `entity_id`, leased
    /// or not, and [`None`] when it holds none.
    #[must_use = "the high-water probe must be awaited"]
    async fn last_intent_for(&self, entity_id: &Identifier) -> Result<Option<TemporalIntentKey>, TemporalError>;

    /// Wait, at most `wait`, until no intent of `entity_id` keyed at most
    /// `through` is held under a live lease, and report what the wait ended
    /// on.
    ///
    /// Answers at once with [`EntityRelease::Clear`] or
    /// [`EntityRelease::Claimable`]; [`EntityRelease::Held`] only once `wait`
    /// has run out.
    #[must_use = "the release wait must be awaited"]
    async fn await_entity_release(&self, entity_id: &Identifier, through: TemporalIntentKey, wait: Duration) -> Result<EntityRelease, TemporalError>;

    /// Release one intent whose application failed, with explicit retry
    /// metadata.
    ///
    /// Guarded by the caller's live lease: a lease that expired before the
    /// failure is recorded is reported as [`LeaseAcknowledgement::Lost`], and
    /// the intent stays queued for the worker that now holds it.
    #[must_use = "temporal intent retries must be recorded"]
    async fn retry(&self, owner: LeaseHolder, intent: &ClaimedTemporalIntent, retry: TemporalIntentRetry) -> Result<LeaseAcknowledgement, TemporalError>;

    /// Release the leases of `intents`, which were claimed but held back
    /// behind an earlier intent of their Entity, without counting an attempt
    /// and without touching their retry delay or last failure.
    ///
    /// Guarded by the caller's live lease: when it no longer holds one of
    /// them, that one is left to the worker that now holds it and
    /// [`LeaseAcknowledgement::Lost`] is answered.
    #[must_use = "temporal intent releases must be recorded"]
    async fn release(&self, owner: LeaseHolder, intents: &[ClaimedTemporalIntent]) -> Result<LeaseAcknowledgement, TemporalError>;
}
