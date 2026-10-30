//! One Entity a read over many Entities reached, read back or not.
//!
//! A read over many Entities (a query page, a listing page) answers for each
//! of them on its own: one whose stored rows no longer read back as an Entity
//! is named by its identifier in its place, and the others read back around
//! it. The store records why it does not read back; its caller decides whether
//! to leave it out or to fail.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// One Entity of a read over many, in its place in the read.
#[derive(Clone, Debug)]
pub enum StoredEntry<T> {
    /// The Entity as it is stored.
    Read(T),
    /// An Entity the store holds whose stored rows no longer read back as an
    /// Entity, named by its identifier.
    Unreadable(Identifier),
}

impl<T> StoredEntry<T> {
    /// The Entity as it is stored, `None` when it does not read back.
    #[must_use]
    pub fn read(self) -> Option<T> {
        match self {
            StoredEntry::Read(entity) => Some(entity),
            StoredEntry::Unreadable(_) => None,
        }
    }

    /// The entry with `f` applied to the Entity it read back.
    #[must_use]
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> StoredEntry<U> {
        match self {
            StoredEntry::Read(entity) => StoredEntry::Read(f(entity)),
            StoredEntry::Unreadable(id) => StoredEntry::Unreadable(id),
        }
    }

    /// The identifier of the Entity, read off the Entity by `id` when it read
    /// back.
    #[must_use]
    pub fn id<'entry>(&'entry self, id: impl FnOnce(&'entry T) -> &'entry Identifier) -> &'entry Identifier {
        match self {
            StoredEntry::Read(entity) => id(entity),
            StoredEntry::Unreadable(unreadable) => unreadable,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::port::stored_entry::StoredEntry;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("a valid URI")
    }

    #[test]
    fn an_entity_read_back_is_handed_on_and_mapped() {
        let entry = StoredEntry::Read((uri("urn:ngsi-ld:A"), 7));
        assert_eq!(entry.id(|(id, _)| id), &uri("urn:ngsi-ld:A"));
        assert_eq!(entry.map(|(_, value)| value * 2).read(), Some(14));
    }

    #[test]
    fn an_unreadable_entity_is_named_and_holds_no_entity() {
        let entry: StoredEntry<(Identifier, i32)> = StoredEntry::Unreadable(uri("urn:ngsi-ld:B"));
        assert_eq!(entry.id(|(id, _)| id), &uri("urn:ngsi-ld:B"));
        assert_eq!(entry.map(|(_, value)| value).read(), None);
    }
}
