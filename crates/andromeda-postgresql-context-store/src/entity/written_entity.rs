//! The identity a write statement hands back for each entity row it affected.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_codes::entity_key::EntityKey;

/// One entity row a write statement actually affected.
///
/// Both identities are returned because they answer different questions. The
/// identifier is what the caller compares against what it asked for, so that an
/// entity missing from the result is reported as a conflict or a lost update
/// (clauses 6.4.3.1 and 5.6.7). The surrogate key is what every relation owned
/// by the entity carries, so the projection written inside the same transaction
/// has a key to write against without looking the row up again.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WrittenEntity {
    /// The surrogate key owned child relations join and cascade from.
    pub key: EntityKey,
    /// The identifier the API addresses this entity by.
    pub id: Identifier,
}
