//! Synchronization boundary between deferred appends and immediate mutations.

use crate::error::TemporalError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_trait::async_trait;

/// Makes committed history for one entity durable in the selected sink before mutation.
#[async_trait]
pub trait TemporalMutationBarrier: Send + Sync {
    /// Deliver pending append intents for the entity before an immediate update or delete.
    #[must_use = "temporal mutation synchronization must be awaited"]
    async fn synchronize(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<(), TemporalError>;
}
