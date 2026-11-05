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
    /// Returns [`TemporalError::HistoryUnsettled`] when the barrier's bound,
    /// which covers applying the intents and waiting for another delivery
    /// owner's lease on them, runs out with one of them still queued, and the
    /// failure of an intent that does not apply, classified as its own.
    #[must_use = "temporal mutation synchronization must be awaited"]
    async fn synchronize(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<(), TemporalError>;
}
