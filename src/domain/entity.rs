use crate::domain::{attribute::AttributeValue, notification::SysAttrsSnapshot, serde_helpers, uri::Uri};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Single or multiple entity type values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EntityTypeValue {
    Single(String),
    Multiple(Vec<String>),
}

/// Single or multiple scope values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Scope {
    Single(String),
    Multiple(Vec<String>),
}

/// NGSI-LD Entity representation (ETSI GS CIM 009 clause 5.2.4, Table 5.2.4-1).
///
/// An Entity is the core informational unit in NGSI-LD, representing a
/// real-world object or concept. It has an identifier, one or more types,
/// optional scope, and a set of attributes (Properties, Relationships, etc.).
///
/// All spec-defined entity members are explicit fields. `@context` is excluded
/// as it is a transport concern handled by the JSON-LD engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: Uri,
    #[serde(rename = "type")]
    pub type_: EntityTypeValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,

    /// Entity expiration timestamp (clause 4.22).
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,

    /// Default geospatial property (clause 4.7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<AttributeValue>,

    /// Observation space geometry (clause 5.2.4).
    #[serde(rename = "observationSpace", skip_serializing_if = "Option::is_none")]
    pub observation_space: Option<AttributeValue>,

    /// Operation space geometry (clause 5.2.4).
    #[serde(rename = "operationSpace", skip_serializing_if = "Option::is_none")]
    pub operation_space: Option<AttributeValue>,

    /// System-generated creation timestamp (clause 4.8, Table 5.2.2-1).
    #[serde(rename = "createdAt", skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub created_at: Option<DateTime<Utc>>,

    /// System-generated modification timestamp (clause 4.8, Table 5.2.2-1).
    #[serde(rename = "modifiedAt", skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub modified_at: Option<DateTime<Utc>>,

    /// Deletion timestamp, output-only in notifications/temporal (clause 4.8).
    #[serde(rename = "deletedAt", skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub deleted_at: Option<DateTime<Utc>>,

    #[serde(flatten)]
    pub attrs: IndexMap<String, AttributeValue>,
}

impl Entity {
    /// Populate system attribute fields from a repository snapshot.
    ///
    /// Sets `created_at` and `modified_at` unconditionally. Sets `expires_at`
    /// only if the field is not already populated (user-provided value takes
    /// precedence).
    pub fn with_sys_attrs(mut self, snapshot: &SysAttrsSnapshot) -> Entity {
        self.created_at = Some(snapshot.created_at);
        self.modified_at = Some(snapshot.modified_at);
        if self.expires_at.is_none() {
            self.expires_at = snapshot.expires_at;
        }
        self
    }

