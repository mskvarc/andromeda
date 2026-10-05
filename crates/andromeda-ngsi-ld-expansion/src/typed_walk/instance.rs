//! Expanding one attribute instance of the typed compact model.
//!
//! Each member of the typed instance is expanded exactly as the member its
//! serialization writes would be, under the key that serialization writes it
//! with: a scalar or an array of strings is serialized and expanded as JSON, a
//! value already held as JSON is expanded in place, and a geometry stays the
//! positions the transport read it into. An instance holding a member the
//! typed model could not read (`AttributeValue::Raw`) among its
//! sub-attributes is serialized whole and expanded as JSON, after the concise
//! rewrite that member needs.

use crate::{
    compact_walk::{
        element::{Expanded, expand_element},
        member::{Written, expanded_member},
        object::{KeyedEntry, NodeBuilder, property_name},
    },
    decline::Decline,
    expanded_tree::{ExpandedGeometry, ExpandedItem},
    term_scope::TermScope,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_model::attribute::{
    common_fields::CommonAttributeFields,
    instance::{Attribute, AttributeValue},
    value::NgsiLdValue,
};
use andromeda_ngsi_ld_representation::normalization::normalize_attribute_node;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use jsonld::Term;
use serde::Serialize;
use serde_json::{Map, Value};
use std::slice::from_ref;

/// The member of an instance that names its family.
const TYPE_MEMBER: &str = "type";

/// Expand the instances of the attribute member `key` written in `scope`.
///
/// # Errors
///
/// Declines whatever expanding one of the instances declines.
pub fn attribute_member(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, member: &AttributeValue) -> Result<(), Decline> {
    let (instances, written) = match member {
        AttributeValue::Single(instance) => (from_ref(instance.as_ref()), Written::Single),
        AttributeValue::Multiple(instances) if !instances.is_empty() => (instances.as_slice(), Written::Array),
        // An empty array and a member the typed model could not read are not
        // instances; their concise rewrite is the caller's.
        AttributeValue::Multiple(_) | AttributeValue::Raw(_) => return Err(Decline::CompactSerialization),
    };
    let term = scope.expand_key(key)?;
    let name = match term.as_ref() {
        Term::Id(property) => property_name(property),
        Term::Null => None,
        Term::Keyword(_) => return Err(Decline::MalformedJsonLd),
    };
    let Some(name) = name else {
        return Ok(());
    };
    let mut items = Vec::with_capacity(instances.len());
    for instance in instances {
        items.extend(instance_items(scope, key, instance)?);
    }
    node.add_expanded(name, expanded_member(scope, key, items, written)?);
    Ok(())
}

/// Expand one instance written under `key` in `parent`.
fn instance_items(parent: &TermScope<'_>, key: &str, instance: &Attribute) -> Result<Vec<ExpandedItem>, Decline> {
    if instance.common().sub_attributes.values().any(needs_concise_rewrite) {
        let mut json = serde_json::to_value(instance).map_err(|_| Decline::CompactSerialization)?;
        normalize_attribute_node(key, &mut json);
        return Ok(expand_element(parent, Some(key), &json)?.into_items());
    }
    let scope = parent.object_scope(Some(key))?;
    let mut node = NodeBuilder::default();
    add_json(
        &mut node,
        &scope,
        TYPE_MEMBER,
        &Value::String(instance.attribute_type().short_name().to_owned()),
    )?;
    family_members(&mut node, &scope, instance)?;
    common_members(&mut node, &scope, instance.common())?;
    Ok(node.finish(Some(key)).into_items())
}

/// Whether a sub-attribute is no typed instance, so the instance it belongs
/// to goes through the concise rewrite.
const fn needs_concise_rewrite(member: &AttributeValue) -> bool {
    match member {
        AttributeValue::Raw(_) => true,
        AttributeValue::Multiple(instances) => instances.is_empty(),
        AttributeValue::Single(_) => false,
    }
}

/// The members of the instance's own family. The `previous...` member is the
/// output-only one clause 5.2.5 and its siblings ignore on writes, so it is
/// never expanded.
fn family_members(node: &mut NodeBuilder, scope: &TermScope<'_>, instance: &Attribute) -> Result<(), Decline> {
    match instance {
        Attribute::Property(property) => {
            add_value(node, scope, compact_properties::VALUE, &property.value)?;
            add_optional(node, scope, compact_properties::UNIT_CODE, property.unit_code.as_ref())?;
            add_optional(node, scope, compact_properties::VALUE_TYPE, property.value_type.as_ref())
        }
        Attribute::Relationship(relationship) => {
            if relationship.entity.is_some() {
                return Err(Decline::OutputOnlyMember);
            }
            add_serialized(node, scope, compact_properties::OBJECT, &relationship.object)?;
            add_optional(node, scope, compact_properties::OBJECT_TYPE, relationship.object_type.as_ref())
        }
        Attribute::GeoProperty(geo_property) => {
            add_geometry(node, scope, compact_properties::VALUE, &geo_property.value)?;
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
            add_serialized(node, scope, compact_properties::JSON, &json_property.json)?;
            add_optional(node, scope, compact_properties::VALUE_TYPE, json_property.value_type.as_ref())
        }
        Attribute::ListProperty(list_property) => {
            add_value_list(node, scope, &list_property.value_list)?;
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
        attribute_member(node, scope, name.as_str(), member)?;
    }
    Ok(())
}

/// A Property `value`: the JSON as written, or the geometry already read.
fn add_value(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: &NgsiLdValue) -> Result<(), Decline> {
    match value {
        NgsiLdValue::Json(json) => add_json(node, scope, key, json),
        NgsiLdValue::Geometry(geometry) => add_geometry(node, scope, key, geometry),
    }
}

/// A `ListProperty` `valueList`, item by item, as the array it serializes to.
fn add_value_list(node: &mut NodeBuilder, scope: &TermScope<'_>, values: &[NgsiLdValue]) -> Result<(), Decline> {
    let key = compact_properties::VALUE_LIST;
    let Some(name) = member_name(scope, key)? else {
        return Ok(());
    };
    let mut items = Vec::with_capacity(values.len());
    for value in values {
        match value {
            NgsiLdValue::Json(json) => items.extend(expand_element(scope, Some(key), json)?.into_items()),
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
            let object = members
                .foreign()
                .iter()
                .map(|(name, value)| serde_json::from_str(value.text()).map(|tree| (name.to_string(), tree)))
                .collect::<Result<Map<String, Value>, _>>()
                .map_err(|_| Decline::CompactSerialization)?;
            match expand_element(scope, Some(key), &Value::Object(object))? {
                Expanded::One(ExpandedItem::Node(node)) if node.id.is_none() && node.types.is_empty() => expanded.foreign = node.members,
                Expanded::Null => {}
                Expanded::One(ExpandedItem::Node(_) | ExpandedItem::Value(_) | ExpandedItem::Json(_) | ExpandedItem::List(_) | ExpandedItem::Geometry(_))
                | Expanded::Many(_) => return Err(Decline::MalformedJsonLd),
            }
        }
    }
    Ok(ExpandedItem::Geometry(Box::new(expanded)))
}

/// The member name `key` expands to, `None` for a key expansion drops.
fn member_name(scope: &TermScope<'_>, key: &str) -> Result<Option<String>, Decline> {
    match scope.expand_key(key)?.as_ref() {
        Term::Id(property) => Ok(property_name(property)),
        Term::Null => Ok(None),
        Term::Keyword(_) => Err(Decline::MalformedJsonLd),
    }
}

/// A typed member written when present.
pub fn add_optional<T: Serialize>(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: Option<&T>) -> Result<(), Decline> {
    value.map_or(Ok(()), |value| add_serialized(node, scope, key, value))
}

/// A typed member, expanded as the JSON it serializes to.
pub fn add_serialized<T: Serialize>(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: &T) -> Result<(), Decline> {
    let json = serde_json::to_value(value).map_err(|_| Decline::CompactSerialization)?;
    add_json(node, scope, key, &json)
}

/// A member held as JSON, expanded in place.
pub fn add_json(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: &Value) -> Result<(), Decline> {
    node.add_json(
        scope,
        &KeyedEntry {
            key,
            term: scope.expand_key(key)?,
            value,
        },
    )
}
