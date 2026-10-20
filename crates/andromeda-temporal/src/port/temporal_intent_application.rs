//! Applying claimed temporal append intents to the live branch's history.

use crate::{error::TemporalError, port::temporal_intent_queue::ClaimedTemporalIntent};
use andromeda_context_commit::lease_holder::LeaseHolder;
use async_trait::async_trait;

/// How one application of claimed intents ended without an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemporalIntentApplicationOutcome {
    /// Every intent is history and has left the queue.
    Applied,
    /// The owner no longer held the live lease of one of them; nothing was
    /// applied and the intents are left to the worker that now holds them.
    LeaseLost,
    /// An earlier intent of an Entity in the set is still queued outside it;
    /// nothing was applied, and the lease of every intent is still held.
    ///
    /// Clauses 4.5.7, 4.5.8 and 4.8 read an Entity's history in the order its
    /// writes committed, so no intent is applied ahead of an earlier one of
    /// its Entity.
    Behind,
}

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
    /// Returns [`TemporalIntentApplicationOutcome::LeaseLost`] when `owner` no
    /// longer holds the live lease of one of them, and
    /// [`TemporalIntentApplicationOutcome::Behind`] when an earlier intent of
    /// one of their Entities is queued outside `intents`, both having applied
    /// nothing.
    ///
    /// # Errors
    ///
    /// Returns the [`TemporalError`] of an intent that cannot be applied, which
    /// applies none of them: an observation of an Entity that has no Temporal
    /// Evolution and states no type, an entry that breaks a history
    /// constraint, or a failing statement.
    #[must_use = "temporal intent applications must be awaited"]
    async fn apply(&self, owner: LeaseHolder, intents: &[ClaimedTemporalIntent]) -> Result<TemporalIntentApplicationOutcome, TemporalError>;
}
