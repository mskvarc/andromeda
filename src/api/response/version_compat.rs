//! Payload adaptation for NGSI-LD version conformance (clause 4.3.6.8).
//!
//! When a client sends `Prefer: ngsi-ld=<version>`, the broker responds with
//! a 203 Non-Authoritative Information and adapts the payload to remove or
//! reformat fields that did not exist in the requested version.

use crate::header::NgsiLdVersion;
use serde_json::Value;

/// Adapt a serialized NGSI-LD entity payload to conform to an older spec version.
///
/// Applies the fallback tables from clause 4.3.6.8:
/// - Table 4.3.6.8-1: entity-level removals
/// - Table 4.3.6.8-2: property-level removals
/// - Table 4.3.6.8-3: relationship-level removals
///
/// This function modifies the value in place. It handles both single entities
/// (JSON objects) and entity arrays.
pub fn adapt_payload_for_version(value: &mut Value, target: NgsiLdVersion) {
    if target >= NgsiLdVersion::CONFORMANT {
        return;
    }

    match value {
        Value::Object(_) => adapt_entity(value, target),
        Value::Array(items) => {
            for item in items {
                adapt_entity(item, target);
            }
        }
        _ => {}
    }
}

/// Adapt a single entity object.
fn adapt_entity(entity: &mut Value, target: NgsiLdVersion) {
    let Some(obj) = entity.as_object_mut() else {
        return;
    };

    // Table 4.3.6.8-1: entity-level removals
    if target < NgsiLdVersion::V1_9 {
        obj.remove("expiresAt");
    }
    if target < NgsiLdVersion::V1_4 {
        obj.remove("scope");
    }

    // Collect attribute keys (skip meta-fields)
    let attr_keys: Vec<String> = obj
        .keys()
        .filter(|k| !matches!(k.as_str(), "id" | "type" | "@context" | "scope" | "expiresAt" | "createdAt" | "modifiedAt"))
        .cloned()
        .collect();

    for key in attr_keys {
        if let Some(attr_value) = obj.get_mut(&key) {
            adapt_attribute(attr_value, target);
        }
    }
}

/// Adapt an attribute value (which may be a single instance or an array of instances).
fn adapt_attribute(value: &mut Value, target: NgsiLdVersion) {
    match value {
        Value::Array(instances) => {
            // Note 1-3: target < V1_3 means multi-attribute not supported;
            // find default instance (first without datasetId) BEFORE adapting removes datasetId
            if target < NgsiLdVersion::V1_3 && instances.len() > 1 {
                let default_idx = instances
                    .iter()
                    .position(|inst| inst.as_object().is_some_and(|o| !o.contains_key("datasetId")))
                    .unwrap_or(0);
                let default_instance = instances.swap_remove(default_idx);
                *instances = vec![default_instance];
            }
            for instance in instances.iter_mut() {
                adapt_attribute_instance(instance, target);
            }
        }
        Value::Object(_) => {
            adapt_attribute_instance(value, target);
        }
        _ => {}
    }
}

