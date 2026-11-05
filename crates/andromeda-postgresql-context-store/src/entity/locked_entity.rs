//! One Entity a write that changes no current state locked.

use andromeda_context_commit::entity_version::EntityVersion;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// The identity row of an Entity a write that changes no current state
/// locked, as the lock read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedEntity {
    /// The Entity the row holds.
    pub id: Identifier,
    /// The version its current state stands at, `None` when the row holds a
    /// Temporal Evolution or a queued deletion alone.
    pub current: Option<EntityVersion>,
}
