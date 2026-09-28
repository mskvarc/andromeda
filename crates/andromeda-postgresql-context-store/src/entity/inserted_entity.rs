//! The identity the fused entity insert hands back for each entity it created,
//! and the proof that its surrogate key is new.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_codes::entity_key::EntityKey;

/// A surrogate key this transaction's own fused entity insert minted.
///
/// Holding one proves that no relation owned by the entity holds a row for it,
/// committed or in flight: `entities.entity_key` is the primary key, so no
/// other `entities` row carries it, and every owned relation references
/// `entities` through an immediate foreign key, so no other transaction can
/// write a row against a key it cannot see and any earlier row with the key
/// would have cascaded away with its entity. A write of the entity's owned rows
/// can therefore insert them without matching or conflict arbitration. Only the
/// fused insert can construct one.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct MintedEntityKey(EntityKey);

impl MintedEntityKey {
    /// Wrap a key the fused insert's `RETURNING` just handed back.
    pub(crate) const fn new(key: EntityKey) -> MintedEntityKey {
        MintedEntityKey(key)
    }

    /// The surrogate key itself.
    #[must_use]
    pub const fn key(self) -> EntityKey {
        self.0
    }
}

/// One entity row the fused insert created.
///
/// The identifier is what the caller compares against what it asked for, so an
/// entity missing from the result is reported as a conflict (clause 6.4.3.1);
/// the minted key is what every relation owned by the entity is written
/// against, without a matching step.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct InsertedEntity {
    /// The surrogate key the insert minted.
    pub key: MintedEntityKey,
    /// The identifier the API addresses this entity by.
    pub id: Uri,
}
