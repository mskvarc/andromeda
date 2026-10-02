//! Turning an expanded tree into the written document (clauses 5.5.8,
//! 5.5.12, 5.6.x).
//!
//! The one place both expansion paths meet: whichever produced the tree, the
//! document is read from it by these rules, so a typed expansion and the
//! processor's can differ only where their trees do.

pub mod expanded_value;
pub mod geometry_node;
pub mod instance;
pub mod member_value;
pub mod null_sentinel;
pub mod written_value;

use crate::{
    assembly::{
        instance::instance_document,
        member_value::{malformed, null_beside_data},
        null_sentinel::{is_null, is_sole_null},
    },
    attribute_fragment::AttributeFragment,
    expanded_tree::{ExpandedItem, ExpandedNode},
};
use andromeda_ngsi_ld_entity::{
    date_time_text::DateTimeText,
    document::{EntityDocument, attribute_member::AttributeMember, slot::Slot},
    member_role::MemberRole,
    member_shape::MemberShape,
    refusal::EntityRefusal,
    scope_slot::{ScopeItem, scope_slot},
    scopes::Scopes,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use indexmap::IndexMap;

/// Read the expanded entity node of a write; no node at all is the empty
/// document (a fragment that names nothing).
///
/// # Errors
///
/// Returns the refusal of the first member that is not what its expanded IRI
/// names.
pub fn entity_document(node: Option<ExpandedNode>) -> Result<EntityDocument, EntityRefusal> {
    let Some(node) = node else {
        return Ok(EntityDocument::default());
    };
    let id = node
        .id
        .map(|id| id.parse::<Identifier>().map_err(|source| EntityRefusal::EntityId { source }))
        .transpose()?;
    let types = node
        .types
        .iter()
        .map(|type_name| type_name.parse::<EntityTypeName>().map_err(|source| EntityRefusal::EntityType { source }))
        .collect::<Result<Vec<_>, _>>()?;
    let mut scope = None;
    let mut expires_at = None;
    let mut attributes = IndexMap::with_capacity(node.members.len());
    for (name, items) in node.members {
        let iri = name.as_str();
        if iri == properties::SCOPE.as_str() {
            scope = Some(scope_member(&items)?);
        } else if iri == properties::EXPIRES_AT.as_str() {
            expires_at = Some(entity_expiry(&items)?);
        } else if [properties::CREATED_AT, properties::MODIFIED_AT, properties::DELETED_AT]
            .iter()
            .any(|system| system.as_str() == iri)
        {
            // Clause 4.8: the entity's own system times are the broker's, and
            // a write that carries them carries nothing the entity keeps.
        } else {
            let name = name.parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)?;
            if MemberRole::is_reserved(name.as_str()) {
                return Err(EntityRefusal::ReservedAttributeName { attribute: name });
            }
            let member = attribute_member(&name, items)?;
            attributes.insert(name, member);
        }
    }
    Ok(EntityDocument::new(id, types, scope, expires_at, attributes))
}

/// Read the expanded attribute fragment of clauses 5.6.4 and 5.6.19: the one
/// instance written under `name`.
///
/// # Errors
///
/// Returns [`EntityRefusal::Malformed`] when the fragment is not one instance,
/// and the refusal of the instance's first malformed member.
pub fn attribute_fragment(name: &str, items: Vec<ExpandedItem>) -> Result<AttributeFragment, EntityRefusal> {
    let name = name.parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)?;
    let mut items = items.into_iter();
    let (Some(ExpandedItem::Node(node)), None) = (items.next(), items.next()) else {
        return Err(malformed(&name, MemberShape::Attribute));
    };
    let instance = instance_document(&name, node)?;
    Ok(AttributeFragment::new(name, instance))
}

/// Read one attribute member: its instances, or its removal by a member that
/// is the NGSI-LD Null alone (clause 5.5.12).
///
/// # Errors
///
/// Returns [`EntityRefusal::NullBesideData`] for a Null beside instances,
/// [`EntityRefusal::Malformed`] for a value that is no instance, and the
/// refusal of an instance's first malformed member.
pub fn attribute_member(name: &AttributeName, items: Vec<ExpandedItem>) -> Result<AttributeMember, EntityRefusal> {
    if is_sole_null(&items) {
        return Ok(AttributeMember::Remove);
    }
    if items.iter().any(is_null) {
        return Err(null_beside_data(name));
    }
    items
        .into_iter()
        .map(|item| match item {
            ExpandedItem::Node(node) => instance_document(name, node),
            ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => Err(malformed(name, MemberShape::Attribute)),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(AttributeMember::Instances)
}

/// Read the `scope` member (clause 4.18): its paths, or its deletion by the
/// NGSI-LD Null alone.
fn scope_member(items: &[ExpandedItem]) -> Result<Slot<Scopes>, EntityRefusal> {
    scope_slot(items.iter().map(|item| match item {
        item if is_null(item) => ScopeItem::Null,
        ExpandedItem::Value(value) => value.as_str().map_or(ScopeItem::NotAString, ScopeItem::Text),
        ExpandedItem::Node(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_) => ScopeItem::NotAString,
    }))
    .map_err(EntityRefusal::Scope)
}

/// Read the entity-level `expiresAt` (clause 4.22): its date-time, or its
/// deletion by the NGSI-LD Null alone.
fn entity_expiry(items: &[ExpandedItem]) -> Result<Slot<DateTimeText>, EntityRefusal> {
    if is_sole_null(items) {
        return Ok(Slot::Remove);
    }
    match items {
        [ExpandedItem::Value(value)] => value
            .as_str()
            .and_then(|text| text.parse::<DateTimeText>().ok())
            .map(Slot::Set)
            .ok_or(EntityRefusal::EntityExpiresAt),
        [] | [_, ..] => Err(EntityRefusal::Malformed {
            attribute: None,
            member: MemberShape::EntityExpiresAt,
        }),
    }
}
