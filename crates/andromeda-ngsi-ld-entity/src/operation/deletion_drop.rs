//! The members of a written document that ask for a deletion, and dropping
//! them from a document that has nothing to delete (clauses 5.5.8, 5.6.8.4).

use crate::{
    date_time_text::DateTimeText,
    document::{EntityDocument, attribute_member::AttributeMember, slot::Slot, written_value::ValueSlot},
    scopes::Scopes,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use indexmap::IndexMap;
use std::mem;

/// The members [`EntityDocument::drop_deletions`] dropped, as the document
/// held them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeletionMembers {
    /// The deleted scope (clause 4.18), when the document deleted it.
    scope: Option<Slot<Scopes>>,
    /// The deleted entity `expiresAt`, when the document deleted it.
    expires_at: Option<Slot<DateTimeText>>,
    /// Every attribute member that asked only for deletions.
    attributes: IndexMap<AttributeName, AttributeMember>,
}

impl DeletionMembers {
    /// The dropped scope member.
    #[must_use]
    pub const fn scope(&self) -> Option<&Slot<Scopes>> {
        self.scope.as_ref()
    }

    /// The dropped entity `expiresAt` member.
    #[must_use]
    pub const fn expires_at(&self) -> Option<&Slot<DateTimeText>> {
        self.expires_at.as_ref()
    }

    /// The dropped attribute members.
    #[must_use]
    pub const fn attributes(&self) -> &IndexMap<AttributeName, AttributeMember> {
        &self.attributes
    }
}

/// What [`EntityDocument::drop_deletions`] found in the document.
#[derive(Clone, Debug, PartialEq)]
pub enum DroppedDeletions {
    /// At least one member asked for a deletion and was dropped; a caller
    /// that finds the target did exist after all puts them back
    /// ([`EntityDocument::restore_deletions`]).
    Dropped(DeletionMembers),
    /// The document asked for no deletion and is unchanged.
    Unchanged,
}

impl EntityDocument {
    /// Drop every member that asks for a deletion.
    ///
    /// This is clause 5.5.8 applied to a target that contains nothing: "for
    /// each member of the Fragment, whose value is an NGSI-LD Null, contained
    /// by the target, the target member is deleted" leaves a deletion the
    /// target does not contain with nothing to do. Clause 5.6.8.4 is where
    /// that arises: its update mode runs clause 5.6.2 over a payload that
    /// creates the entity when it does not exist yet, and the deletions are
    /// dropped rather than stored, because clause 4.5.0 reserves the value for
    /// a deletion. A member is a deletion when it is the NGSI-LD Null itself or
    /// when every instance it writes is its family's deletion encoding; a
    /// member mixing a deletion with data is not.
    pub fn drop_deletions(&mut self) -> DroppedDeletions {
        let parts = mem::take(self).into_parts();
        let (scope, dropped_scope) = split_deletion(parts.scope);
        let (expires_at, dropped_expiry) = split_deletion(parts.expires_at);
        let (dropped_attributes, attributes): (IndexMap<_, _>, IndexMap<_, _>) = parts.attributes.into_iter().partition(|(_, member)| is_deletion(member));
        *self = EntityDocument::new(parts.id, parts.types, scope, expires_at, attributes);
        if dropped_scope.is_none() && dropped_expiry.is_none() && dropped_attributes.is_empty() {
            return DroppedDeletions::Unchanged;
        }
        DroppedDeletions::Dropped(DeletionMembers {
            scope: dropped_scope,
            expires_at: dropped_expiry,
            attributes: dropped_attributes,
        })
    }

    /// Put the `deletions` [`EntityDocument::drop_deletions`] dropped back.
    ///
    /// Clause 5.6.8.4 update mode creates an entity it found absent; when the
    /// entity turns out to exist after all (created concurrently), the same
    /// request is an update of it by clause 5.6.2, whose fragment carries the
    /// deletions again.
    pub fn restore_deletions(&mut self, deletions: DeletionMembers) {
        let mut parts = mem::take(self).into_parts();
        parts.attributes.extend(deletions.attributes);
        *self = EntityDocument::new(
            parts.id,
            parts.types,
            deletions.scope.or(parts.scope),
            deletions.expires_at.or(parts.expires_at),
            parts.attributes,
        );
    }
}

/// Split a member into what the document keeps and the deletion it drops.
fn split_deletion<T>(member: Option<Slot<T>>) -> (Option<Slot<T>>, Option<Slot<T>>) {
    match member {
        Some(Slot::Remove) => (None, Some(Slot::Remove)),
        kept @ (Some(Slot::Set(_)) | None) => (kept, None),
    }
}

