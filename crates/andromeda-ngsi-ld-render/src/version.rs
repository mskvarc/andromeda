//! Payload adaptation for NGSI-LD version conformance (clause 4.3.6.8).
//!
//! When a client sends `Prefer: ngsi-ld=<version>`, the broker responds with
//! a 203 Non-Authoritative Information and adapts the payload to remove or
//! reformat fields that did not exist in the requested version.

use andromeda_ngsi_ld_http_headers::version::NgsiLdVersion;
use serde_json::Value;

/// Whether version adaptation was applied to a response payload.
///
/// Clause 4.3.6.8: a broker that adapts a payload to an older version is no
/// longer speaking authoritatively about it, so the response carries 203
/// rather than 200. The flag travels with the rendered output because the
/// status code is decided after rendering, by whoever writes the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionAdaptation {
    /// The payload was adapted to an older NGSI-LD version.
    Applied,
    /// No version adaptation was performed.
    None,
}

/// Adapt a serialized NGSI-LD entity payload to conform to an older spec version.
///
/// Applies the fallback tables from clause 4.3.6.8:
/// - Table 4.3.6.8-1: entity-level removals
/// - Table 4.3.6.8-2: property-level removals
/// - Table 4.3.6.8-3: relationship-level removals
///
/// Handles both single entities (JSON objects) and entity arrays.
#[derive(Debug)]
pub struct VersionRenderer {
    /// The spec version the payload is adapted down to.
    target: NgsiLdVersion,
    /// The payload being adapted.
    payload: Value,
}

impl VersionRenderer {
    /// Create a renderer targeting the given NGSI-LD version with an empty payload.
    pub const fn new(target: NgsiLdVersion) -> VersionRenderer {
        VersionRenderer { target, payload: Value::Null }
    }

    /// Set the payload value to adapt.
    pub fn with_payload(mut self, payload: Value) -> VersionRenderer {
        self.payload = payload;
        self
    }

