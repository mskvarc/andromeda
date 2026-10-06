//! Expanding one attribute instance of the typed compact model.
//!
//! Each member of the typed instance is expanded exactly as the member its
//! serialization writes would be, under the key that serialization writes it
//! with: a typed member is written as the JSON text it serializes to and
//! expanded from that text, a value held as a JSON literal is expanded in
//! place, and a geometry stays the positions the transport read it into.

use crate::{
    compact_walk::{
        element::{Expanded, expand_element},
        member::{Written, expand_member, expanded_member},
        object::{KeyedEntry, NodeBuilder, property_name},
    },
    decline::Decline,
    expanded_tree::{ExpandedGeometry, ExpandedItem},
    term_scope::TermScope,
    typed_walk::merge_instance::merge_expanded,
    written_member::{WrittenMember, WrittenMemberRef},
};
use andromeda_json_literal::piece::JsonPiece;
use andromeda_ngsi_ld_geometry::{foreign_members::ForeignMembers, geometry::Geometry};
use andromeda_ngsi_ld_model::attribute::{
    common_fields::CommonAttributeFields,
    geo_property_value::GeoPropertyValue,
    instance::{Attribute, AttributeValue},
    merge_member::MergeMember,
    value::NgsiLdValue,
};
use andromeda_ngsi_ld_representation::compact_node::CompactNode;
use andromeda_ngsi_ld_vocabulary::{NULL_URI, compact::properties as compact_properties};
use jsonld::Term;
use serde::{Serialize, Serializer};
use serde_json::value::RawValue;
use std::{borrow::Cow, slice::from_ref};

/// The member of an instance that names its family.
const TYPE_MEMBER: &str = "type";

/// The concise Property clause 4.5.2.3 reads the NGSI-LD Null written in place
/// of a member as, wherever no deletion of the whole member is meant.
const NULL_PROPERTY: &str = r#"{"type":"Property","value":"urn:ngsi-ld:null"}"#;

/// The expanded name and items of the entity member written under `key` in
/// `scope`; `None` for a key expansion drops.
fn attribute_expanded(member: &AttributeValue, scope: &TermScope<'_>, key: &str) -> Result<Option<(String, Vec<ExpandedItem>)>, Decline> {
    let (instances, written) = match member {
        AttributeValue::Single(instance) => (from_ref(instance.as_ref()), Written::Single),
        AttributeValue::Multiple(instances) => (instances.as_slice(), Written::Array),
        // Below the top level the NGSI-LD Null is the value of a concise
        // Property (clause 4.5.2.3); only a whole top-level member is
        // deleted by it.
        AttributeValue::NgsiLdNull => return null_property(scope, key),
    };
    let Some(name) = member_name(scope, key)? else {
        return Ok(None);
    };
    let mut items = Vec::with_capacity(instances.len());
    for instance in instances {
        items.extend(instance_items(scope, key, instance)?);
    }
    Ok(Some((name, expanded_member(scope, key, items, written)?)))
}

/// The expanded name and items of the member written under `key` in `scope`,
/// below the top of the document or as the top-level member a write does not
/// delete; `None` for a key expansion drops.
pub(crate) fn expanded<M: WrittenMember>(member: &M, scope: &TermScope<'_>, key: &str) -> Result<Option<(String, Vec<ExpandedItem>)>, Decline> {
    match member.written() {
        WrittenMemberRef::Attribute(member) => attribute_expanded(member, scope, key),
        WrittenMemberRef::Merge(member) => merge_expanded(member, scope, key),
    }
}

/// Whether the member is the NGSI-LD Null written in place of the whole
/// member.
pub(crate) fn is_ngsi_ld_null<M: WrittenMember>(member: &M) -> bool {
    match member.written() {
        WrittenMemberRef::Attribute(member) => matches!(member, AttributeValue::NgsiLdNull),
        WrittenMemberRef::Merge(member) => matches!(member, MergeMember::NgsiLdNull),
    }
}

/// The concise Property clause 4.5.2.3 reads the NGSI-LD Null written in place
/// of the member `key` as.
pub(crate) fn null_property(scope: &TermScope<'_>, key: &str) -> Result<Option<(String, Vec<ExpandedItem>)>, Decline> {
    let raw: &RawValue = serde_json::from_str(NULL_PROPERTY).map_err(|_| Decline::CompactSerialization)?;
    let Some(name) = member_name(scope, key)? else {
        return Ok(None);
    };
    Ok(expand_member(scope, key, JsonPiece::new(raw))?.map(|items| (name, items)))
}

/// Add the member `key` of `scope` to `node`, as its expansion reads it.
pub fn add_member<M: WrittenMember>(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, member: &M) -> Result<(), Decline> {
    if let Some((name, items)) = expanded(member, scope, key)? {
        node.add_expanded(name, items);
    }
    Ok(())
}

