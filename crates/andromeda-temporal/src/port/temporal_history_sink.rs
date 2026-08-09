//! Idempotent temporal-history append capability.

use crate::{append_intent::TemporalAppendIntent, error::TemporalError};
use async_trait::async_trait;

/// Result of one idempotent sink append.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemporalSinkAppendOutcome {
    /// The sink durably accepted the intent and its observations.
    Appended,
    /// The sink had already durably accepted the same intent identifier.
    AlreadyPresent,
}

/// Appends complete intents atomically and idempotently by intent identifier.
#[async_trait]
pub trait TemporalHistorySink: Send + Sync {
    /// Durably append one intent or confirm that it was already appended.
    #[must_use = "temporal sink appends must be awaited"]
    async fn append(&self, intent: &TemporalAppendIntent) -> Result<TemporalSinkAppendOutcome, TemporalError>;
}
