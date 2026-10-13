//! Durable claim, retry and pending state for temporal intent delivery.
//!
//! Only the live branch defers its temporal history, so the queue is the live
//! branch's and no operation names a branch.

use crate::{append_intent::TemporalIntentKey, error::TemporalError};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, write_id::WriteId};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::num::NonZeroU32;

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

/// Durable queue operations consumed by the temporal policy drain.
#[async_trait]
pub trait TemporalIntentQueue: Send + Sync {
    /// Claim the oldest eligible intents for one bounded lease interval.
    #[must_use = "temporal intent claims must be awaited"]
    async fn claim(&self, owner: LeaseHolder, limit: TemporalIntentBatchSize, lease_until: DateTime<Utc>) -> Result<Vec<ClaimedTemporalIntent>, TemporalError>;

    /// Claim the oldest eligible intents for one entity before an immediate mutation.
    #[must_use = "entity-scoped temporal intent claims must be awaited"]
    async fn claim_for_entity(
        &self,
        owner: LeaseHolder,
        entity_id: &Identifier,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalIntent>, TemporalError>;

    /// Whether the queue holds an intent for `entity_id`, leased or not.
    #[must_use = "the pending probe must be awaited"]
    async fn holds_intent_for(&self, entity_id: &Identifier) -> Result<bool, TemporalError>;

    /// Release one intent whose application failed, with explicit retry
    /// metadata.
    ///
    /// Guarded by the caller's live lease: a lease that expired before the
    /// failure is recorded is reported as [`LeaseAcknowledgement::Lost`], and
    /// the intent stays queued for the worker that now holds it.
    #[must_use = "temporal intent retries must be recorded"]
    async fn retry(&self, owner: LeaseHolder, intent: &ClaimedTemporalIntent, retry: TemporalIntentRetry) -> Result<LeaseAcknowledgement, TemporalError>;
}
