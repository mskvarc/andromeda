//! Idempotent temporal-history append capability.

use crate::{append_intent::TemporalAppendIntent, error::TemporalError};
use async_trait::async_trait;

/// Appends complete intents atomically and idempotently by intent identifier.
#[async_trait]
pub trait TemporalHistorySink: Send + Sync {
    /// Durably append one intent's entries. Idempotency is row-level, on the
    /// entries' own `ON CONFLICT ... DO NOTHING` constraints, so a replay of
    /// the same intent reaches this call again and simply collides with the
    /// rows the first attempt wrote rather than needing to be told the intent
    /// was already accepted.
    #[must_use = "temporal sink appends must be awaited"]
    async fn append(&self, intent: &TemporalAppendIntent) -> Result<(), TemporalError>;
}