/// Whether an attribute member asks for nothing but deletions.
fn is_deletion(member: &AttributeMember) -> bool {
    match member {
        AttributeMember::Remove => true,
        AttributeMember::Instances(instances) => !instances.is_empty() && instances.iter().all(|instance| matches!(instance.value(), ValueSlot::Remove(_))),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{EntityDocument, attribute_member::AttributeMember, slot::Slot},
        operation::{
            deletion_drop::DroppedDeletions,
            speed_entity::{name, number, property_deletion, property_document},
        },
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use indexmap::IndexMap;

    /// A document creating a device: one live attribute and three deletions.
    fn requested() -> EntityDocument {
        let mut attributes = IndexMap::new();
        attributes.insert(
            name("https://example.com/speed"),
            AttributeMember::Instances(vec![property_document(number(10), None)]),
        );
        attributes.insert(name("https://example.com/gone"), AttributeMember::Instances(vec![property_deletion(None)]));
        attributes.insert(name("https://example.com/bare"), AttributeMember::Remove);
        EntityDocument::new(
            Some("urn:ngsi-ld:Device:1".parse().expect("a valid identifier")),
            vec!["https://example.com/Device".parse().expect("a valid type")],
            Some(Slot::Remove),
            None,
            attributes,
        )
    }

    /// Clause 5.6.8.4 update mode runs clause 5.6.2 over a payload that also
    /// creates the entity when it does not exist: on that branch the deletions
    /// have nothing to delete and are dropped, while everything else is kept.
    #[test]
    fn dropping_deletion_sentinels_keeps_the_data_and_removes_every_deletion() {
        let mut document = requested();

        let DroppedDeletions::Dropped(dropped) = document.drop_deletions() else {
            panic!("the deletions are dropped: {document:?}");
        };

        pretty_assertions::assert_eq!(
            dropped.attributes().keys().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/gone", "https://example.com/bare"]
        );
        pretty_assertions::assert_eq!(dropped.scope(), Some(&Slot::Remove));
        pretty_assertions::assert_eq!(document.id().map(Identifier::as_str), Some("urn:ngsi-ld:Device:1"));
        pretty_assertions::assert_eq!(document.scope(), None);
        pretty_assertions::assert_eq!(
            document.attributes().keys().map(AttributeName::as_str).collect::<Vec<_>>(),
            vec!["https://example.com/speed"]
        );
    }

    /// Restoring what was dropped gives back the document the request
    /// carried, which is the fragment clause 5.6.2 applies when the entity
    /// turns out to exist after all.
    #[test]
    fn restoring_the_dropped_deletions_gives_back_the_requested_payload() {
        let mut document = requested();
        let DroppedDeletions::Dropped(dropped) = document.drop_deletions() else {
            panic!("the deletions are dropped: {document:?}");
        };

        document.restore_deletions(dropped);

        pretty_assertions::assert_eq!(document, requested());
    }

    /// A document that asks for no deletion is left exactly as it was.
    #[test]
    fn dropping_deletion_sentinels_leaves_a_payload_without_one_unchanged() {
        let mut attributes = IndexMap::new();
        attributes.insert(
            name("https://example.com/speed"),
            AttributeMember::Instances(vec![property_document(number(10), None)]),
        );
        let original = EntityDocument::new(None, Vec::new(), None, None, attributes);
        let mut document = original.clone();

        pretty_assertions::assert_eq!(document.drop_deletions(), DroppedDeletions::Unchanged);
        pretty_assertions::assert_eq!(document, original);
    }

    /// An attribute whose instances mix a deletion with live data is not a
    /// deletion of the member: the patch decides per instance.
    #[test]
    fn an_attribute_mixing_a_deletion_with_data_is_not_dropped_as_a_deletion() {
        let mut attributes = IndexMap::new();
        attributes.insert(
            name("https://example.com/speed"),
            AttributeMember::Instances(vec![
                property_deletion(Some("urn:ngsi-ld:Dataset:A")),
                property_document(number(20), Some("urn:ngsi-ld:Dataset:B")),
            ]),
        );
        let mut document = EntityDocument::new(None, Vec::new(), None, None, attributes);

        pretty_assertions::assert_eq!(document.drop_deletions(), DroppedDeletions::Unchanged);
        assert!(document.attributes().contains_key("https://example.com/speed"));
    }
}
