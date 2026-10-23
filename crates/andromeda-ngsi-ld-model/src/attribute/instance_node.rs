//! Reading one attribute instance of a known family from the members of a
//! compact node.
//!
//! The instance is the family its `type` names or its value member implies,
//! built from the members [`InstanceMembers`] sorted out for that family. A
//! member the family requires and does not find, or a member that does not
//! decode as what the family holds in it, refuses the instance, located at the
//! member.

use crate::{
    attribute::{
        common_fields::CommonAttributeFields,
        geo_property::GeoProperty,
        geo_property_value::{GeoPropertyValue, GeoValueRefusal},
        inline_entity::InlineEntity,
        instance::Attribute,
        instance_member::{InstanceMembers, ReadError, optional_member, refused_member, unit_code_member},
        json_property::JsonProperty,
        language_property::LanguageProperty,
        list_property::ListProperty,
        list_relationship::ListRelationship,
        one_or_many::OneOrMany,
        property::Property,
        relationship::Relationship,
        value::{NgsiLdValue, ValueRefusal},
        vocab_property::VocabProperty,
    },
    compact_member::decoded,
    entity::Entity,
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeMembers, NodeShape};
use serde::de::DeserializeOwned;

impl Attribute {
    /// The instance of `family` the members of a compact object written
    /// under the attribute name `name` are.
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] when the members are no instance of
    /// `family`, located at the member at fault, and
    /// [`NodeReadError::Unreadable`] when a node of them has no JSON form.
    pub(crate) fn from_members<'n, N: CompactNode<'n>>(name: &str, family: AttributeType, members: NodeMembers<'n, N>) -> Result<Attribute, ReadError<'n, N>> {
        let members = InstanceMembers::sorted(name, family, members)?;
        let value_member = RefusedMember::Value(family);
        let previous_member = RefusedMember::Previous(family);
        let value = members
            .value
            .ok_or_else(|| ReadError::<N>::from(NodeRefusal::Missing { member: value_member }))
            .and_then(|value| required(value, value_member));
        let common = members.common()?;
        let value_type = optional_member(members.value_type, RefusedMember::ValueType)?;
        let unit_code = unit_code_member(name, members.unit_code)?;
        Ok(match family {
            AttributeType::Property => Attribute::Property(Property {
                value: ngsi_ld_value(value?, value_member)?,
                unit_code,
                value_type,
                previous_value: optional_present(members.previous, previous_member, |previous| ngsi_ld_value(previous, previous_member))?,
                entity_id_sealed: optional_present(members.entity_id_sealed, RefusedMember::EntityIdSealed, |seal| {
                    json(seal, RefusedMember::EntityIdSealed)
                })?,
                entity_type_sealed: optional_present(members.entity_type_sealed, RefusedMember::EntityTypeSealed, |seal| {
                    json(seal, RefusedMember::EntityTypeSealed)
                })?,
                common,
            }),
            AttributeType::Relationship => Attribute::Relationship(Relationship {
                object: member(value?, value_member)?,
                object_type: optional_member(members.object_type, RefusedMember::ObjectType)?,
                previous_object: optional_member(members.previous, previous_member)?,
                entity: optional_present(members.inline, RefusedMember::Entity, |inline| inline_entities(inline, RefusedMember::Entity))?,
                common,
            }),
            AttributeType::VocabProperty => Attribute::VocabProperty(VocabProperty {
                vocab: member(value?, value_member)?,
                value_type,
                previous_vocab: optional_member(members.previous, previous_member)?,
                common,
            }),
            AttributeType::GeoProperty => Attribute::GeoProperty(GeoProperty {
                value: geo_value(value?, value_member)?,
                unit_code,
                value_type,
                previous_value: optional_present(members.previous, previous_member, |previous| geo_value(previous, previous_member))?,
                common,
            }),
            AttributeType::LanguageProperty => Attribute::LanguageProperty(LanguageProperty {
                language_map: member(value?, value_member)?,
                value_type,
                previous_language_map: optional_member(members.previous, previous_member)?,
                common,
            }),
            AttributeType::JsonProperty => Attribute::JsonProperty(JsonProperty {
                json: json(value?, value_member)?,
                value_type,
                previous_json: optional_present(members.previous, previous_member, |previous| json(previous, previous_member))?,
                common,
            }),
            AttributeType::ListProperty => Attribute::ListProperty(ListProperty {
                value_list: value_list(value?, value_member)?,
                unit_code,
                value_type,
                previous_value_list: optional_present(members.previous, previous_member, |previous| value_list(previous, previous_member))?,
                common,
            }),
            AttributeType::ListRelationship => Attribute::ListRelationship(ListRelationship {
                object_list: member(value?, value_member)?,
                object_type: optional_member(members.object_type, RefusedMember::ObjectType)?,
                entity_list: optional_present(members.inline, RefusedMember::EntityList, |inline| {
                    inline_entities(inline, RefusedMember::EntityList)
                })?,
                previous_object_list: optional_member(members.previous, previous_member)?,
                common,
            }),
        })
    }

    /// The concise Property a bare value written where an attribute stands
    /// abbreviates (clause 4.5.2.3).
    ///
    /// # Errors
    ///
    /// Returns the refusal of the value and the node's
    /// [`CompactNode::Unreadable`].
    pub(crate) fn concise_property<'n, N: CompactNode<'n>>(value: N) -> Result<Attribute, ReadError<'n, N>> {
        let member = RefusedMember::Value(AttributeType::Property);
        let value = NgsiLdValue::from_node(value)
            .map_err(NodeReadError::Unreadable)?
            .map_err(|refusal| ReadError::<N>::from(value_refusal(member, refusal)))?;
        Ok(Attribute::Property(Property {
            value,
            unit_code: None,
            value_type: None,
            previous_value: None,
            entity_id_sealed: None,
            entity_type_sealed: None,
            common: CommonAttributeFields::default(),
        }))
    }

    /// The concise `GeoProperty` a bare geometry written where an attribute
    /// stands abbreviates (clause 4.7.3).
    ///
    /// # Errors
    ///
    /// Returns the refusal of the geometry and the node's
    /// [`CompactNode::Unreadable`].
    pub(crate) fn concise_geo_property<'n, N: CompactNode<'n>>(geometry: N) -> Result<Attribute, ReadError<'n, N>> {
        let member = RefusedMember::Value(AttributeType::GeoProperty);
        let value = GeoPropertyValue::from_node(geometry)
            .map_err(NodeReadError::Unreadable)?
            .map_err(|refusal| ReadError::<N>::from(geo_refusal(member, refusal)))?;
        Ok(Attribute::GeoProperty(GeoProperty {
            value,
            unit_code: None,
            value_type: None,
            previous_value: None,
            common: CommonAttributeFields::default(),
        }))
    }
}

