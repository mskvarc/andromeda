//! The clause 4.5.23 `join=inline` expansion of a rendered entity.
//!
//! A request with `join=inline` asks for the entities a Relationship points at
//! to appear nested under the Relationship rather than being named by URI
//! alone. The expansion runs over an already-rendered value, not over the
//! typed entity, because the same expansion applies to a payload that reached
//! the broker as JSON-LD and was never a `Entity` at all.

use crate::{entity::EntityRenderer, error::RenderError};
use andromeda_ngsi_ld_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_model::entity::Entity;
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

/// Apply `join=inline` expansion to an already-rendered entity response.
///
/// Exposed within the crate (`pub(crate)`) because it applies to a `Value` as
/// readily as to an [`Entity`]: [`crate::snapshot::SnapshotRenderer`] runs it
/// over a payload that arrived already compacted, and [`EntityRenderer`] runs it
/// over one it just built.
///
/// The `linked` map legitimately contains entities that reference one another,
/// so the recursion is bounded by the set of ids already open on the current
/// path: a linked entity that references itself, or forms a cycle with another,
/// is rendered once and not descended into again. Without that guard a cyclic
/// linked entity recurses without end and overflows the stack from a request.
pub(crate) fn apply_inline_join(entity: &mut Value, linked: &HashMap<String, Entity>, language_filter: Option<&LanguageFilter>) -> Result<(), RenderError> {
    let mut ancestors = HashSet::new();
    apply_inline_join_guarded(entity, linked, language_filter, &mut ancestors)
}

/// Expand one entity, tracking the ids already open on the current path so a
/// reference cycle terminates.
fn apply_inline_join_guarded(
    entity: &mut Value,
    linked: &HashMap<String, Entity>,
    language_filter: Option<&LanguageFilter>,
    ancestors: &mut HashSet<String>,
) -> Result<(), RenderError> {
    let self_id = entity.get("id").and_then(Value::as_str).map(ToOwned::to_owned);
    // Track this entity's id on the current path so a self-reference is caught.
    let seeded = self_id.as_ref().is_some_and(|id| ancestors.insert(id.clone()));

    let result = inline_join_object(entity, linked, language_filter, ancestors);

    if seeded && let Some(id) = &self_id {
        ancestors.remove(id);
    }
    result
}