    /// Convert the entity to a simplified key-value representation.
    pub fn simplify(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("id".to_string(), serde_json::json!(self.id));
        map.insert("type".to_string(), serde_json::json!(self.type_));

        if let Some(scope) = &self.scope {
            map.insert("scope".to_string(), serde_json::json!(scope));
        }

        if let Some(ref expires_at) = self.expires_at {
            map.insert(
                "expiresAt".to_string(),
                serde_json::json!(expires_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            );
        }

        if let Some(ref location) = self.location {
            map.insert("location".to_string(), simplify_attribute_value(location));
        }

        if let Some(ref observation_space) = self.observation_space {
            map.insert("observationSpace".to_string(), simplify_attribute_value(observation_space));
        }

        if let Some(ref operation_space) = self.operation_space {
            map.insert("operationSpace".to_string(), simplify_attribute_value(operation_space));
        }

        if let Some(ref created_at) = self.created_at {
            map.insert(
                "createdAt".to_string(),
                serde_json::json!(created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            );
        }

        if let Some(ref modified_at) = self.modified_at {
            map.insert(
                "modifiedAt".to_string(),
                serde_json::json!(modified_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            );
        }

        if let Some(ref deleted_at) = self.deleted_at {
            map.insert(
                "deletedAt".to_string(),
                serde_json::json!(deleted_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
            );
        }

        for (key, attr) in &self.attrs {
            map.insert(key.clone(), simplify_attribute_value(attr));
        }

        serde_json::Value::Object(map)
    }
}

/// Simplify a single `AttributeValue` to its key-value form.
fn simplify_attribute_value(attr: &AttributeValue) -> serde_json::Value {
    match attr {
        AttributeValue::Single(a) => a.value(),
        AttributeValue::Multiple(list) => {
            // Dataset map format: {"dataset": {"datasetId1": value1, "@none": value2}}
            let mut dataset_map = serde_json::Map::new();
            for attr in list {
                let ds_key = attr.dataset_id_string().unwrap_or_else(|| "@none".to_string());
                dataset_map.insert(ds_key, attr.value());
            }
            serde_json::json!({"dataset": dataset_map})
        }
        AttributeValue::Raw(v) => {
            // For language-negotiated or other raw values that still
            // have an NGSI-LD attribute envelope, extract the value.
            if let Some(obj) = v.as_object() {
                match obj.get("type").and_then(|t| t.as_str()) {
                    Some("Property") | Some("GeoProperty") | Some("VocabProperty") => obj.get("value").cloned().unwrap_or_else(|| v.clone()),
                    Some("Relationship") => obj.get("object").cloned().unwrap_or_else(|| v.clone()),
                    Some("ListProperty") => obj.get("valueList").cloned().unwrap_or_else(|| v.clone()),
                    Some("ListRelationship") => obj.get("objectList").cloned().unwrap_or_else(|| v.clone()),
                    Some("LanguageProperty") | Some("JsonProperty") => v.clone(),
                    _other => v.clone(),
                }
            } else {
                v.clone()
            }
        }
    }
}

/// Partial entity representation for merge/update operations.
///
/// According to NGSI-LD spec, merge and update operations accept a partial
/// entity representation where `id` and `type` are optional (since `id` is
/// in the URL path and `type` cannot be changed).
///
/// System-managed timestamps (`createdAt`, `modifiedAt`, `deletedAt`) are
/// excluded as they are not patchable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uri>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub type_: Option<EntityTypeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,

    /// Entity expiration timestamp (clause 4.22).
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none", with = "serde_helpers::rfc3339_option", default)]
    pub expires_at: Option<DateTime<Utc>>,

    /// Default geospatial property (clause 4.7).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<AttributeValue>,

    /// Observation space geometry (clause 5.2.4).
    #[serde(rename = "observationSpace", skip_serializing_if = "Option::is_none")]
    pub observation_space: Option<AttributeValue>,

    /// Operation space geometry (clause 5.2.4).
    #[serde(rename = "operationSpace", skip_serializing_if = "Option::is_none")]
    pub operation_space: Option<AttributeValue>,

    #[serde(flatten)]
    pub attrs: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use super::{Entity, EntityPatch};
    use crate::domain::{attribute::AttributeValue, notification::SysAttrsSnapshot};
    use chrono::{Datelike, TimeZone, Utc};
    use serde_json::json;

    #[test]
    fn entity_deserializes_with_scope_string() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:001",
            "type": "Building",
            "scope": "/Madrid/Gardens/ParqueNorte"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with scope");
        assert!(entity.scope.is_some());
    }

    #[test]
    fn entity_deserializes_multi_instance_attribute_array() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:002",
            "type": "Building",
            "name": [
                { "type": "Property", "value": "Eiffel Tower" },
                { "type": "Property", "value": "Tour Eiffel", "datasetId": "urn:ngsi-ld:Dataset:french-name" }
            ]
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with array attrs");

        assert!(matches!(entity.attrs.get("name"), Some(AttributeValue::Multiple(_))));
    }

    #[test]
    fn entity_deserializes_language_and_json_property() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:003",
            "type": "Building",
            "street": {
                "type": "LanguageProperty",
                "languageMap": { "en": "Main Street", "de": "Hauptstrasse" }
            },
            "jsonProperty": {
                "type": "JsonProperty",
                "json": { "id": "some-id", "ok": true }
            }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with new attrs");
        assert!(entity.attrs.contains_key("street"));
        assert!(entity.attrs.contains_key("jsonProperty"));
    }

