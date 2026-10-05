//! Expanding one attribute member of a merge fragment (clause 5.5.12).
//!
//! Each member the fragment wrote is expanded as the member of an entity's
//! instance is ([`crate::typed_walk::instance`]); a member it removes is
//! expanded as the NGSI-LD Null written under that member's key, which the
//! merge of the expanded fragment reads as the removal.

use crate::{
    compact_walk::{
        member::{Written, expanded_member},
        object::NodeBuilder,
    },
    decline::Decline,
    expanded_tree::ExpandedItem,
    term_scope::TermScope,
    typed_walk::instance::{add_geo_value, add_json, add_member, add_optional, add_serialized, add_value, add_value_list, member_name, null_property},
};
use andromeda_ngsi_ld_model::attribute::{merge_instance::MergeInstance, merge_member::MergeMember, merge_value::MergeValue, patched::Patched};
use andromeda_ngsi_ld_vocabulary::{NULL_URI, compact::properties as compact_properties};
use std::slice::from_ref;

/// The member of an instance that names its family.
const TYPE_MEMBER: &str = "type";

/// The expanded name and items of the merge member written under `key` in
/// `scope`; `None` for a key expansion drops.
pub(crate) fn merge_expanded(member: &MergeMember, scope: &TermScope<'_>, key: &str) -> Result<Option<(String, Vec<ExpandedItem>)>, Decline> {
    let (instances, written) = match member {
        MergeMember::Single(instance) => (from_ref(instance.as_ref()), Written::Single),
        MergeMember::Multiple(instances) => (instances.as_slice(), Written::Array),
        // Below the top level the NGSI-LD Null is the value of a concise
        // Property (clause 4.5.2.3), whose merge removes the member.
        MergeMember::NgsiLdNull => return null_property(scope, key),
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

/// Expand one merge instance written under `key` in `parent`.
fn instance_items(parent: &TermScope<'_>, key: &str, instance: &MergeInstance) -> Result<Vec<ExpandedItem>, Decline> {
    let scope = parent.object_scope(Some(key))?;
    let mut node = NodeBuilder::default();
    // An instance that names no family is merged into the family of the
    // attribute it updates (clauses 5.6.4 and 5.6.17), so none is written.
    if let Some(family) = instance.family {
        add_serialized(&mut node, &scope, TYPE_MEMBER, family.short_name())?;
        match &instance.value {
            Some(Patched::Set(value)) => add_merge_value(&mut node, &scope, family.compact_value_key(), value)?,
            Some(Patched::Null) => add_serialized(&mut node, &scope, family.compact_value_key(), NULL_URI.as_str())?,
            None => {}
        }
    }
    add_optional(&mut node, &scope, compact_properties::UNIT_CODE, instance.unit_code.as_ref())?;
    add_optional(&mut node, &scope, compact_properties::VALUE_TYPE, instance.value_type.as_ref())?;
    add_optional(&mut node, &scope, compact_properties::OBJECT_TYPE, instance.object_type.as_ref())?;
    // Clause 4.5.2.2: the seals of `ngsildproof` are non-reified, each written
    // as the value it is.
    if let Some(seal) = &instance.entity_id_sealed {
        add_json(&mut node, &scope, compact_properties::ENTITY_ID_SEALED, seal.piece())?;
    }
    if let Some(seal) = &instance.entity_type_sealed {
        add_json(&mut node, &scope, compact_properties::ENTITY_TYPE_SEALED, seal.piece())?;
    }
    add_optional(&mut node, &scope, compact_properties::DATASET_ID, instance.dataset_id.as_ref())?;
    add_optional(&mut node, &scope, compact_properties::EXPIRES_AT, instance.expires_at.as_ref())?;
    add_optional(&mut node, &scope, compact_properties::INSTANCE_ID, instance.instance_id.as_ref())?;
    add_optional(&mut node, &scope, compact_properties::OBSERVED_AT, instance.observed_at.as_ref())?;
    for (name, member) in &instance.sub_attributes {
        add_member(&mut node, &scope, name.as_str(), member)?;
    }
    Ok(node.finish(Some(key)).into_items())
}

/// The value member of a merge instance, under `key`.
fn add_merge_value(node: &mut NodeBuilder, scope: &TermScope<'_>, key: &str, value: &MergeValue) -> Result<(), Decline> {
    match value {
        MergeValue::Property(value) => add_value(node, scope, key, value),
        MergeValue::GeoProperty(value) => add_geo_value(node, scope, key, value),
        MergeValue::JsonProperty(json) => add_json(node, scope, key, json.piece()),
        MergeValue::ListProperty(values) => add_value_list(node, scope, values),
        MergeValue::Relationship(object) => add_serialized(node, scope, key, object),
        MergeValue::VocabProperty(vocab) => add_serialized(node, scope, key, vocab),
        MergeValue::LanguageProperty(language_map) => add_serialized(node, scope, key, language_map),
        MergeValue::ListRelationship(object_list) => add_serialized(node, scope, key, object_list),
    }
}
