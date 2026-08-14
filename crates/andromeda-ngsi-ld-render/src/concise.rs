//! The clause 4.5.2.3 concise rendering of an entity.
//!
//! Concise drops the `type` member of every attribute whose kind the value
//! member already implies, and collapses an attribute that carries nothing but
//! a type and a value down to the value alone. Which member carries the value
//! is the same eight-way table the model owns, so it is read from
//! `AttributeType` rather than restated here.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
use andromeda_ngsi_ld_representation::entity_members::ENTITY_MEMBERS;
use serde_json::{Map, Value};
use std::str::FromStr;

/// Attribute members that are metadata rather than sub-attributes.
///
/// Clause 4.5.2.3 conciseness applies to sub-attributes; these members are part
/// of the attribute itself, so they survive unchanged rather than being walked
/// into.
const ATTRIBUTE_METADATA: &[&str] = &["observedAt", "datasetId", "unitCode", "createdAt", "modifiedAt", "expiresAt", "instanceId"];

/// Convert a normalized rendered entity value into concise form (NGSI-LD clause 4.5.2.3).
///
/// Concise representation removes redundant `"type"` keys from attributes when the
/// attribute kind can be inferred from its value key. Sub-attributes and metadata
/// are preserved. Entity-level keys (`id`, `type`, `scope`, `@context`) pass through
/// unchanged.
///
/// Consumes `val` to avoid deep-cloning the entity tree; pass `value.clone()`
/// at the call site if the caller still needs the original.
pub fn concise_value(val: Value) -> Value {
    let Value::Object(obj) = val else { return val };

    let mut map = Map::with_capacity(obj.len());
    for (key, value) in obj {
        if ENTITY_MEMBERS.contains(&key.as_str()) {
            map.insert(key, value);
        } else {
            map.insert(key, concise_attr_value(value));
        }
    }
    Value::Object(map)
}

/// Apply concise transformation to an attribute value (single instance or multi-attribute array).
fn concise_attr_value(value: Value) -> Value {
    match value {
        Value::Object(attr_obj) => concise_single_instance(attr_obj),
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|item| match item {
                    Value::Object(attr_obj) => concise_single_instance(attr_obj),
                    other => other,
                })
                .collect(),
        ),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other,
    }
}

/// Concise a single attribute instance object.
///
/// Only a Property or GeoProperty collapses to its bare value: clause 4.5.2.3
/// licenses the bare form for those two, so `{"type": "Property", "value": 25}`
/// becomes `25`. Every other family keeps its mandatory distinguishing member
/// (clause 4.5.3.3 and the five list/language/vocab/json clauses): the `type`
/// is dropped but a Relationship stays `{"object": …}`, a LanguageProperty
/// `{"languageMap": …}`, and so on — collapsing them to a bare value would read
/// back as a different attribute type. An instance carrying sub-attributes or
/// metadata always keeps its object shape, whatever its family.
fn concise_single_instance(mut attr_obj: Map<String, Value>) -> Value {
    // The `type` member of a rendered attribute is unvalidated JSON, so it is
    // read through `AttributeType`'s own parser; anything it rejects is not an
    // attribute this rendering understands and keeps its envelope.
    let attr_type = attr_obj.get("type").and_then(Value::as_str).and_then(|raw| AttributeType::from_str(raw).ok());
    let Some(attr_type) = attr_type else { return Value::Object(attr_obj) };
    let value_key = attr_type.compact_value_key();

    let collapses_to_bare_value = matches!(attr_type, AttributeType::Property | AttributeType::GeoProperty);
    let is_bare = attr_obj.len() == 2 && attr_obj.contains_key("type") && attr_obj.contains_key(value_key);

    if is_bare && collapses_to_bare_value {
        if let Some(value) = attr_obj.remove(value_key) {
            return value;
        }
        return Value::Object(attr_obj);
    }

    // Drop `type` and keep the value member; recursively concise sub-attributes.
    attr_obj.remove("type");
    let mut result = Map::with_capacity(attr_obj.len());
    for (k, v) in attr_obj {
        if k == value_key || ATTRIBUTE_METADATA.contains(&k.as_str()) {
            result.insert(k, v);
        } else {
            // Sub-attribute: recursively concise.
            result.insert(k, concise_attr_value(v));
        }
    }
    Value::Object(result)
}

#[cfg(test)]
mod tests {
    use crate::{concise::concise_value, entity::EntityRenderer};
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
    use serde_json::{Value, json};

    fn parse_entity(value: Value) -> Entity {
        serde_json::from_value(value).expect("entity")
    }

