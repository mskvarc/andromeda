//! Synchronization boundary between queued history and immediate mutations.

use crate::error::TemporalError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use async_trait::async_trait;

/// Makes the queued history of one entity part of its stored history before a
/// mutation of it runs.
#[async_trait]
pub trait TemporalMutationBarrier: Send + Sync {
    /// Apply the entity's queued append intents before an immediate update or
    /// delete of its history.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError::HistoryUnsettled`] when another delivery owner
    /// still holds one of the intents once the barrier's wait has run out, and
    /// the failure of an intent that does not apply, classified as its own.
    #[must_use = "temporal mutation synchronization must be awaited"]
    async fn synchronize(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<(), TemporalError>;
}