    /// Apply the clause 4.3.6.8 fallback tables and return the adapted payload
    /// together with whether adaptation was performed.
    ///
    /// The [`VersionAdaptation`] the caller receives drives the status code: a
    /// requested version that is the current one or newer needs no adaptation,
    /// so the payload is returned unchanged with [`VersionAdaptation::None`] and
    /// the response stays 200. Only a target below the conformant version runs
    /// the fallback tables, which makes the representation non-authoritative
    /// (203). Removing members cannot fail, so this answers with a value rather
    /// than a `Result` no caller could act on.
    pub fn render(self) -> (Value, VersionAdaptation) {
        let VersionRenderer { target, mut payload } = self;
        if target >= NgsiLdVersion::CONFORMANT {
            return (payload, VersionAdaptation::None);
        }

        match &mut payload {
            Value::Object(_) => adapt_entity(&mut payload, target),
            Value::Array(items) => {
                for item in items {
                    adapt_entity(item, target);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
        (payload, VersionAdaptation::Applied)
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
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
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
        // Unknown attribute type -- no adaptation needed
        Some(_) | None => {}
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

    fn adapt(target: NgsiLdVersion, value: Value) -> Value {
        VersionRenderer::new(target).with_payload(value).render().0
    }

    #[test]
    fn a_conformant_or_newer_target_reports_no_adaptation() {
        // Clause 4.3.6.8: the payload is already authoritative for the current
        // version or newer, so no adaptation is performed and the response is
        // 200, not 203. This is what the short-circuit must report.
        let payload = json!({"id": "urn:x", "type": "T", "expiresAt": "2025-01-01T00:00:00Z"});
        let (_, adaptation) = VersionRenderer::new(NgsiLdVersion::CONFORMANT).with_payload(payload).render();
        pretty_assertions::assert_eq!(adaptation, VersionAdaptation::None);
    }

    #[test]
    fn a_target_below_the_conformant_version_reports_adaptation() {
        let payload = json!({"id": "urn:x", "type": "T", "expiresAt": "2025-01-01T00:00:00Z"});
        let (_, adaptation) = VersionRenderer::new(NgsiLdVersion::V1_4).with_payload(payload).render();
        pretty_assertions::assert_eq!(adaptation, VersionAdaptation::Applied);
    }

    #[test]
    fn no_adaptation_for_conformant_version() {
        let value = json!({
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
        let adapted = adapt(NgsiLdVersion::V1_9, value);
        pretty_assertions::assert_eq!(adapted, original);
    }

    #[test]
    fn v1_8_removes_entity_expires_at() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "expiresAt": "2025-01-01T00:00:00Z",
            "temperature": {
                "type": "Property",
                "value": 25.0
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_8, value);
        assert!(adapted.get("expiresAt").is_none());
    }

    #[test]
    fn v1_3_removes_scope_from_entity() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "scope": ["/A/B"],
            "temperature": {
                "type": "Property",
                "value": 25.0
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_3, value);
        assert!(adapted.get("scope").is_none());
    }

    #[test]
    fn v1_0_removes_dataset_id_and_observed_at_from_property() {
        let value = json!({
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
        let adapted = adapt(NgsiLdVersion::V1_0, value);
        let temp = adapted.get("temperature").unwrap();
        assert!(temp.get("datasetId").is_none());
        assert!(temp.get("observedAt").is_none());
        assert!(temp.get("unitCode").is_none());
    }

    #[test]
    fn v1_0_removes_dataset_id_and_observed_at_from_relationship() {
        let value = json!({
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
        let adapted = adapt(NgsiLdVersion::V1_0, value);
        let rel = adapted.get("refBuilding").unwrap();
        assert!(rel.get("datasetId").is_none());
        assert!(rel.get("observedAt").is_none());
        assert!(rel.get("objectType").is_none());
    }

    #[test]
    fn v1_3_reformats_language_property_as_property() {
        let value = json!({
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
        let adapted = adapt(NgsiLdVersion::V1_3, value);
        let name = adapted.get("name").unwrap();
        pretty_assertions::assert_eq!(name.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(name.get("languageMap").is_none());
        assert!(name.get("value").is_some());
    }

    #[test]
    fn v1_4_removes_expires_at_and_value_type_from_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "temperature": {
                "type": "Property",
                "value": 25.0,
                "expiresAt": "2025-01-01T00:00:00Z",
                "valueType": "Number"
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_4, value);
        let temp = adapted.get("temperature").unwrap();
        assert!(temp.get("expiresAt").is_none());
        assert!(temp.get("valueType").is_none());
    }

    #[test]
    fn v1_4_reformats_vocab_property_as_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "category": {
                "type": "VocabProperty",
                "vocab": "commercial"
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_4, value);
        let cat = adapted.get("category").unwrap();
        pretty_assertions::assert_eq!(cat.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(cat.get("vocab").is_none());
        pretty_assertions::assert_eq!(cat.get("value").and_then(|v| v.as_str()), Some("commercial"));
    }

    #[test]
    fn v1_4_reformats_list_relationship_as_relationship() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refRooms": {
                "type": "ListRelationship",
                "objectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_4, value);
        let rel = adapted.get("refRooms").unwrap();
        pretty_assertions::assert_eq!(rel.get("type").and_then(|t| t.as_str()), Some("Relationship"));
        assert!(rel.get("objectList").is_none());
        pretty_assertions::assert_eq!(rel.get("object").and_then(|v| v.as_str()), Some("urn:ngsi-ld:Room:1"));
    }

    #[test]
    fn v1_0_multi_attribute_returns_default_instance() {
        let value = json!({
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
        let adapted = adapt(NgsiLdVersion::V1_0, value);
        let temp = adapted.get("temperature").unwrap();
        let arr = temp.as_array().unwrap();
        pretty_assertions::assert_eq!(arr.len(), 1);
        pretty_assertions::assert_eq!(arr[0].get("value").and_then(|v| v.as_f64()), Some(30.0));
    }

    #[test]
    fn adapts_entity_array() {
        let value = json!([
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
        let adapted = adapt(NgsiLdVersion::V1_3, value);
        let arr = adapted.as_array().unwrap();
        assert!(arr[0].get("expiresAt").is_none());
        assert!(arr[1].get("scope").is_none());
    }

    #[test]
    fn v1_4_reformats_json_property_as_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "config": {
                "type": "JsonProperty",
                "json": {"key": "value"}
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_4, value);
        let config = adapted.get("config").unwrap();
        pretty_assertions::assert_eq!(config.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(config.get("json").is_none());
        assert!(config.get("value").is_some());
    }

    #[test]
    fn v1_4_reformats_list_property_as_property() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "tags": {
                "type": "ListProperty",
                "valueList": ["a", "b", "c"]
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_4, value);
        let tags = adapted.get("tags").unwrap();
        pretty_assertions::assert_eq!(tags.get("type").and_then(|t| t.as_str()), Some("Property"));
        assert!(tags.get("valueList").is_none());
        pretty_assertions::assert_eq!(tags.get("value").unwrap(), &json!(["a", "b", "c"]));
    }

    #[test]
    fn v1_4_removes_object_type_from_relationship() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Building:1",
                "objectType": "Building"
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_4, value);
        let rel = adapted.get("refBuilding").unwrap();
        assert!(rel.get("objectType").is_none());
    }

    #[test]
    fn v1_4_removes_expires_at_from_relationship() {
        let value = json!({
            "id": "urn:ngsi-ld:Entity:1",
            "type": "TestEntity",
            "refBuilding": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Building:1",
                "expiresAt": "2025-01-01T00:00:00Z"
            }
        });
        let adapted = adapt(NgsiLdVersion::V1_4, value);
        let rel = adapted.get("refBuilding").unwrap();
        assert!(rel.get("expiresAt").is_none());
    }
}