/// A required member: present, and not JSON `null` (clause 5.2.1).
pub(crate) fn required<'n, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<N, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Null => Err(refused_member::<N>(name, NodeRefusal::Null)),
        NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => Ok(node),
    }
}

/// Decode a required member as a `T`.
pub(crate) fn member<'n, T: DeserializeOwned, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<T, ReadError<'n, N>> {
    decoded(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| refused_member::<N>(name, NodeRefusal::Member { member: name, source }))
}

/// Read a member named `name` that may be absent with `read`; a JSON `null`
/// is no value of any member (clause 5.2.1).
pub(crate) fn optional_present<'n, T, N: CompactNode<'n>>(
    node: Option<N>,
    name: RefusedMember,
    read: impl FnOnce(N) -> Result<T, ReadError<'n, N>>,
) -> Result<Option<T>, ReadError<'n, N>> {
    node.map(|node| required(node, name).and_then(read)).transpose()
}

/// The refusal of a value the member `member` holds.
pub(crate) fn value_refusal(member: RefusedMember, refusal: ValueRefusal) -> NodeRefusal {
    match refusal {
        ValueRefusal::Geometry(source) => NodeRefusal::Value { member, source },
        ValueRefusal::Written(source) => NodeRefusal::Member { member, source },
        ValueRefusal::Literal(source) => NodeRefusal::Literal { member, source },
    }
}

/// The refusal of a geometry the member `member` holds.
pub(crate) fn geo_refusal(member: RefusedMember, refusal: GeoValueRefusal) -> NodeRefusal {
    match refusal {
        GeoValueRefusal::Typed(source) => NodeRefusal::Geometry { member, source },
        GeoValueRefusal::Written(source) => NodeRefusal::Member { member, source },
        GeoValueRefusal::Geometry(source) => NodeRefusal::Value { member, source },
    }
}

/// A Property's value, or one item of a `ListProperty` value.
pub(crate) fn ngsi_ld_value<'n, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<NgsiLdValue, ReadError<'n, N>> {
    NgsiLdValue::from_node(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|refusal| refused_member::<N>(name, value_refusal(name, refusal)))
}

/// A `GeoProperty` value.
pub(crate) fn geo_value<'n, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<GeoPropertyValue, ReadError<'n, N>> {
    GeoPropertyValue::from_node(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|refusal| refused_member::<N>(name, geo_refusal(name, refusal)))
}

/// A `ListProperty` value list: an array of values.
pub(crate) fn value_list<'n, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<Vec<NgsiLdValue>, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                NgsiLdValue::from_node(item).map_err(NodeReadError::Unreadable)?.map_err(|refusal| {
                    ReadError::<N>::from(value_refusal(name, refusal))
                        .within_element(index)
                        .within_member(&name.to_string())
                })
            })
            .collect(),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            Err(refused_member::<N>(name, NodeRefusal::NotAnArray { member: name }))
        }
    }
}

/// A `JsonProperty` literal: the JSON value as written.
pub(crate) fn json<'n, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<JsonLiteral, ReadError<'n, N>> {
    node.literal()
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| refused_member::<N>(name, NodeRefusal::Literal { member: name, source }))
}

/// The entities a clause 4.5.23 join wrote inline: one entity or an array of
/// them, each read as an entity is.
fn inline_entities<'n, N: CompactNode<'n>>(node: N, name: RefusedMember) -> Result<InlineEntity, ReadError<'n, N>> {
    let within = |error: ReadError<'n, N>| error.within_member(&name.to_string());
    match node.shape() {
        NodeShape::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(index, item)| <Entity>::from_node(item).map_err(|error| within(error.within_element(index))))
            .collect::<Result<Vec<_>, _>>()
            .map(OneOrMany::Many),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            <Entity>::from_node(node).map(OneOrMany::One).map_err(within)
        }
    }
}
