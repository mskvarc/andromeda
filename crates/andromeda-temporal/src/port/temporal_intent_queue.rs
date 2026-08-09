//! Durable claim, acknowledgement and retry state for temporal intent delivery.

use crate::{append_intent::SequencedTemporalAppendIntent, error::TemporalError};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use derive_more::Display;
use std::num::NonZeroU32;
use uuid::Uuid;

/// Stable owner of one temporal drain's durable leases.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
#[display("{_0}")]
pub struct TemporalIntentLeaseOwner(Uuid);

impl TemporalIntentLeaseOwner {
    /// Mint one owner identifier for a constructed tenant drain.
    pub fn mint() -> TemporalIntentLeaseOwner {
        TemporalIntentLeaseOwner(Uuid::new_v4())
    }

    /// Return the UUID persistence representation.
    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

/// Non-zero upper bound for one temporal intent claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TemporalIntentBatchSize(NonZeroU32);

impl TemporalIntentBatchSize {
    /// Construct a non-zero bounded claim size.
    pub const fn new(value: NonZeroU32) -> TemporalIntentBatchSize {
        TemporalIntentBatchSize(value)
    }

    /// Return the SQL-compatible claim limit.
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
        owner: TemporalIntentLeaseOwner,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError>;

    /// Claim the oldest eligible intents for one entity before an immediate mutation.
    #[must_use = "entity-scoped temporal intent claims must be awaited"]
    async fn claim_for_entity(
        &self,
        owner: TemporalIntentLeaseOwner,
        entity_id: &Uri,
        limit: TemporalIntentBatchSize,
        lease_until: DateTime<Utc>,
    ) -> Result<Vec<ClaimedTemporalAppendIntent>, TemporalError>;

    /// Acknowledge one intent only after the sink confirmed durability.
    #[must_use = "temporal intent acknowledgements must be awaited"]
    async fn acknowledge(&self, owner: TemporalIntentLeaseOwner, intent: &SequencedTemporalAppendIntent) -> Result<(), TemporalError>;

    /// Release one failed intent with explicit retry metadata.
    #[must_use = "temporal intent retries must be recorded"]
    async fn retry(&self, owner: TemporalIntentLeaseOwner, intent: &SequencedTemporalAppendIntent, retry: TemporalIntentRetry) -> Result<(), TemporalError>;
}