    #[test]
    fn entity_deserializes_concise_attribute_values() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:004",
            "type": "Building",
            "A1": "value of A1",
            "A2": 42
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with concise attrs");
        assert!(matches!(entity.attrs.get("A1"), Some(AttributeValue::Raw(serde_json::Value::String(_)))));
        assert!(matches!(entity.attrs.get("A2"), Some(AttributeValue::Raw(serde_json::Value::Number(_)))));
    }

    #[test]
    fn entity_deserializes_with_expires_at() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:005",
            "type": "Building",
            "expiresAt": "2030-12-31T23:59:59Z"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with expiresAt");
        assert!(entity.expires_at.is_some());
        assert_eq!(entity.expires_at.unwrap().year(), 2030);
        assert!(!entity.attrs.contains_key("expiresAt"));
    }

    #[test]
    fn entity_deserializes_with_location() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:006",
            "type": "Building",
            "location": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [13.3986, 52.5547] }
            }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with location");
        assert!(entity.location.is_some());
        assert!(!entity.attrs.contains_key("location"));
    }

    #[test]
    fn entity_deserializes_with_observation_space() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:007",
            "type": "Building",
            "observationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Polygon", "coordinates": [[[0,0],[1,0],[1,1],[0,1],[0,0]]] }
            }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with observationSpace");
        assert!(entity.observation_space.is_some());
        assert!(!entity.attrs.contains_key("observationSpace"));
    }

    #[test]
    fn entity_deserializes_with_operation_space() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:008",
            "type": "Building",
            "operationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [8.6845, 50.1109] }
            }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with operationSpace");
        assert!(entity.operation_space.is_some());
        assert!(!entity.attrs.contains_key("operationSpace"));
    }

    #[test]
    fn entity_deserializes_with_sys_attrs() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:009",
            "type": "Building",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-06-15T12:30:00Z"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with sys attrs");
        assert!(entity.created_at.is_some());
        assert!(entity.modified_at.is_some());
        assert!(!entity.attrs.contains_key("createdAt"));
        assert!(!entity.attrs.contains_key("modifiedAt"));
    }

    #[test]
    fn entity_serializes_omits_none_fields() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:010",
            "type": "Building",
            "name": { "type": "Property", "value": "Test" }
        });

        let entity: Entity = serde_json::from_value(payload).expect("basic entity");
        let serialized = serde_json::to_value(&entity).expect("serialize");
        let obj = serialized.as_object().expect("is object");

        assert!(!obj.contains_key("expiresAt"));
        assert!(!obj.contains_key("location"));
        assert!(!obj.contains_key("observationSpace"));
        assert!(!obj.contains_key("operationSpace"));
        assert!(!obj.contains_key("createdAt"));
        assert!(!obj.contains_key("modifiedAt"));
        assert!(!obj.contains_key("deletedAt"));
    }

    #[test]
    fn entity_simplify_includes_new_fields() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:011",
            "type": "Building",
            "expiresAt": "2030-12-31T23:59:59Z",
            "location": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [1.0, 2.0] }
            },
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-06-15T12:30:00Z"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with fields");
        let simplified = entity.simplify();
        let obj = simplified.as_object().expect("is object");

        assert!(obj.contains_key("expiresAt"));
        assert!(obj.contains_key("location"));
        assert!(obj.contains_key("createdAt"));
        assert!(obj.contains_key("modifiedAt"));
    }

    #[test]
    fn entity_special_fields_not_in_attrs() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:012",
            "type": "Building",
            "expiresAt": "2030-12-31T23:59:59Z",
            "location": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [1.0, 2.0] }
            },
            "observationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [3.0, 4.0] }
            },
            "operationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [5.0, 6.0] }
            },
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-06-15T12:30:00Z",
            "deletedAt": "2024-12-31T23:59:59Z",
            "temperature": { "type": "Property", "value": 25 }
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with all fields");

        // Named fields must NOT leak into the flatten map
        assert!(!entity.attrs.contains_key("expiresAt"));
        assert!(!entity.attrs.contains_key("location"));
        assert!(!entity.attrs.contains_key("observationSpace"));
        assert!(!entity.attrs.contains_key("operationSpace"));
        assert!(!entity.attrs.contains_key("createdAt"));
        assert!(!entity.attrs.contains_key("modifiedAt"));
        assert!(!entity.attrs.contains_key("deletedAt"));

        // Regular attributes still go into attrs
        assert!(entity.attrs.contains_key("temperature"));

        // Named fields are populated
        assert!(entity.expires_at.is_some());
        assert!(entity.location.is_some());
        assert!(entity.observation_space.is_some());
        assert!(entity.operation_space.is_some());
        assert!(entity.created_at.is_some());
        assert!(entity.modified_at.is_some());
        assert!(entity.deleted_at.is_some());
    }

    #[test]
    fn entity_patch_deserializes_new_fields() {
        let payload = json!({
            "expiresAt": "2030-12-31T23:59:59Z",
            "location": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [1.0, 2.0] }
            },
            "observationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [3.0, 4.0] }
            },
            "operationSpace": {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [5.0, 6.0] }
            },
            "temperature": { "type": "Property", "value": 25 }
        });

        let patch: EntityPatch = serde_json::from_value(payload).expect("entity patch with new fields");

        assert!(patch.expires_at.is_some());
        assert!(patch.location.is_some());
        assert!(patch.observation_space.is_some());
        assert!(patch.operation_space.is_some());
        assert!(patch.attrs.contains_key("temperature"));
        assert!(!patch.attrs.contains_key("expiresAt"));
        assert!(!patch.attrs.contains_key("location"));
    }

    #[test]
    fn entity_with_sys_attrs_populates_timestamps() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:013",
            "type": "Building"
        });

        let entity: Entity = serde_json::from_value(payload).expect("basic entity");
        assert!(entity.created_at.is_none());
        assert!(entity.modified_at.is_none());

        let snapshot = SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            modified_at: Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap(),
            expires_at: Some(Utc.with_ymd_and_hms(2030, 12, 31, 23, 59, 59).unwrap()),
        };

        let entity = entity.with_sys_attrs(&snapshot);
        assert_eq!(entity.created_at, Some(snapshot.created_at));
        assert_eq!(entity.modified_at, Some(snapshot.modified_at));
        assert_eq!(entity.expires_at, Some(Utc.with_ymd_and_hms(2030, 12, 31, 23, 59, 59).unwrap()));
    }

    #[test]
    fn entity_with_sys_attrs_preserves_existing_expires_at() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:014",
            "type": "Building",
            "expiresAt": "2025-06-01T00:00:00Z"
        });

        let entity: Entity = serde_json::from_value(payload).expect("entity with expiresAt");
        let original_expires = entity.expires_at;

        let snapshot = SysAttrsSnapshot {
            created_at: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
            modified_at: Utc.with_ymd_and_hms(2024, 6, 15, 12, 30, 0).unwrap(),
            expires_at: Some(Utc.with_ymd_and_hms(2099, 1, 1, 0, 0, 0).unwrap()),
        };

        let entity = entity.with_sys_attrs(&snapshot);
        // User-provided expiresAt is preserved, not overwritten by snapshot
        assert_eq!(entity.expires_at, original_expires);
    }
}
