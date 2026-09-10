//! The clause 4.5.23 `join=inline` expansion of a rendered temporal entity.
//!
//! A temporal request with `join=inline` asks for the entities a Relationship
//! points at to appear nested under the Relationship rather than being named by
//! URI alone. Applied to the temporal API the nesting is per *instance*: a
//! temporal representation carries one Relationship instance per observation,
//! and an attribute that pointed somewhere different at two instants nests a
//! different entity under each.
//!
//! The expansion runs over already-rendered values on both sides. A temporal
//! representation is never a typed resource — it is assembled from history rows
//! and compacted — so there is no entity type to render from, and the linked
//! side arrives as the same kind of value the roots do.

use crate::relationship_targets::{ObjectSpelling, rendered_relationship_targets};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use std::{collections::HashSet, slice};

/// The members of a rendered temporal entity that never name a Linked Entity.
const STRUCTURAL_MEMBERS: [&str; 6] = ["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"];

/// Nest the resolved temporal representations under the Relationship instances
/// that name them.
///
/// An `object` whose identifier `linked` resolves gains an `entity` member
/// carrying that entity's temporal evolution; an `objectList` gains an
/// `entityList` with one entry per identifier that resolved. An instance whose
/// object resolved to nothing is left exactly as it was — clause 4.5.23 leaves
/// an unresolvable Relationship as a plain URI rather than failing the
/// retrieval.
///
/// `linked` legitimately contains entities that reference one another, so the
/// recursion is bounded by the set of ids already open on the current path: an
/// entity that references itself, or forms a cycle with another, is nested once
/// and not descended into again. Without that guard a cyclic link recurses
/// without end and overflows the stack from a request.
pub fn apply_temporal_inline_join(entity: &mut Value, linked: &IndexMap<Uri, Value>) {
    let mut ancestors = HashSet::new();
    join_guarded(entity, linked, &mut ancestors);
}

/// Expand one temporal entity, tracking the ids already open on the current
/// path so a reference cycle terminates.
fn join_guarded(entity: &mut Value, linked: &IndexMap<Uri, Value>, ancestors: &mut HashSet<Uri>) {
    let self_id = identifier_of(entity);
    let seeded = self_id.as_ref().is_some_and(|id| ancestors.insert(id.clone())); // clone: the guard owns the ids on the current path

    if let Some(members) = entity.as_object_mut() {
        for (name, member) in members.iter_mut() {
            if STRUCTURAL_MEMBERS.contains(&name.as_str()) {
                continue;
            }
            for instance in instances_of_mut(member) {
                if let Value::Object(fields) = instance {
                    join_instance(fields, linked, ancestors);
                }
            }
        }
    }

    if seeded && let Some(id) = &self_id {
        ancestors.remove(id);
    }
}

/// Nest whatever one rendered attribute instance points at.
fn join_instance(fields: &mut Map<String, Value>, linked: &IndexMap<Uri, Value>, ancestors: &mut HashSet<Uri>) {
    // Clause 4.5.3.2 gives `object` "a URI or array of URIs", and the `entity`
    // member clause 4.5.23.2 adds mirrors that spelling: one nested evolution
    // for a bare URI, an ordered array for an array of them. The identifiers
    // are read out first so the borrow of `fields` ends before the nested
    // values are inserted back into it.
    if let Some(object) = fields.get(compact_properties::OBJECT) {
        let spelling = ObjectSpelling::of(object);
        let mut nested = nested_entities(&rendered_relationship_targets(object), linked, ancestors);
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
        let nested = nested_entities(&rendered_relationship_targets(object_list), linked, ancestors);
        if !nested.is_empty() {
            fields.insert(compact_properties::ENTITY_LIST.to_owned(), Value::Array(nested));
        }
    }
}

/// The temporal evolutions a list of identifiers resolves to, in order.
///
/// Clause 4.5.23.1 retrieves only Relationships targeting a locally stored
/// Entity, so an identifier that resolved to nothing contributes nothing rather
/// than a hole in the list.
fn nested_entities(objects: &[Uri], linked: &IndexMap<Uri, Value>, ancestors: &mut HashSet<Uri>) -> Vec<Value> {
    let mut nested = Vec::with_capacity(objects.len());
    for object in objects {
        if let Some(entity) = nested_entity(object, linked, ancestors) {
            nested.push(entity);
        }
    }
    nested
}

/// The temporal evolution one identifier resolves to, expanded in turn unless
/// it is already open on the current path.
fn nested_entity(object: &Uri, linked: &IndexMap<Uri, Value>, ancestors: &mut HashSet<Uri>) -> Option<Value> {
    let mut nested = linked.get(object).cloned()?; // clone: each nesting site owns its own copy of the linked representation
    if !ancestors.contains(object) {
        join_guarded(&mut nested, linked, ancestors);
    }
    Some(nested)
}

/// The identifier a rendered temporal entity carries, when it carries a valid
/// one.
fn identifier_of(entity: &Value) -> Option<Uri> {
    entity.get("id").and_then(Value::as_str).and_then(|raw| raw.parse().ok())
}

