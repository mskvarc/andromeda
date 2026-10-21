//! The compact JSON the JSON-LD processor expands when the typed walk declines a
//! request: the request's serialization, with the NGSI-LD Null the walk reads as
//! the value of a concise Property written as that Property.
//!
//! The processor reads a request as JSON-LD, where a Null written as a whole
//! member is a plain string value. NGSI-LD reads it otherwise (clauses 4.5.0
//! and 4.5.2.3): below the top level it is always the value of a concise
//! Property, which the model writes itself when it writes a sub-attribute, and at
//! the top level either the same or, for a merge fragment or a notification,
//! the deletion of the whole member. The typed walk makes that decision as it
//! expands; a request the walk declines is written for the processor with the
//! decision already made.

use crate::{
    error::ExpansionError,
    normalization::Normalization,
    written_member::{WrittenMember, WrittenMemberRef},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::rendering::instant_text};
use andromeda_ngsi_ld_model::{
    attribute::{instance::AttributeValue, merge_member::MergeMember, nested_null::NullProperty},
    entity::{Entity, EntityPatch},
    entity_type::EntityTypeValue,
    scope_member::ScopeMember,
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer, ser::SerializeMap};

/// One top-level attribute member as the processor is handed it.
struct ProcessorMember<'a, M> {
    /// The member.
    member: &'a M,
    /// What a Null written in place of the whole member is.
    normalization: Normalization,
}

impl<M: WrittenMember> Serialize for ProcessorMember<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let is_null = match self.member.written() {
            WrittenMemberRef::Attribute(member) => matches!(member, AttributeValue::NgsiLdNull),
            WrittenMemberRef::Merge(member) => matches!(member, MergeMember::NgsiLdNull),
        };
        match (is_null, self.normalization) {
            (true, Normalization::Entity) => NullProperty.serialize(serializer),
            (true, Normalization::MemberDeletions) | (false, Normalization::Entity | Normalization::MemberDeletions) => self.member.serialize(serializer),
        }
    }
}

/// The entity members clause 5.2.4 defines, written beside the attributes.
struct EntityMembers<'a> {
    /// The identifier, when the request names one.
    id: Option<&'a Identifier>,
    /// The types, when the request names any.
    type_: Option<&'a EntityTypeValue>,
    /// The Scope.
    scope: Option<&'a ScopeMember>,
    /// The expiry.
    expires_at: Option<&'a DateTime<Utc>>,
    /// The system times: creation, modification, deletion.
    system: [Option<&'a DateTime<Utc>>; 3],
}

impl EntityMembers<'_> {
    /// Write the members into `map`.
    fn write<M: SerializeMap>(&self, map: &mut M) -> Result<(), M::Error> {
        if let Some(id) = self.id {
            map.serialize_entry("id", id)?;
        }
        if let Some(type_) = self.type_ {
            map.serialize_entry("type", type_)?;
        }
        if let Some(scope) = self.scope {
            map.serialize_entry(compact_properties::SCOPE, scope)?;
        }
        if let Some(expires_at) = self.expires_at {
            map.serialize_entry(compact_properties::EXPIRES_AT, &instant_text(*expires_at))?;
        }
        for (name, instant) in [compact_properties::CREATED_AT, compact_properties::MODIFIED_AT, compact_properties::DELETED_AT]
            .into_iter()
            .zip(self.system)
        {
            if let Some(instant) = instant {
                map.serialize_entry(name, &instant_text(*instant))?;
            }
        }
        Ok(())
    }
}

/// An entity as the processor is handed it.
struct ProcessorEntity<'a, M> {
    /// The entity.
    entity: &'a Entity<M>,
    /// What a Null written in place of a whole member is.
    normalization: Normalization,
}

impl<M: WrittenMember> Serialize for ProcessorEntity<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let entity = self.entity;
        let mut map = serializer.serialize_map(None)?;
        EntityMembers {
            id: Some(&entity.id),
            type_: Some(&entity.type_),
            scope: entity.scope.as_ref(),
            expires_at: entity.expires_at.as_ref(),
            system: [entity.created_at.as_ref(), entity.modified_at.as_ref(), entity.deleted_at.as_ref()],
        }
        .write(&mut map)?;
        for (name, member) in entity.attribute_members() {
            map.serialize_entry(
                name,
                &ProcessorMember {
                    member,
                    normalization: self.normalization,
                },
            )?;
        }
        map.end()
    }
}

/// An entity fragment as the processor is handed it.
struct ProcessorPatch<'a, M> {
    /// The fragment.
    patch: &'a EntityPatch<M>,
    /// What a Null written in place of a whole member is.
    normalization: Normalization,
}

impl<M: WrittenMember> Serialize for ProcessorPatch<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let patch = self.patch;
        let mut map = serializer.serialize_map(None)?;
        EntityMembers {
            id: patch.id.as_ref(),
            type_: patch.type_.as_ref(),
            scope: patch.scope.as_ref(),
            expires_at: patch.expires_at.as_ref(),
            system: [None, None, None],
        }
        .write(&mut map)?;
        for (name, member) in patch.attribute_members() {
            map.serialize_entry(
                name,
                &ProcessorMember {
                    member,
                    normalization: self.normalization,
                },
            )?;
        }
        map.end()
    }
}

/// The text of `entity` as the processor is handed it.
///
/// # Errors
///
/// Returns [`ExpansionError::CompactDocument`] when the request does not
/// serialize.
pub(crate) fn entity_text<M: WrittenMember>(entity: &Entity<M>, normalization: Normalization) -> Result<Vec<u8>, ExpansionError> {
    serde_json::to_vec(&ProcessorEntity { entity, normalization }).map_err(ExpansionError::CompactDocument)
}

/// The text of `patch` as the processor is handed it.
///
/// # Errors
///
/// As [`entity_text`].
pub(crate) fn patch_text<M: WrittenMember>(patch: &EntityPatch<M>, normalization: Normalization) -> Result<Vec<u8>, ExpansionError> {
    serde_json::to_vec(&ProcessorPatch { patch, normalization }).map_err(ExpansionError::CompactDocument)
}

/// The text of the one `member` written for the attribute `attribute`, as the
/// processor is handed it.
///
/// # Errors
///
/// As [`entity_text`].
pub(crate) fn attribute_text<M: WrittenMember>(attribute: &AttributeName, member: &M) -> Result<Vec<u8>, ExpansionError> {
    let written = OneAttribute {
        name: attribute.as_str(),
        member: ProcessorMember {
            member,
            normalization: Normalization::Entity,
        },
    };
    serde_json::to_vec(&written).map_err(ExpansionError::CompactDocument)
}

/// The object `{"<attribute>": <member>}`.
struct OneAttribute<'a, M> {
    /// The attribute's name.
    name: &'a str,
    /// The member written under it.
    member: ProcessorMember<'a, M>,
}

impl<M: WrittenMember> Serialize for OneAttribute<'_, M> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry(self.name, &self.member)?;
        map.end()
    }
}
