//! The clause 4.5.23.2 `join=inline` writer of a rendered temporal entity.
//!
//! Applied to the temporal API the nesting is per *instance*: a temporal
//! representation carries one Relationship instance per observation, and an
//! Attribute that pointed somewhere different at two instants nests a
//! different entity under each. Which copy is nested where, narrowed by which
//! list, and where nesting stops is the Linked Entity engine's decision; this
//! writer asks the cursor it is handed and spells the answer: an `entity`
//! member mirroring the spelling of `object` beside it, and an `entityList` in
//! `objectList` order.

use crate::{
    relationship_targets::{ObjectSpelling, rendered_relationship_targets},
    temporal::{
        linked_projection::narrowed_temporal_entity,
        temporal_evolutions::{TemporalEvolutions, TemporalMember},
    },
};
use andromeda_linked_retrieval::{
    plan::{InlineCursor, OpenEntity},
    representation::LinkedRepresentation,
};
use andromeda_native_stack::on_sufficient_stack;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::Uri};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::{Map, Value};
use std::slice;

/// Nest under `entity`'s Relationship instances the Linked Entities `cursor`
/// plans for them.
///
/// An instance whose targets the plan nests nothing for keeps its `object`
/// and gains no member: clause 4.5.23 leaves an unresolved Relationship a
/// plain URI rather than failing the retrieval.
pub fn write_temporal_inline_join(entity: &mut Value, cursor: InlineCursor<'_, TemporalEvolutions>) {
    // A root whose list removed `id` (clause 4.21) is opened without it.
    match TemporalEvolutions::identifier(entity) {
        Some(id) => nest_members(entity, &cursor.open(&id)),
        None => nest_members(entity, &cursor.open_unnamed_root()),
    }
}

/// Nest under every Attribute instance of the open entity.
fn nest_members(entity: &mut Value, open: &OpenEntity<'_, TemporalEvolutions>) {
    let Some(members) = entity.as_object_mut() else {
        return;
    };
    for (key, member) in members.iter_mut() {
        if TemporalMember::of(key) != TemporalMember::Attribute {
            continue;
        }
        for instance in instances_of_mut(member) {
            if let Value::Object(fields) = instance {
                nest_instance(fields, key, open);
            }
        }
    }
}

/// Nest whatever one rendered Attribute instance points at.
fn nest_instance(fields: &mut Map<String, Value>, attribute: &str, open: &OpenEntity<'_, TemporalEvolutions>) {
    // Clause 4.5.3.2 gives `object` "a URI or array of URIs", and the `entity`
    // member clause 4.5.23.2 adds mirrors that spelling. The targets are read
    // out first so the borrow of `fields` ends before the nested copies are
    // inserted back into it.
    if let Some(object) = fields.get(compact_properties::OBJECT) {
        let spelling = ObjectSpelling::of(object);
        let mut nested = nested_copies(&rendered_relationship_targets(object), attribute, open);
        match spelling {
            ObjectSpelling::OneUri => {
                if let Some(entity) = nested.pop() {
                    fields.insert(compact_properties::ENTITY.to_owned(), entity);
                }
            }
            ObjectSpelling::ArrayOfUris => {
                if !nested.is_empty() {
                    fields.insert(compact_properties::ENTITY.to_owned(), Value::Array(nested));
                }
            }
            ObjectSpelling::NoUri => {}
        }
    }
    if let Some(object_list) = fields.get(compact_properties::OBJECT_LIST) {
        let nested = nested_copies(&rendered_relationship_targets(object_list), attribute, open);
        if !nested.is_empty() {
            fields.insert(compact_properties::ENTITY_LIST.to_owned(), Value::Array(nested));
        }
    }
}

/// The copies the plan nests for `targets` under the open entity's
/// `attribute`, in order, each narrowed by its own list and expanded in turn.
fn nested_copies(targets: &[Uri], attribute: &str, open: &OpenEntity<'_, TemporalEvolutions>) -> Vec<Value> {
    targets
        .iter()
        .filter_map(|target| {
            let (node, cursor) = open.nested(attribute, target)?;
            let id = Identifier::try_from(target.clone()).ok()?; // clone: the path holds the identifier of the copy it opens
            let mut copy = narrowed_temporal_entity(node, cursor.rule());
            // The one point the writer recurses per nested Linked Entity.
            // Clause 4.5.23 sets no `joinLevel` cap, so the nesting depth is
            // the data's, and each level moves to a fresh stack segment once
            // the current one runs low.
            on_sufficient_stack(|| nest_members(&mut copy, &cursor.open(&id)));
            Some(copy)
        })
        .collect()
}

/// The instances one rendered Attribute member holds: an array of them, or
/// one normalised to a bare object.
fn instances_of_mut(member: &mut Value) -> impl Iterator<Item = &mut Value> {
    match member {
        Value::Array(instances) => instances.iter_mut(),
        single @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => slice::from_mut(single).iter_mut(),
    }
}

#[cfg(test)]
mod tests;