/// Walk every attribute instance of one rendered entity object.
fn inline_join_object(
    entity: &mut Value,
    linked: &HashMap<String, Entity>,
    language_filter: Option<&LanguageFilter>,
    ancestors: &mut HashSet<String>,
) -> Result<(), RenderError> {
    let Some(obj) = entity.as_object_mut() else { return Ok(()) };
    for attr_val in obj.values_mut() {
        match attr_val {
            Value::Object(attr_obj) => {
                inline_join_attribute_instance(attr_obj, linked, language_filter, ancestors)?;
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    if let Value::Object(attr_obj) = item {
                        inline_join_attribute_instance(attr_obj, linked, language_filter, ancestors)?;
                    }
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    Ok(())
}

/// Expand a single attribute instance with inline-joined linked entities.
///
/// For `Relationship` attributes, resolves the `object` URI against the linked
/// entity map and inserts the rendered entity under the `"entity"` key.
/// For `ListRelationship` attributes, resolves each URI in `objectList` and
/// inserts the rendered entities under the `"entityList"` key.
fn inline_join_attribute_instance(
    attr_obj: &mut Map<String, Value>,
    linked: &HashMap<String, Entity>,
    language_filter: Option<&LanguageFilter>,
    ancestors: &mut HashSet<String>,
) -> Result<(), RenderError> {
    let attr_type = attr_obj.get("type").and_then(Value::as_str).and_then(|raw| AttributeType::from_str(raw).ok());
    match attr_type {
        Some(AttributeType::Relationship) => {
            if let Some(object_uri) = attr_obj.get("object").and_then(Value::as_str).map(ToOwned::to_owned)
                && let Some(linked_entity) = linked.get(&object_uri)
            {
                let linked_val = render_linked(linked_entity, &object_uri, linked, language_filter, ancestors)?;
                attr_obj.insert("entity".to_string(), linked_val);
            }
        }
        Some(AttributeType::ListRelationship) => {
            if let Some(object_list) = attr_obj.get("objectList").and_then(Value::as_array) {
                // Collect the URIs first so the borrow of `attr_obj` is released
                // before the recursive expansion, which needs the mutable
                // ancestor set.
                let uris: Vec<String> = object_list.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect();
                let mut entity_list = Vec::new();
                for uri in &uris {
                    if let Some(linked_entity) = linked.get(uri) {
                        entity_list.push(render_linked(linked_entity, uri, linked, language_filter, ancestors)?);
                    }
                }
                if !entity_list.is_empty() {
                    attr_obj.insert("entityList".to_string(), Value::Array(entity_list));
                }
            }
        }
        // Only a Relationship or a ListRelationship names a linked entity; every
        // other attribute kind, and an unrecognised type, is left untouched.
        Some(
            AttributeType::Property
            | AttributeType::GeoProperty
            | AttributeType::VocabProperty
            | AttributeType::LanguageProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty,
        )
        | None => {}
    }
    Ok(())
}

/// Render one linked entity and expand it in turn, unless its id is already open
/// on the current path — in which case the back-reference is left unexpanded so
/// the cycle terminates.
fn render_linked(
    linked_entity: &Entity,
    object_uri: &str,
    linked: &HashMap<String, Entity>,
    language_filter: Option<&LanguageFilter>,
    ancestors: &mut HashSet<String>,
) -> Result<Value, RenderError> {
    let mut linked_val = EntityRenderer::new(linked_entity).with_optional_language_filter(language_filter).render()?;
    if !ancestors.contains(object_uri) {
        apply_inline_join_guarded(&mut linked_val, linked, language_filter, ancestors)?;
    }
    Ok(linked_val)
}

#[cfg(test)]
mod tests {
    use crate::inline_join::apply_inline_join;
    use andromeda_ngsi_ld_model::entity::Entity;
    use serde_json::{Value, json};
    use std::collections::HashMap;

    fn parse_entity(value: Value) -> Entity {
        serde_json::from_value(value).expect("entity")
    }

    #[test]
    fn inline_join_expands_relationship() {
        let linked_entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Room:1",
            "type": "Room",
            "name": {"type": "Property", "value": "Conference Room"}
        }));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:ngsi-ld:Room:1".to_string(), linked_entity);

        let mut entity = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "hasRoom": {"type": "Relationship", "object": "urn:ngsi-ld:Room:1"}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        assert!(entity["hasRoom"]["entity"].is_object());
        pretty_assertions::assert_eq!(entity["hasRoom"]["entity"]["id"], "urn:ngsi-ld:Room:1");
        pretty_assertions::assert_eq!(entity["hasRoom"]["entity"]["name"]["value"], "Conference Room");
    }

    #[test]
    fn inline_join_expands_list_relationship() {
        let room_a = parse_entity(json!({"id": "urn:a", "type": "Room", "name": {"type": "Property", "value": "A"}}));
        let room_b = parse_entity(json!({"id": "urn:b", "type": "Room", "name": {"type": "Property", "value": "B"}}));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:a".to_string(), room_a);
        linked_map.insert("urn:b".to_string(), room_b);

        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRooms": {"type": "ListRelationship", "objectList": ["urn:a", "urn:b"]}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        let entity_list = entity["hasRooms"]["entityList"].as_array().expect("entityList array");
        pretty_assertions::assert_eq!(entity_list.len(), 2);
    }

    #[test]
    fn inline_join_skips_unresolved() {
        let linked_map: HashMap<String, Entity> = HashMap::new();
        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRoom": {"type": "Relationship", "object": "urn:ngsi-ld:Missing:1"}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        assert!(entity["hasRoom"].get("entity").is_none());
    }

    #[test]
    fn inline_join_handles_multi_attribute_array() {
        let room = parse_entity(json!({"id": "urn:a", "type": "Room", "name": {"type": "Property", "value": "A"}}));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:a".to_string(), room);

        let mut entity = json!({
            "id": "urn:building", "type": "Building",
            "hasRoom": [
                {"type": "Relationship", "object": "urn:a", "datasetId": "urn:ds:1"},
                {"type": "Relationship", "object": "urn:missing", "datasetId": "urn:ds:2"}
            ]
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        let arr = entity["hasRoom"].as_array().expect("array");
        assert!(arr[0].get("entity").is_some());
        assert!(arr[1].get("entity").is_none());
    }

    #[test]
    fn inline_join_recursive() {
        let entity_c = parse_entity(json!({
            "id": "urn:c", "type": "T", "name": {"type": "Property", "value": "C"}
        }));
        let entity_b = parse_entity(json!({
            "id": "urn:b", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:c"}
        }));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:b".to_string(), entity_b);
        linked_map.insert("urn:c".to_string(), entity_c);

        let mut entity = json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:b"}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["id"], "urn:b");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["ref"]["entity"]["id"], "urn:c");
    }

    #[test]
    fn inline_join_breaks_a_self_reference() {
        // An entity that links to itself must render once and terminate, not
        // recurse until the stack overflows.
        let self_ref = parse_entity(json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:a"}
        }));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:a".to_string(), self_ref);

        let mut entity = json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:a"}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join terminates");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["id"], "urn:a");
        // The self-reference inside the nested entity is left unexpanded.
        assert!(entity["ref"]["entity"]["ref"].get("entity").is_none());
    }

    #[test]
    fn inline_join_breaks_a_two_entity_cycle() {
        // A -> B -> A. Rendering must terminate; the back-reference from B to A
        // is left as a plain Relationship rather than expanded a second time.
        let entity_a = parse_entity(json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:b"}
        }));
        let entity_b = parse_entity(json!({
            "id": "urn:b", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:a"}
        }));
        let mut linked_map = HashMap::new();
        linked_map.insert("urn:a".to_string(), entity_a);
        linked_map.insert("urn:b".to_string(), entity_b);

        let mut entity = json!({
            "id": "urn:a", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:b"}
        });

        apply_inline_join(&mut entity, &linked_map, None).expect("inline join terminates");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["id"], "urn:b");
        pretty_assertions::assert_eq!(entity["ref"]["entity"]["ref"]["entity"]["id"], "urn:a");
        // The cycle is broken: A reached through B is not expanded again.
        assert!(entity["ref"]["entity"]["ref"]["entity"]["ref"].get("entity").is_none());
    }
}
