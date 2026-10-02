//! Reading one attribute instance of any family from a compact node.
//!
//! The instance is the family its `type` names, built from the members
//! [`InstanceMembers`] sorted out for that family. A member the family requires
//! and does not find, or a member that does not decode as what the family
//! holds in it, refuses the instance, and the attribute member holding it is
//! then carried verbatim (see [`AttributeValue::from_node`]).
//!
//! [`AttributeValue::from_node`]: crate::attribute::instance::AttributeValue::from_node

use crate::{
    attribute::{
        geo_property::{GeoProperty, TYPE_MEMBER, typed_geometry},
        instance::Attribute,
        instance_member::{InstanceMembers, optional_member},
        json_property::JsonProperty,
        language_property::LanguageProperty,
        list_property::ListProperty,
        list_relationship::ListRelationship,
        property::Property,
        relationship::Relationship,
        value::NgsiLdValue,
        vocab_property::VocabProperty,
    },
    compact_member::decoded,
    compact_node::{CompactNode, NodeShape},
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The failure of reading a member of a node of type `N`.
type ReadError<N> = NodeReadError<<N as CompactNode>::Unreadable>;

impl Attribute {
    /// The attribute instance a compact node is.
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] when the node is no instance of the
    /// family it names, and [`NodeReadError::Unreadable`] when a node of it has
    /// no JSON form.
    pub fn from_node<N: CompactNode>(node: &N) -> Result<Attribute, ReadError<N>> {
        let NodeShape::Object(members) = node.shape() else {
            return Err(NodeRefusal::NotAnObject.into());
        };
        let family = family_of(node).ok_or(NodeRefusal::NoFamily)?;
        let members = InstanceMembers::sorted(family, members);
        let value_member = RefusedMember::Value(family);
        let previous_member = RefusedMember::Previous(family);
        let value = members.value.ok_or(NodeRefusal::Missing { member: value_member });
        let common = members.common()?;
        let value_type = optional_member(members.value_type, RefusedMember::ValueType)?;
        Ok(match family {
            AttributeType::Property => Attribute::Property(Property {
                value: ngsi_ld_value(value?, value_member)?,
                unit_code: optional_member(members.unit_code, RefusedMember::UnitCode)?,
                value_type,
                previous_value: optional_present(members.previous, |previous| ngsi_ld_value(previous, previous_member))?,
                common,
            }),
            AttributeType::Relationship => Attribute::Relationship(Relationship {
                object: member(value?, value_member)?,
                object_type: optional_member(members.object_type, RefusedMember::ObjectType)?,
                previous_object: optional_member(members.previous, previous_member)?,
                entity: optional_member(members.inline, RefusedMember::Entity)?,
                common,
            }),
            AttributeType::VocabProperty => Attribute::VocabProperty(VocabProperty {
                vocab: member(value?, value_member)?,
                value_type,
                previous_vocab: optional_member(members.previous, previous_member)?,
                common,
            }),
            AttributeType::GeoProperty => Attribute::GeoProperty(GeoProperty {
                value: geometry(value?, value_member)?,
                value_type,
                previous_value: optional_present(members.previous, |previous| geometry(previous, previous_member))?,
                common,
            }),
            AttributeType::LanguageProperty => Attribute::LanguageProperty(LanguageProperty {
                language_map: member(value?, value_member)?,
                value_type,
                previous_language_map: optional_member(members.previous, previous_member)?,
                common,
            }),
            AttributeType::JsonProperty => Attribute::JsonProperty(JsonProperty {
                json: json(value?)?,
                value_type,
                previous_json: optional_present(members.previous, json)?,
                common,
            }),
            AttributeType::ListProperty => Attribute::ListProperty(ListProperty {
                value_list: value_list(value?, value_member)?,
                value_type,
                previous_value_list: optional_present(members.previous, |previous| value_list(previous, previous_member))?,
                common,
            }),
            AttributeType::ListRelationship => Attribute::ListRelationship(ListRelationship {
                object_list: member(value?, value_member)?,
                object_type: optional_member(members.object_type, RefusedMember::ObjectType)?,
                entity_list: optional_member(members.inline, RefusedMember::EntityList)?,
                previous_object_list: optional_member(members.previous, previous_member)?,
                common,
            }),
        })
    }
}

/// The family an instance names in its `type` member.
fn family_of<N: CompactNode>(node: &N) -> Option<AttributeType> {
    let NodeShape::String(tag) = node.member(TYPE_MEMBER)?.shape() else {
        return None;
    };
    AttributeType::ALL.into_iter().find(|family| family.short_name() == tag)
}

/// Decode a required member as a `T`.
fn member<T: DeserializeOwned, N: CompactNode>(node: &N, name: RefusedMember) -> Result<T, ReadError<N>> {
    decoded(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source }.into())
}

/// Read a member that may be absent or `null` with `read`.
fn optional_present<T, N: CompactNode>(node: Option<&N>, read: impl FnOnce(&N) -> Result<T, ReadError<N>>) -> Result<Option<T>, ReadError<N>> {
    match node.map(|node| (node, node.shape())) {
        None | Some((_, NodeShape::Null)) => Ok(None),
        Some((node, NodeShape::Boolean | NodeShape::Number | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_))) => {
            read(node).map(Some)
        }
    }
}

/// A Property's value, or one item of a `ListProperty` value.
fn ngsi_ld_value<N: CompactNode>(node: &N, name: RefusedMember) -> Result<NgsiLdValue, ReadError<N>> {
    NgsiLdValue::from_node(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Value { member: name, source }.into())
}

/// A `ListProperty` value list: an array of values.
fn value_list<N: CompactNode>(node: &N, name: RefusedMember) -> Result<Vec<NgsiLdValue>, ReadError<N>> {
    match node.shape() {
        NodeShape::Array(items) => items.iter().map(|item| ngsi_ld_value(item, name)).collect(),
        NodeShape::Null | NodeShape::Boolean | NodeShape::Number | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            Err(NodeRefusal::NotAnArray { member: name }.into())
        }
    }
}

/// A `JsonProperty` literal: the JSON value as written.
fn json<N: CompactNode>(node: &N) -> Result<Value, ReadError<N>> {
    node.to_json().map_err(NodeReadError::Unreadable)
}

/// A `GeoProperty` geometry: the typed geometry a node holds, or the
/// written `GeoJSON` object read as one.
fn geometry<N: CompactNode>(node: &N, name: RefusedMember) -> Result<Geometry, ReadError<N>> {
    if let NodeShape::Geometry(form) = node.shape() {
        // clone: the compact entity owns its geometry, the tree keeps the one it was written from
        return Ok(Geometry::new(form.clone()));
    }
    let written = node
        .read_geometry()
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member: name, source })?;
    typed_geometry(written).map_err(|source| NodeRefusal::Geometry { member: name, source }.into())
}
