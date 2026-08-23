//! Reads committed temporal append intents that still participate in the overlay.

use crate::{append_intent::SequencedTemporalAppendIntent, error::TemporalError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use async_trait::async_trait;

/// Reads committed intents until their durable delivery acknowledgement lands.
#[async_trait]
pub trait PendingTemporalIntentReader: Send + Sync {
    /// Return every pending intent for one entity in anchor sequence order.
    #[must_use = "pending temporal intents must be awaited"]
    async fn pending_for_entity(&self, target: &ContextTarget, entity_id: &Uri) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError>;

    /// Return every pending intent in anchor sequence order.
    #[must_use = "pending temporal intents must be awaited"]
    async fn pending_intents(&self, target: &ContextTarget) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError>;
}
