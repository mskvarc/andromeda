//! Durable claim, acknowledgement and retry state for temporal intent delivery.

use crate::{append_intent::SequencedTemporalAppendIntent, error::TemporalError};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
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

/// One leased temporal append intent.
#[derive(Clone, Debug)]
pub struct ClaimedTemporalAppendIntent {
    /// Durable ordered intent.
    pub pending: SequencedTemporalAppendIntent,
    /// Number of earlier delivery attempts.
    pub attempts: u32,
}

/// Retry metadata recorded after a sink rejection.
#[derive(Clone, Debug)]
pub struct TemporalIntentRetry {
    /// Next eligible claim time.
    pub eligible_at: DateTime<Utc>,
    /// Human-readable sink failure summary for operators.
    pub summary: String,
}

/// Durable queue operations consumed by the temporal policy drain.
#[async_trait]
pub trait TemporalIntentQueue: Send + Sync {
    /// Claim the oldest eligible intents for one bounded lease interval.
    #[must_use = "temporal intent claims must be awaited"]
    async fn claim(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError>;

    /// Claim the oldest eligible intents for one entity before an immediate mutation.
    #[must_use = "entity-scoped temporal intent claims must be awaited"]
    async fn claim_for_entity(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        entity_id: &Uri,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError>;

    /// Acknowledge one intent only after the sink confirmed durability.
    ///
    /// The acknowledgement is guarded by the caller's live lease: a lease that
    /// expired before the sink confirmed is reported as
    /// [`LeaseAcknowledgement::Lost`], and the intent stays queued for the
    /// worker that now holds it. The sink is idempotent under the write
    /// identity, so a second delivery lands nothing twice.
    #[must_use = "temporal intent acknowledgements must be awaited"]
    async fn acknowledge(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        intent: &SequencedTemporalAppendIntent,
    ) -> Result<LeaseAcknowledgement, TemporalError>;

    /// Release one failed intent with explicit retry metadata.
    ///
    /// Guarded by the caller's live lease exactly as [`TemporalIntentQueue::acknowledge`] is.
    #[must_use = "temporal intent retries must be recorded"]
    async fn retry(
        &self,
        target: &ContextTarget,
        owner: LeaseHolder,
        intent: &SequencedTemporalAppendIntent,
        retry: TemporalIntentRetry,
    ) -> Result<LeaseAcknowledgement, TemporalError>;
}