    #[test]
    fn concise_property_without_sub_attrs_collapses_to_value() {
        let input = json!({"id": "urn:x", "type": "T", "temp": {"type": "Property", "value": 25}});
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["temp"], json!(25));
        pretty_assertions::assert_eq!(result["id"], "urn:x");
        pretty_assertions::assert_eq!(result["type"], "T");
    }

    #[test]
    fn concise_property_with_sub_attrs_removes_type() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "temp": {
                "type": "Property",
                "value": 121,
                "observedAt": "2024-01-01T00:00:00Z",
                "reliability": {"type": "Property", "value": 0.7}
            }
        });
        let result = concise_value(input);
        let temp = result.get("temp").expect("temp");
        assert!(temp.get("type").is_none(), "type key should be removed");
        pretty_assertions::assert_eq!(temp["value"], 121);
        pretty_assertions::assert_eq!(temp["observedAt"], "2024-01-01T00:00:00Z");
        // Sub-attribute should be recursively concised
        pretty_assertions::assert_eq!(temp["reliability"], json!(0.7));
    }

    #[test]
    fn concise_relationship_keeps_its_object_member() {
        // Clause 4.5.3.3: `object` is Mandatory; the concise form drops `type`
        // but keeps `{"object": …}`, or it would read back as a string Property.
        let input = json!({"id": "urn:x", "type": "T", "ref": {"type": "Relationship", "object": "urn:y"}});
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["ref"], json!({"object": "urn:y"}));
    }

    #[test]
    fn concise_relationship_with_sub_attrs() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "ref": {"type": "Relationship", "object": "urn:y", "confidence": {"type": "Property", "value": 0.9}}
        });
        let result = concise_value(input);
        let r = result.get("ref").expect("ref");
        assert!(r.get("type").is_none());
        pretty_assertions::assert_eq!(r["object"], "urn:y");
        pretty_assertions::assert_eq!(r["confidence"], json!(0.9));
    }

    #[test]
    fn concise_geo_property_without_sub_attrs() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}}
        });
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["location"], json!({"type": "Point", "coordinates": [1, 2]}));
    }

    #[test]
    fn concise_language_property_keeps_its_language_map_member() {
        // Clause 4.5.18.3: `languageMap` is Mandatory and survives the collapse.
        let input = json!({
            "id": "urn:x", "type": "T",
            "name": {"type": "LanguageProperty", "languageMap": {"en": "Tower"}}
        });
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["name"], json!({"languageMap": {"en": "Tower"}}));
    }

    #[test]
    fn concise_json_property_keeps_its_json_member() {
        // Clause 4.5.24.3: `json` is Mandatory and survives the collapse.
        let input = json!({
            "id": "urn:x", "type": "T",
            "config": {"type": "JsonProperty", "json": {"key": "val"}}
        });
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["config"], json!({"json": {"key": "val"}}));
    }

    #[test]
    fn concise_vocab_property_keeps_its_vocab_member() {
        // Clause 4.5.20.3: `vocab` is Mandatory and survives the collapse.
        let input = json!({
            "id": "urn:x", "type": "T",
            "category": {"type": "VocabProperty", "vocab": "commercial"}
        });
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["category"], json!({"vocab": "commercial"}));
    }

    #[test]
    fn concise_list_property_keeps_its_value_list_member() {
        // Clause 4.5.21.3: `valueList` is Mandatory and survives the collapse.
        let input = json!({
            "id": "urn:x", "type": "T",
            "tags": {"type": "ListProperty", "valueList": ["a", "b"]}
        });
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["tags"], json!({"valueList": ["a", "b"]}));
    }

    #[test]
    fn concise_list_relationship_keeps_its_object_list_member() {
        // Clause 4.5.22.3: `objectList` is Mandatory and survives the collapse.
        let input = json!({
            "id": "urn:x", "type": "T",
            "refs": {"type": "ListRelationship", "objectList": ["urn:a", "urn:b"]}
        });
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["refs"], json!({"objectList": ["urn:a", "urn:b"]}));
    }

    #[test]
    fn concise_preserves_id_type_scope_context() {
        let input = json!({
            "id": "urn:x",
            "type": "T",
            "scope": ["/A"],
            "@context": "https://example.org/ctx",
            "temp": {"type": "Property", "value": 10}
        });
        let result = concise_value(input);
        pretty_assertions::assert_eq!(result["id"], "urn:x");
        pretty_assertions::assert_eq!(result["type"], "T");
        pretty_assertions::assert_eq!(result["scope"], json!(["/A"]));
        pretty_assertions::assert_eq!(result["@context"], "https://example.org/ctx");
    }

    #[test]
    fn concise_multi_attribute_array() {
        let input = json!({
            "id": "urn:x", "type": "T",
            "temp": [
                {"type": "Property", "value": 25, "datasetId": "urn:ds:1"},
                {"type": "Property", "value": 30, "datasetId": "urn:ds:2"}
            ]
        });
        let result = concise_value(input);
        let arr = result["temp"].as_array().expect("array");
        pretty_assertions::assert_eq!(arr.len(), 2);
        // Each instance should be concised individually -- with datasetId they have metadata
        // so they keep the value key but lose "type"
        assert!(arr[0].get("type").is_none());
        pretty_assertions::assert_eq!(arr[0]["value"], 25);
        pretty_assertions::assert_eq!(arr[0]["datasetId"], "urn:ds:1");
    }

    #[test]
    fn concise_via_renderer_mode() {
        let entity = parse_entity(json!({
            "id": "urn:ngsi-ld:Building:010",
            "type": "Building",
            "name": {"type": "Property", "value": "Tower"}
        }));
        let result = EntityRenderer::new(&entity).with_mode(ResponseFormat::Concise).render().expect("concise");
        pretty_assertions::assert_eq!(result["name"], "Tower");
    }
}
