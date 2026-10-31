//! One Entity a create or update statement wrote, and when it committed.

use andromeda_context_commit::store_instant::StoreInstant;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// An Entity whose current state one statement wrote.
#[derive(Debug)]
pub struct LandedEntity {
    /// The Entity written.
    pub id: Identifier,
    /// The transaction timestamp of the statement's transaction.
    pub committed_at: StoreInstant,
}