/// The attribute instances one rendered member holds.
///
/// A temporal member is normally an array of instances, but a single-instance
/// attribute may have been normalised to a bare object, and both shapes carry
/// the same Relationships.
fn instances_of_mut(member: &mut Value) -> impl Iterator<Item = &mut Value> {
    match member {
        Value::Array(instances) => instances.iter_mut(),
        single @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => slice::from_mut(single).iter_mut(),
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::inline_join::apply_temporal_inline_join;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use indexmap::IndexMap;
    use serde_json::{Value, json};

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    /// A temporal representation of one entity, carrying one plain observation
    /// so it is distinguishable once nested.
    fn linked(id: &str) -> Value {
        json!({
            "id": id,
            "type": "Thing",
            "reading": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}]
        })
    }

    fn resolved(entries: &[&str]) -> IndexMap<Uri, Value> {
        entries.iter().map(|id| (uri(id), linked(id))).collect()
    }

    /// Clause 4.5.23 per instance: an attribute that pointed at a different
    /// entity at each observation nests a different entity under each
    /// instance.
    #[test]
    fn each_relationship_instance_nests_the_entity_that_instance_names() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "parkedAt": [
                {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Relationship", "object": "urn:ngsi-ld:T:C", "observedAt": "2024-01-02T00:00:00Z"}
            ]
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]));

        pretty_assertions::assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
        pretty_assertions::assert_eq!(entity["parkedAt"][1]["entity"]["id"], "urn:ngsi-ld:T:C");
        assert!(entity["parkedAt"][0]["entity"]["reading"].is_array());
    }

    /// An instance whose object resolved to nothing is left untouched: clause
    /// 4.5.23 leaves it a plain URI rather than failing the retrieval.
    #[test]
    fn an_unresolved_instance_is_left_untouched() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "parkedAt": [
                {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"},
                {"type": "Relationship", "object": "urn:ngsi-ld:T:Missing", "observedAt": "2024-01-02T00:00:00Z"}
            ]
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B"]));

        assert!(entity["parkedAt"][0].get("entity").is_some());
        assert!(entity["parkedAt"][1].get("entity").is_none());
        pretty_assertions::assert_eq!(entity["parkedAt"][1]["object"], "urn:ngsi-ld:T:Missing");
    }

    /// A single-instance attribute normalised to a bare object nests just the
    /// same as an array of one.
    #[test]
    fn a_single_instance_rendered_as_a_bare_object_is_expanded() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "parkedAt": {"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B"]));

        pretty_assertions::assert_eq!(entity["parkedAt"]["entity"]["id"], "urn:ngsi-ld:T:B");
    }

    /// Clause 4.5.3.2 gives `object` "a URI or array of URIs", and a temporal
    /// instance naming several targets nests all of them in the order it named
    /// them.
    #[test]
    fn an_instance_naming_several_objects_nests_each_of_them_in_order() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "passengers": [
                {"type": "Relationship", "object": ["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"], "observedAt": "2024-01-01T00:00:00Z"}
            ]
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]));

        let nested = entity["passengers"][0]["entity"].as_array().expect("an entity array");
        pretty_assertions::assert_eq!(nested.len(), 2);
        pretty_assertions::assert_eq!(nested[0]["id"], "urn:ngsi-ld:T:B");
        pretty_assertions::assert_eq!(nested[1]["id"], "urn:ngsi-ld:T:C");
    }

    /// The nested member mirrors the spelling of the `object` beside it, so an
    /// array of one target answers with an array of one entity.
    #[test]
    fn an_object_written_as_a_one_element_array_nests_a_one_element_array() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "passengers": [{"type": "Relationship", "object": ["urn:ngsi-ld:T:B"], "observedAt": "2024-01-01T00:00:00Z"}]
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B"]));

        pretty_assertions::assert_eq!(entity["passengers"][0]["entity"].as_array().expect("an entity array").len(), 1);
    }

    /// A multi-target instance none of whose targets resolved gains no member,
    /// exactly as a single-target one does.
    #[test]
    fn a_multi_target_instance_that_resolves_to_nothing_is_left_untouched() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "passengers": [{"type": "Relationship", "object": ["urn:ngsi-ld:T:Missing"], "observedAt": "2024-01-01T00:00:00Z"}]
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B"]));

        assert!(entity["passengers"][0].get("entity").is_none(), "{}", entity["passengers"][0]);
    }

    /// A `ListRelationship` nests one entry per identifier that resolved, in list
    /// order, and the ones that did not resolve simply have no entry.
    #[test]
    fn an_object_list_nests_the_entities_that_resolved_in_list_order() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "hasParts": [{
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:T:C", "urn:ngsi-ld:T:Missing", "urn:ngsi-ld:T:B"],
                "observedAt": "2024-01-01T00:00:00Z"
            }]
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]));

        let nested = entity["hasParts"][0]["entityList"].as_array().expect("entityList array");
        pretty_assertions::assert_eq!(
            nested.iter().map(|item| item["id"].as_str()).collect::<Vec<_>>(),
            vec![Some("urn:ngsi-ld:T:C"), Some("urn:ngsi-ld:T:B")]
        );
    }

    /// Clause 4.5.22.2 renders a normalized `objectList` as `{"object": …}`
    /// members, and clause 4.5.23.2 nests the target Linked Entities of "a
    /// `ListRelationship`'s `objectList` in normalized representation", so the
    /// join has to read its targets out of that member shape as well as out of
    /// the bare URIs of the concise form.
    #[test]
    fn a_normalized_object_list_nests_the_entities_its_object_members_name() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "hasParts": [{
                "type": "ListRelationship",
                "objectList": [{"object": "urn:ngsi-ld:T:C"}, {"object": "urn:ngsi-ld:T:Missing"}, {"object": "urn:ngsi-ld:T:B"}],
                "observedAt": "2024-01-01T00:00:00Z"
            }]
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B", "urn:ngsi-ld:T:C"]));

        let nested = entity["hasParts"][0]["entityList"].as_array().expect("entityList array");
        pretty_assertions::assert_eq!(
            nested.iter().map(|item| item["id"].as_str()).collect::<Vec<_>>(),
            vec![Some("urn:ngsi-ld:T:C"), Some("urn:ngsi-ld:T:B")]
        );
    }

    /// The structural members of a temporal entity are never treated as
    /// attributes, so nothing inside them is expanded.
    #[test]
    fn structural_members_are_never_expanded() {
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "createdAt": "2024-01-01T00:00:00Z"
        });

        apply_temporal_inline_join(&mut entity, &resolved(&["urn:ngsi-ld:T:B"]));

        pretty_assertions::assert_eq!(entity["createdAt"], "2024-01-01T00:00:00Z");
    }

    /// A deeper level nests inside the one above it, so a two-hop join renders
    /// as a two-level nesting.
    #[test]
    fn a_second_level_nests_inside_the_first() {
        let mut linked_map = IndexMap::new();
        linked_map.insert(
            uri("urn:ngsi-ld:T:B"),
            json!({
                "id": "urn:ngsi-ld:T:B",
                "type": "Thing",
                "parkedAt": [{"type": "Relationship", "object": "urn:ngsi-ld:T:C", "observedAt": "2024-01-01T00:00:00Z"}]
            }),
        );
        linked_map.insert(uri("urn:ngsi-ld:T:C"), linked("urn:ngsi-ld:T:C"));
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "parkedAt": [{"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}]
        });

        apply_temporal_inline_join(&mut entity, &linked_map);

        pretty_assertions::assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:B");
        pretty_assertions::assert_eq!(entity["parkedAt"][0]["entity"]["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:C");
    }

    /// An entity that links to itself nests once and terminates, rather than
    /// recursing until the stack overflows.
    #[test]
    fn a_self_reference_terminates() {
        let self_ref = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "parkedAt": [{"type": "Relationship", "object": "urn:ngsi-ld:T:A", "observedAt": "2024-01-01T00:00:00Z"}]
        });
        let mut linked_map = IndexMap::new();
        linked_map.insert(uri("urn:ngsi-ld:T:A"), self_ref.clone()); // clone: the root and the resolved copy are separate values
        let mut entity = self_ref;

        apply_temporal_inline_join(&mut entity, &linked_map);

        pretty_assertions::assert_eq!(entity["parkedAt"][0]["entity"]["id"], "urn:ngsi-ld:T:A");
        assert!(entity["parkedAt"][0]["entity"]["parkedAt"][0].get("entity").is_none());
    }

    /// A two-entity cycle terminates: the back-reference is left as a plain
    /// Relationship rather than expanded a second time.
    #[test]
    fn a_two_entity_cycle_terminates() {
        let mut linked_map = IndexMap::new();
        linked_map.insert(
            uri("urn:ngsi-ld:T:B"),
            json!({
                "id": "urn:ngsi-ld:T:B",
                "type": "Thing",
                "parkedAt": [{"type": "Relationship", "object": "urn:ngsi-ld:T:A", "observedAt": "2024-01-01T00:00:00Z"}]
            }),
        );
        linked_map.insert(
            uri("urn:ngsi-ld:T:A"),
            json!({
                "id": "urn:ngsi-ld:T:A",
                "type": "Thing",
                "parkedAt": [{"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}]
            }),
        );
        let mut entity = json!({
            "id": "urn:ngsi-ld:T:A",
            "type": "Thing",
            "parkedAt": [{"type": "Relationship", "object": "urn:ngsi-ld:T:B", "observedAt": "2024-01-01T00:00:00Z"}]
        });

        apply_temporal_inline_join(&mut entity, &linked_map);

        let second = &entity["parkedAt"][0]["entity"]["parkedAt"][0]["entity"];
        pretty_assertions::assert_eq!(second["id"], "urn:ngsi-ld:T:A");
        assert!(second["parkedAt"][0].get("entity").is_none());
    }
}
