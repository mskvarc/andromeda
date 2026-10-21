//! Reading an expanded entity document (clause 5.5.7) into an
//! [`EntityDocument`].
//!
//! The stored form is a one-element array holding the entity node; a bare node
//! is read the same way. The node's `@id` and `@type` name the entity, `scope`
//! and `expiresAt` are its two non-attribute members (clauses 4.18, 4.22), and
//! every other member is an attribute.

use crate::{
    codec::{
        decode_context::DecodeContext,
        element::{ElementsSeed, TypeEntries},
        instance_decode::{AttributeMemberSeed, InstanceDecoded, InstanceSeed},
        member_decode::{date_time, malformed, scope},
        object_key::{ObjectKey, attribute_key},
    },
    document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument, slot::Slot},
    member_role::MemberRole,
    member_shape::MemberShape,
    refusal::EntityRefusal,
    scopes::Scopes,
};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    time::written_date_time::DateTimeText,
};
use indexmap::IndexMap;
use serde::{
    Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

/// Reads a whole document: `[node]` or `node`.
#[derive(Clone, Copy)]
pub(crate) struct DocumentSeed<'c> {
    /// Where a refusal is recorded.
    pub(crate) context: &'c DecodeContext,
}

impl<'de> DeserializeSeed<'de> for DocumentSeed<'_> {
    type Value = EntityDocument;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<EntityDocument, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for DocumentSeed<'_> {
    type Value = EntityDocument;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded entity document")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<EntityDocument, A::Error> {
        let Some(document) = seq.next_element_seed(EntityNodeSeed { context: self.context })? else {
            return Err(self.context.refuse(malformed(None, MemberShape::Root)));
        };
        if seq.next_element::<IgnoredAny>()?.is_some() {
            return Err(self.context.refuse(malformed(None, MemberShape::Root)));
        }
        Ok(document)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<EntityDocument, A::Error> {
        EntityNodeSeed { context: self.context }.visit_map(map)
    }
}

/// Reads one entity node.
#[derive(Clone, Copy)]
struct EntityNodeSeed<'c> {
    /// Where a refusal is recorded.
    context: &'c DecodeContext,
}

impl<'de> DeserializeSeed<'de> for EntityNodeSeed<'_> {
    type Value = EntityDocument;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<EntityDocument, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for EntityNodeSeed<'_> {
    type Value = EntityDocument;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded entity node")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<EntityDocument, A::Error> {
        let refuse = |refusal: EntityRefusal| self.context.refuse::<A::Error>(refusal);
        let elements = ElementsSeed {
            context: self.context,
            owner: None,
        };
        let mut id = None;
        let mut types = Vec::new();
        let mut scope_member: Option<Slot<Scopes>> = None;
        let mut expires_at: Option<Slot<DateTimeText>> = None;
        let mut attributes: IndexMap<AttributeName, AttributeMember> = IndexMap::new();
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::Id => {
                    let text = map.next_value::<String>()?;
                    id = Some(text.parse::<Identifier>().map_err(|source| refuse(EntityRefusal::EntityId { source }))?);
                }
                ObjectKey::Type => types = entity_types(map.next_value::<TypeEntries>()?).map_err(refuse)?,
                ObjectKey::Name(name) => {
                    let name = attribute_key(&name).map_err(refuse)?;
                    match MemberRole::of(name.as_str()) {
                        MemberRole::Scope => scope_member = Some(scope(&map.next_value_seed(elements)?).map_err(refuse)?),
                        MemberRole::ExpiresAt => {
                            expires_at = Some(date_time(map.next_value_seed(elements)?, None, MemberShape::EntityExpiresAt).map_err(refuse)?);
                        }
                        MemberRole::Attribute => {
                            let member = map.next_value_seed(AttributeMemberSeed {
                                context: self.context,
                                attribute: &name,
                            })?;
                            attributes.insert(name, member);
                        }
                        MemberRole::Carrier(_)
                        | MemberRole::DatasetId
                        | MemberRole::InstanceId
                        | MemberRole::ObservedAt
                        | MemberRole::CreatedAt
                        | MemberRole::ModifiedAt
                        | MemberRole::DeletedAt
                        | MemberRole::UnitCode
                        | MemberRole::ValueType
                        | MemberRole::ObjectType
                        | MemberRole::Seal(_)
                        | MemberRole::OutputOnly => return Err(refuse(malformed(Some(&name), MemberShape::Root))),
                    }
                }
                ObjectKey::InvalidName(source) => return Err(refuse(EntityRefusal::ExpandedMemberName(source))),
                ObjectKey::Value | ObjectKey::Language | ObjectKey::Direction | ObjectKey::List | ObjectKey::OtherKeyword => {
                    return Err(refuse(malformed(None, MemberShape::Root)));
                }
            }
        }
        Ok(EntityDocument::new(id, types, scope_member, expires_at, attributes))
    }
}

/// The entity types `@type` names, in order.
fn entity_types(types: TypeEntries) -> Result<Vec<EntityTypeName>, EntityRefusal> {
    let TypeEntries::Array(types) = types else {
        return Err(EntityRefusal::TypeNotString);
    };
    types
        .into_iter()
        .map(|entry| entry.parse::<EntityTypeName>().map_err(|source| EntityRefusal::EntityType { source }))
        .collect()
}

/// Reads the bare attribute fragment of clauses 5.6.4 and 5.6.19: one instance,
/// as `[instance]` or `instance`.
#[derive(Clone, Copy)]
pub(crate) struct FragmentSeed<'c> {
    /// Where a refusal is recorded.
    pub(crate) context: &'c DecodeContext,
    /// The attribute the request path names.
    pub(crate) attribute: &'c AttributeName,
}

impl FragmentSeed<'_> {
    /// The instance, refusing the member-level Null a fragment cannot be.
    fn instance(self, decoded: InstanceDecoded) -> Result<InstanceDocument, EntityRefusal> {
        match decoded {
            InstanceDecoded::Instance(instance) => Ok(*instance),
            InstanceDecoded::Null => Err(EntityRefusal::ReservedNullValue {
                attribute: self.attribute.clone(),
            }),
        }
    }
}

impl<'de> DeserializeSeed<'de> for FragmentSeed<'_> {
    type Value = InstanceDocument;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<InstanceDocument, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for FragmentSeed<'_> {
    type Value = InstanceDocument;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded attribute fragment")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<InstanceDocument, A::Error> {
        let seed = InstanceSeed {
            context: self.context,
            attribute: self.attribute,
        };
        let Some(decoded) = seq.next_element_seed(seed)? else {
            return Err(self.context.refuse(malformed(Some(self.attribute), MemberShape::Attribute)));
        };
        if seq.next_element::<IgnoredAny>()?.is_some() {
            return Err(self.context.refuse(malformed(Some(self.attribute), MemberShape::Attribute)));
        }
        self.instance(decoded).map_err(|refusal| self.context.refuse(refusal))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<InstanceDocument, A::Error> {
        let decoded = InstanceSeed {
            context: self.context,
            attribute: self.attribute,
        }
        .visit_map(map)?;
        self.instance(decoded).map_err(|refusal| self.context.refuse(refusal))
    }
}