/// Expand one instance written under `key` in `parent`.
fn instance_items(parent: &TermScope<'_>, key: &str, instance: &Attribute) -> Result<Vec<ExpandedItem>, Decline> {
    let scope = parent.object_scope(Some(key))?;
    let mut node = NodeBuilder::default();
    add_serialized(&mut node, &scope, TYPE_MEMBER, instance.attribute_type().short_name())?;
    family_members(&mut node, &scope, instance)?;
    common_members(&mut node, &scope, instance.common())?;
    Ok(node.finish(Some(key)).into_items())
}

/// The members of the instance's own family. The `previous...` member is the
/// output-only one clause 5.2.5 and its siblings ignore on writes, so it is
/// never expanded.
fn family_members(node: &mut NodeBuilder, scope: &TermScope<'_>, instance: &Attribute) -> Result<(), Decline> {
    match instance {
        Attribute::Property(property) => {
            add_value(node, scope, compact_properties::VALUE, &property.value)?;
            add_optional(node, scope, compact_properties::UNIT_CODE, property.unit_code.as_ref())?;
            add_optional(node, scope, compact_properties::VALUE_TYPE, property.value_type.as_ref())?;
            // Clause 4.5.2.2: the seals of `ngsildproof` are non-reified, each
            // written as the value it is.
            if let Some(seal) = &property.entity_id_sealed {
                add_json(node, scope, compact_properties::ENTITY_ID_SEALED, seal.piece())?;
            }
            if let Some(seal) = &property.entity_type_sealed {
                add_json(node, scope, compact_properties::ENTITY_TYPE_SEALED, seal.piece())?;
            }
            Ok(())
        }
        Attribute::Relationship(relationship) => {
            if relationship.entity.is_some() {
                return Err(Decline::OutputOnlyMember);
            }
            add_serialized(node, scope, compact_properties::OBJECT, &relationship.object)?;
            add_optional(node, scope, compact_properties::OBJECT_TYPE, relationship.object_type.as_ref())
        }
        Attribute::GeoProperty(geo_property) => {
            add_geo_value(node, scope, compact_properties::VALUE, &geo_property.value)?;
            add_optional(node, scope, compact_properties::UNIT_CODE, geo_property.unit_code.as_ref())?;
            add_optional(node, scope, compact_properties::VALUE_TYPE, geo_property.value_type.as_ref())
        }
        Attribute::VocabProperty(vocab_property) => {
            add_serialized(node, scope, compact_properties::VOCAB, &vocab_property.vocab)?;
            add_optional(node, scope, compact_properties::VALUE_TYPE, vocab_property.value_type.as_ref())
        }
        Attribute::LanguageProperty(language_property) => {
            add_serialized(node, scope, compact_properties::LANGUAGE_MAP, &language_property.language_map)?;
            add_optional(node, scope, compact_properties::VALUE_TYPE, language_property.value_type.as_ref())
        }
        Attribute::JsonProperty(json_property) => {
            add_json(node, scope, compact_properties::JSON, json_property.json.piece())?;
            add_optional(node, scope, compact_properties::VALUE_TYPE, json_property.value_type.as_ref())
        }
        Attribute::ListProperty(list_property) => {
            add_value_list(node, scope, &list_property.value_list)?;
            add_optional(node, scope, compact_properties::UNIT_CODE, list_property.unit_code.as_ref())?;
            add_optional(node, scope, compact_properties::VALUE_TYPE, list_property.value_type.as_ref())
        }
        Attribute::ListRelationship(list_relationship) => {
            if list_relationship.entity_list.is_some() {
                return Err(Decline::OutputOnlyMember);
            }
            add_serialized(node, scope, compact_properties::OBJECT_LIST, &list_relationship.object_list)?;
            add_optional(node, scope, compact_properties::OBJECT_TYPE, list_relationship.object_type.as_ref())
        }
    }
}

/// The members every family carries, sub-attributes included.
fn common_members(node: &mut NodeBuilder, scope: &TermScope<'_>, common: &CommonAttributeFields) -> Result<(), Decline> {
    add_optional(node, scope, compact_properties::CREATED_AT, common.created_at.as_ref())?;
    add_optional(node, scope, compact_properties::DATASET_ID, common.dataset_id.as_ref())?;
    add_optional(node, scope, compact_properties::DELETED_AT, common.deleted_at.as_ref())?;
    add_optional(node, scope, compact_properties::EXPIRES_AT, common.expires_at.as_ref())?;
    add_optional(node, scope, compact_properties::INSTANCE_ID, common.instance_id.as_ref())?;
    add_optional(node, scope, compact_properties::MODIFIED_AT, common.modified_at.as_ref())?;
    add_optional(node, scope, compact_properties::OBSERVED_AT, common.observed_at.as_ref())?;
    for (name, member) in &common.sub_attributes {
        add_member(node, scope, name.as_str(), member)?;
    }
    Ok(())
}

/// A Property `value`: the JSON as written, or the geometry already read.
pub(crate) fn add_value(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: &NgsiLdValue) -> Result<(), Decline> {
    match value {
        NgsiLdValue::Json(json) => add_json(node, scope, key, json.piece()),
        NgsiLdValue::Geometry(geometry) => add_geometry(node, scope, key, geometry),
    }
}

