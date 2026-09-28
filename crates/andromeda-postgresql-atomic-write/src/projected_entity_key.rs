//! The surrogate key a commit projects an entity's attribute instances against,
//! and whether the commit's own insert minted it.

use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_context_store::entity::inserted_entity::MintedEntityKey;

/// The surrogate key of one entity a commit's statements wrote or resolved.
///
/// A key the commit's fused insert minted owns no projection row yet, so its
/// rows are inserted without matching; every other key -- one an update
/// returned or a lookup resolved -- may already own rows, so its rows are
/// upserted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProjectedEntityKey {
    /// The commit's own insert minted the key.
    Minted(MintedEntityKey),
    /// The key existed before the commit.
    Existing(EntityKey),
}

impl ProjectedEntityKey {
    /// The surrogate key, whichever way the commit came by it.
    pub(crate) const fn key(self) -> EntityKey {
        match self {
            ProjectedEntityKey::Minted(minted) => minted.key(),
            ProjectedEntityKey::Existing(key) => key,
        }
    }
}