/// Adapt a single attribute instance based on its type.
fn adapt_attribute_instance(instance: &mut Value, target: NgsiLdVersion) {
    let Some(obj) = instance.as_object_mut() else {
        return;
    };

    let attr_type = obj.get("type").and_then(|t| t.as_str()).map(String::from);

    match attr_type.as_deref() {
        Some("Property") | Some("GeoProperty") => {
            adapt_property_fields(obj, target);
        }
        Some("Relationship") => {
            adapt_relationship_fields(obj, target);
        }
        Some("LanguageProperty") => {
            if target < NgsiLdVersion::V1_4 {
                // Reformat LanguageProperty as Property: move languageMap to value
                if let Some(lang_map) = obj.remove("languageMap") {
                    obj.insert("value".to_string(), lang_map);
                }
                obj.insert("type".to_string(), Value::String("Property".to_string()));
            }
            adapt_property_fields(obj, target);
        }
        Some("VocabProperty") => {
            if target < NgsiLdVersion::V1_8 {
                // Reformat VocabProperty as Property: vocab -> value
                if let Some(vocab) = obj.remove("vocab") {
                    obj.insert("value".to_string(), vocab);
                }
                obj.insert("type".to_string(), Value::String("Property".to_string()));
            }
            adapt_property_fields(obj, target);
        }
        Some("JsonProperty") => {
            if target < NgsiLdVersion::V1_8 {
                // Reformat JsonProperty as Property: json -> value
                if let Some(json_val) = obj.remove("json") {
                    obj.insert("value".to_string(), json_val);
                }
                obj.insert("type".to_string(), Value::String("Property".to_string()));
            }
            adapt_property_fields(obj, target);
        }
        Some("ListProperty") => {
            if target < NgsiLdVersion::V1_8 {
                // Reformat ListProperty as Property: valueList -> value
                if let Some(list) = obj.remove("valueList") {
                    obj.insert("value".to_string(), list);
                }
                obj.insert("type".to_string(), Value::String("Property".to_string()));
            }
            adapt_property_fields(obj, target);
        }
        Some("ListRelationship") => {
            if target < NgsiLdVersion::V1_8 {
                // Reformat ListRelationship as Relationship: objectList -> object (first element)
                if let Some(Value::Array(list)) = obj.remove("objectList") {
                    let first = list.into_iter().next().unwrap_or(Value::Null);
                    obj.insert("object".to_string(), first);
                }
                obj.insert("type".to_string(), Value::String("Relationship".to_string()));
            }
            adapt_relationship_fields(obj, target);
        }
        _ => {}
    }
}

/// Remove property-level fields per Table 4.3.6.8-2.
fn adapt_property_fields(obj: &mut serde_json::Map<String, Value>, target: NgsiLdVersion) {
    if target < NgsiLdVersion::V1_3 {
        obj.remove("datasetId");
        obj.remove("observedAt");
        obj.remove("unitCode");
    }
    if target < NgsiLdVersion::V1_9 {
        obj.remove("expiresAt");
        obj.remove("valueType");
    }
}