/// A `GeoProperty` `value`: the geometry already read, the clause 4.7.2 string
/// as written, or the NGSI-LD Null.
pub(crate) fn add_geo_value(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: &GeoPropertyValue) -> Result<(), Decline> {
    match value {
        GeoPropertyValue::Geometry(geometry) => add_geometry(node, scope, key, geometry),
        GeoPropertyValue::Encoded(encoded) => add_serialized(node, scope, key, encoded.as_str()),
        GeoPropertyValue::Null => add_serialized(node, scope, key, NULL_URI.as_str()),
    }
}

/// A `ListProperty` `valueList`, item by item, as the array it serializes to.
pub(crate) fn add_value_list(node: &mut NodeBuilder, scope: &TermScope<'_>, values: &[NgsiLdValue]) -> Result<(), Decline> {
    let key = compact_properties::VALUE_LIST;
    let Some(name) = member_name(scope, key)? else {
        return Ok(());
    };
    let mut items = Vec::with_capacity(values.len());
    for value in values {
        match value {
            NgsiLdValue::Json(json) => items.extend(expand_element(scope, Some(key), json.piece())?.into_items()),
            NgsiLdValue::Geometry(geometry) => items.push(geometry_item(scope, key, geometry)?),
        }
    }
    node.add_expanded(name, expanded_member(scope, key, items, Written::Array)?);
    Ok(())
}

/// A geometry the transport already read into its positions.
fn add_geometry(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, geometry: &Geometry) -> Result<(), Decline> {
    let Some(name) = member_name(scope, key)? else {
        return Ok(());
    };
    // A geometry written under a term with a scoped context would be
    // expanded under that context; the core GeoJSON terms it holds are
    // protected, but the walk does not assume so.
    if scope.has_scoped_context(key) {
        return Err(Decline::NestedScopedContext);
    }
    let items = vec![geometry_item(scope, key, geometry)?];
    node.add_expanded(name, expanded_member(scope, key, items, Written::Single)?);
    Ok(())
}

/// The item a geometry the transport already read stands for: its positions,
/// its `bbox`, and its foreign members (IETF RFC 7946 sections 5 and 6.1)
/// expanded as the members of the object written under `key`, as the processor
/// expands every member of a geometry object it reads.
fn geometry_item(scope: &TermScope<'_>, key: &str, geometry: &Geometry) -> Result<ExpandedItem, Decline> {
    let mut expanded = ExpandedGeometry::bare(geometry.form().clone()); // clone: the typed request is borrowed
    if let Some(members) = geometry.members() {
        expanded.bbox = members.bbox().copied();
        if !members.foreign().is_empty() {
            let text = written_text(&ForeignObject(members.foreign()))?;
            match expand_element(scope, Some(key), JsonPiece::new(&text))? {
                Expanded::One(ExpandedItem::Node(node)) if node.id.is_none() && node.types.is_empty() => expanded.foreign = node.members,
                Expanded::Null => {}
                Expanded::One(ExpandedItem::Node(_) | ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_))
                | Expanded::Many(_) => return Err(Decline::MalformedJsonLd),
            }
        }
    }
    Ok(ExpandedItem::Geometry(Box::new(expanded)))
}

/// The foreign members of a geometry, written as the members of one object.
struct ForeignObject<'a>(&'a ForeignMembers);

impl Serialize for ForeignObject<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(name, value)| (name.as_ref(), value)))
    }
}

/// The member name `key` expands to, `None` for a key expansion drops.
pub(crate) fn member_name(scope: &TermScope<'_>, key: &str) -> Result<Option<String>, Decline> {
    match scope.expand_key(key)?.as_ref() {
        Term::Id(property) => Ok(property_name(property)),
        Term::Null => Ok(None),
        Term::Keyword(_) => Err(Decline::MalformedJsonLd),
    }
}

/// A typed member written when present.
pub fn add_optional<T: Serialize + ?Sized>(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: Option<&T>) -> Result<(), Decline> {
    value.map_or(Ok(()), |value| add_serialized(node, scope, key, value))
}

/// A typed member, expanded as the JSON text it serializes to.
pub fn add_serialized<T: Serialize + ?Sized>(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: &T) -> Result<(), Decline> {
    let text = written_text(value)?;
    add_json(node, scope, key, JsonPiece::new(&text))
}

/// The JSON text a typed member serializes to.
fn written_text<T: Serialize + ?Sized>(value: &T) -> Result<Box<RawValue>, Decline> {
    serde_json::to_string(value)
        .and_then(RawValue::from_string)
        .map_err(|_| Decline::CompactSerialization)
}

/// A member held as JSON, expanded in place.
pub fn add_json<'n, N: CompactNode<'n>>(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: N) -> Result<(), Decline> {
    node.add_json(
        scope,
        &KeyedEntry {
            key: Cow::Borrowed(key),
            term: scope.expand_key(key)?,
            value,
        },
    )
}
