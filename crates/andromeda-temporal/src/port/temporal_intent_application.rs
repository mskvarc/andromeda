//! Applying claimed temporal append intents to the live branch's history.

use crate::{error::TemporalError, port::temporal_intent_queue::ClaimedTemporalIntent};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use async_trait::async_trait;

/// Moves the entries of claimed intents into history and removes the intents
/// from the queue, in one transaction.
///
/// Applying and removing are one step, so a reader sees each entry either
/// queued or recorded, never both and never neither. Idempotency is row-level,
/// on the history rows' write identity, so an intent applied again after a
/// lost lease records nothing twice.
#[async_trait]
pub trait TemporalIntentApplication: Send + Sync {
    /// Apply every intent of `intents` and remove them, all or none.
    ///
    /// Returns [`LeaseAcknowledgement::Lost`], having applied nothing, when
    /// `owner` no longer holds the live lease of one of them.
    ///
    /// # Errors
    ///
    /// Returns the [`TemporalError`] of an intent that cannot be applied, which
    /// applies none of them: an observation of an Entity that has no Temporal
    /// Evolution and states no type, an entry that breaks a history
    /// constraint, or a failing statement.
    #[must_use = "temporal intent applications must be awaited"]
    async fn apply(&self, owner: LeaseHolder, intents: &[ClaimedTemporalIntent]) -> Result<LeaseAcknowledgement, TemporalError>;
}
