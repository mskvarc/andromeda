//! The clause 4.11 `join=inline` expansion of a rendered entity.
//!
//! A request with `join=inline` asks for the entities a Relationship points at
//! to appear nested under the Relationship rather than being named by URI
//! alone. The expansion runs over an already-rendered value, not over the
//! typed entity, because the same expansion applies to a payload that reached
//! the broker as JSON-LD and was never a `Entity` at all.

use crate::{entity::EntityRenderer, error::RepresentationError};
use andromeda_ngsi_ld_http_headers::language::LanguageFilter;
use andromeda_ngsi_ld_model::entity::Entity;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// Apply `join=inline` expansion to an already-rendered entity response.
///
/// Public because it applies to a `Value` as readily as to an [`Entity`]:
/// [`crate::snapshot::SnapshotRenderer`] runs it over a payload that arrived
/// already compacted, and [`EntityRenderer`] runs it over one it just built.
pub(crate) fn apply_inline_join(
    entity: &mut Value,
    linked: &HashMap<String, Entity>,
    language_filter: Option<&LanguageFilter>,
) -> Result<(), RepresentationError> {
    let Some(obj) = entity.as_object_mut() else { return Ok(()) };
    for attr_val in obj.values_mut() {
        match attr_val {
            Value::Object(attr_obj) => {
                inline_join_attribute_instance(attr_obj, linked, language_filter)?;
            }
            Value::Array(items) => {
                for item in items.iter_mut() {
                    if let Value::Object(attr_obj) = item {
                        inline_join_attribute_instance(attr_obj, linked, language_filter)?;
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
) -> Result<(), RepresentationError> {
    let attr_type = attr_obj.get("type").and_then(Value::as_str).map(ToOwned::to_owned);
    if attr_type.as_deref() == Some("Relationship") {
        if let Some(object_uri) = attr_obj.get("object").and_then(Value::as_str).map(ToOwned::to_owned)
            && let Some(linked_entity) = linked.get(&object_uri)
        {
            let mut rendered = EntityRenderer::new(linked_entity);
            if let Some(filter) = language_filter {
                rendered = rendered.with_language_filter(filter);
            }
            let mut linked_val = rendered.render()?;
            apply_inline_join(&mut linked_val, linked, language_filter)?;
            attr_obj.insert("entity".to_string(), linked_val);
        }
    } else if attr_type.as_deref() == Some("ListRelationship")
        && let Some(object_list) = attr_obj.get("objectList").and_then(Value::as_array)
    {
        let entity_list = object_list
            .iter()
            .filter_map(Value::as_str)
            .filter_map(|uri| linked.get(uri))
            .map(|linked_entity| {
                let mut rendered = EntityRenderer::new(linked_entity);
                if let Some(filter) = language_filter {
                    rendered = rendered.with_language_filter(filter);
                }
                let mut linked_val = rendered.render()?;
                apply_inline_join(&mut linked_val, linked, language_filter)?;
                Ok(linked_val)
            })
            .collect::<Result<Vec<_>, RepresentationError>>()?;
        if !entity_list.is_empty() {
            attr_obj.insert("entityList".to_string(), Value::Array(entity_list));
        }
    }
    Ok(())
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
}
