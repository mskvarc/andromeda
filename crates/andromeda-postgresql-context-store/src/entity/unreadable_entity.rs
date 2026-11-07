//! An Entity a read over many reached whose stored rows no longer read back,
//! and the two things a caller does with it: fail the whole read, or leave the
//! Entity out and record why.
//!
//! A read by identifier fails, because the caller asked for that Entity and a
//! write must not start from a state it cannot read. A query page, a listing
//! page and the expiry sweep leave it out, so one Entity stored in a form this
//! broker no longer reads does not stop every read that reaches it.

use crate::error::ContextStoreError;
use andromeda_entity::port::stored_entry::StoredEntry;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use tracing::error;

/// An Entity whose head or rows do not read back, with why.
#[derive(Debug)]
pub(crate) struct UnreadableEntity {
    /// The Entity's identifier, which its head always reads back with.
    pub(crate) id: Identifier,
    /// The first of its columns or rows that did not read back, boxed so an
    /// entry of a read stays the size of the Entity it holds.
    pub(crate) fault: Box<ContextStoreError>,
}

/// Every Entity of `entries`, or the fault of the first that did not read
/// back.
///
/// # Errors
///
/// Returns the [`ContextStoreError`] the first unreadable Entity met.
pub(crate) fn every_entity<T>(entries: Vec<Result<T, UnreadableEntity>>) -> Result<Vec<T>, ContextStoreError> {
    entries.into_iter().map(|entry| entry.map_err(|unreadable| *unreadable.fault)).collect()
}

/// The entry a read that leaves an unreadable Entity out hands on in its
/// place, the fault logged for operators.
pub(crate) fn stored_entry<T>(entry: Result<T, UnreadableEntity>) -> StoredEntry<T> {
    match entry {
        Ok(entity) => StoredEntry::Read(entity),
        Err(UnreadableEntity { id, fault }) => {
            error!(entity = %id, %fault, "a stored entity does not read back and is left out of the read");
            StoredEntry::Unreadable(id)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::{
            stored_entity_error::StoredEntityError,
            unreadable_entity::{UnreadableEntity, every_entity, stored_entry},
        },
        error::ContextStoreError,
    };
    use andromeda_entity::port::stored_entry::StoredEntry;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("a valid URI")
    }

    fn unreadable(raw: &str) -> UnreadableEntity {
        UnreadableEntity {
            id: uri(raw),
            fault: Box::new(ContextStoreError::from(StoredEntityError::Untyped)),
        }
    }

    #[test]
    fn a_read_of_every_entity_fails_with_the_first_unreadable_one() {
        assert!(matches!(
            every_entity(vec![Ok(1), Err(unreadable("urn:ngsi-ld:B")), Ok(3)]),
            Err(ContextStoreError::StoredEntity(StoredEntityError::Untyped))
        ));
        assert_eq!(every_entity::<i32>(vec![Ok(1), Ok(3)]).expect("every entity reads back"), vec![1, 3]);
    }

    #[test]
    fn an_unreadable_entity_is_named_in_its_place() {
        let entries: Vec<StoredEntry<i32>> = vec![Ok(1), Err(unreadable("urn:ngsi-ld:B"))].into_iter().map(stored_entry).collect();
        assert!(matches!(entries.as_slice(), [StoredEntry::Read(1), StoredEntry::Unreadable(id)] if *id == uri("urn:ngsi-ld:B")));
    }
}