/// Remove relationship-level fields per Table 4.3.6.8-3.
fn adapt_relationship_fields(obj: &mut serde_json::Map<String, Value>, target: NgsiLdVersion) {
    if target < NgsiLdVersion::V1_3 {
        obj.remove("datasetId");
        obj.remove("observedAt");
    }
    if target < NgsiLdVersion::V1_8 {
        obj.remove("objectType");
    }
    if target < NgsiLdVersion::V1_9 {
        obj.remove("expiresAt");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn no_adaptation_for_conformant_version() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "expiresAt": "2025-01-01T00:00:00Z",
            "scope": ["/A"],
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "datasetId": "urn:ds:1",
                "expiresAt": "2025-01-01T00:00:00Z"
            }
        });
        let original = value.clone();
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_9);
        assert_eq!(value, original);
    }

    #[test]
    fn v1_8_removes_entity_expires_at() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "expiresAt": "2025-01-01T00:00:00Z",
            "temperature": {
                "type": "Property",
                "value": 25.0
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_8);
        assert!(value.get("expiresAt").is_none());
    }

    #[test]
    fn v1_3_removes_scope_from_entity() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "scope": ["/A/B"],
            "temperature": {
                "type": "Property",
                "value": 25.0
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_3);
        assert!(value.get("scope").is_none());
    }

    #[test]
    fn v1_0_removes_dataset_id_and_observed_at_from_property() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "datasetId": "urn:ds:1",
                "observedAt": "2025-01-01T00:00:00Z",
                "unitCode": "CEL"
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_0);
        let temp = value.get("temperature").unwrap();
        assert!(temp.get("datasetId").is_none());
        assert!(temp.get("observedAt").is_none());
        assert!(temp.get("unitCode").is_none());
    }

    #[test]
    fn v1_0_removes_dataset_id_and_observed_at_from_relationship() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Building:1",
                "datasetId": "urn:ds:1",
                "observedAt": "2025-01-01T00:00:00Z",
                "objectType": "Building"
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_0);
        let rel = value.get("refBuilding").unwrap();
        assert!(rel.get("datasetId").is_none());
        assert!(rel.get("observedAt").is_none());
        assert!(rel.get("objectType").is_none());
    }

    #[test]
    fn v1_3_reformats_language_property_as_property() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "name": {
                "type": "LanguageProperty",
                "languageMap": {
                    "en": "Tower",
                    "de": "Turm"
                }
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_3);
        let name = value.get("name").unwrap();
        assert_eq!(name.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(name.get("languageMap").is_none());
        assert!(name.get("value").is_some());
    }

    #[test]
    fn v1_4_removes_expires_at_and_value_type_from_property() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "expiresAt": "2025-01-01T00:00:00Z",
                "valueType": "Number"
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_4);
        let temp = value.get("temperature").unwrap();
        assert!(temp.get("expiresAt").is_none());
        assert!(temp.get("valueType").is_none());
    }

    #[test]
    fn v1_4_reformats_vocab_property_as_property() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "category": {
                "type": "VocabProperty",
                "vocab": "commercial"
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_4);
        let cat = value.get("category").unwrap();
        assert_eq!(cat.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(cat.get("vocab").is_none());
        assert_eq!(cat.get("value").and_then(|v| v.as_str()), Some("commercial"));
    }

    #[test]
    fn v1_4_reformats_list_relationship_as_relationship() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refRooms": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_4);
        let rel = value.get("refRooms").unwrap();
        assert_eq!(rel.get("type").and_then(|t| t.as_str()), Some("Relationship"));
        assert!(rel.get("objectList").is_none());
        assert_eq!(rel.get("object").and_then(|v| v.as_str()), Some("urn:ngsi-ld:Room:1"));
    }

    #[test]
    fn v1_0_multi_attribute_returns_default_instance() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": [
                {
                    "type": "Property",
                    "value": 25.0,
                    "datasetId": "urn:ds:1"
                },
                {
                    "type": "Property",
                    "value": 30.0
                }
            ]
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_0);
        let temp = value.get("temperature").unwrap();
        // Should be an array with only the default instance (no datasetId)
        let arr = temp.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0].get("value").and_then(|v| v.as_f64()), Some(30.0));
    }

    #[test]
    fn adapts_entity_array() {
        let mut value = json!([
            {
                "id": "urn:ngsi-ld:Entity:1",
                "type": "TestEntity",
                "expiresAt": "2025-01-01T00:00:00Z"
            },
            {
                "id": "urn:ngsi-ld:Entity:2",
                "type": "TestEntity",
                "scope": ["/A"]
            }
        ]);
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_3);
        let arr = value.as_array().unwrap();
        assert!(arr[0].get("expiresAt").is_none());
        assert!(arr[1].get("scope").is_none());
    }

    #[test]
    fn v1_4_reformats_json_property_as_property() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "config": {
                "type": "JsonProperty",
                "json": {"key": "value"}
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_4);
        let config = value.get("config").unwrap();
        assert_eq!(config.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(config.get("json").is_none());
        assert!(config.get("value").is_some());
    }

    #[test]
    fn v1_4_reformats_list_property_as_property() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "tags": {
                "type": "ListProperty",
                "valueList": ["a", "b", "c"]
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_4);
        let tags = value.get("tags").unwrap();
        assert_eq!(tags.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(tags.get("valueList").is_none());
        assert_eq!(tags.get("value").unwrap(), &json!(["a", "b", "c"]));
    }

    #[test]
    fn v1_4_removes_object_type_from_relationship() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Building:1",
                "objectType": "Building"
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_4);
        let rel = value.get("refBuilding").unwrap();
        assert!(rel.get("objectType").is_none());
    }

    #[test]
    fn v1_4_removes_expires_at_from_relationship() {
        let mut value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Building:1",
                "expiresAt": "2025-01-01T00:00:00Z"
            }
        });
        adapt_payload_for_version(&mut value, NgsiLdVersion::V1_4);
        let rel = value.get("refBuilding").unwrap();
        assert!(rel.get("expiresAt").is_none());
    }
}
