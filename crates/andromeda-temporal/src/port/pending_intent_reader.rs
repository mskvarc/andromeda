//! Reads committed temporal append intents that still participate in the overlay.

use crate::{append_intent::SequencedTemporalAppendIntent, error::TemporalError};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use async_trait::async_trait;

/// Reads committed intents until their durable delivery acknowledgement lands.
#[async_trait]
pub trait PendingTemporalIntentReader: Send + Sync {
    /// Return every pending intent for one entity in anchor sequence order.
    #[must_use = "pending temporal intents must be awaited"]
    async fn pending_for_entity(&self, entity_id: &Uri) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError>;

    /// Return every pending intent in anchor sequence order.
    #[must_use = "pending temporal intents must be awaited"]
    async fn pending_intents(&self) -> Result<Vec<SequencedTemporalAppendIntent>, TemporalError>;
}
