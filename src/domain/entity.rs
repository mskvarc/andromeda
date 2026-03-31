use crate::domain::{attribute::AttributeValue, uri::Uri};
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

/// NGSI-LD Entity representation (ETSI GS CIM 009 clause 5.2.4).
///
/// An Entity is the core informational unit in NGSI-LD, representing a
/// real-world object or concept. It has an identifier, one or more types,
/// optional scope, and a set of attributes (Properties, Relationships, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: Uri,
    #[serde(rename = "type")]
    pub type_: EntityTypeValue,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(flatten)]
    pub attrs: IndexMap<String, AttributeValue>,
}

impl Entity {
    /// Convert the entity to a simplified key-value representation.
    pub fn simplify(&self) -> serde_json::Value {
        let mut map = serde_json::Map::new();
        map.insert("id".to_string(), serde_json::json!(self.id));
        map.insert("type".to_string(), serde_json::json!(self.type_));

        if let Some(scope) = &self.scope {
            map.insert("scope".to_string(), serde_json::json!(scope));
        }

        for (key, attr) in &self.attrs {
            match attr {
                AttributeValue::Single(a) => {
                    map.insert(key.clone(), a.value());
                }
                AttributeValue::Multiple(list) => {
                    // Dataset map format: {"dataset": {"datasetId1": value1, "@none": value2}}
                    let mut dataset_map = serde_json::Map::new();
                    for attr in list {
                        let ds_key = attr.dataset_id_string().unwrap_or_else(|| "@none".to_string());
                        dataset_map.insert(ds_key, attr.value());
                    }
                    let wrapper = serde_json::json!({"dataset": dataset_map});
                    map.insert(key.clone(), wrapper);
                }
                AttributeValue::Raw(v) => {
                    // For language-negotiated or other raw values that still
                    // have an NGSI-LD attribute envelope, extract the value.
                    let simplified = if let Some(obj) = v.as_object() {
                        match obj.get("type").and_then(|t| t.as_str()) {
                            Some("Property") | Some("GeoProperty") | Some("VocabProperty") => obj.get("value").cloned().unwrap_or_else(|| v.clone()),
                            Some("Relationship") => obj.get("object").cloned().unwrap_or_else(|| v.clone()),
                            Some("ListProperty") => obj.get("valueList").cloned().unwrap_or_else(|| v.clone()),
                            Some("ListRelationship") => obj.get("objectList").cloned().unwrap_or_else(|| v.clone()),
                            _ => v.clone(),
                        }
                    } else {
                        v.clone()
                    };
                    map.insert(key.clone(), simplified);
                }
            }
        }

        serde_json::Value::Object(map)
    }
}

/// Partial entity representation for merge/update operations.
///
/// According to NGSI-LD spec, merge and update operations accept a partial
/// entity representation where `id` and `type` are optional (since `id` is
/// in the URL path and `type` cannot be changed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uri>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub type_: Option<EntityTypeValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    #[serde(flatten)]
    pub attrs: IndexMap<String, AttributeValue>,
}

#[cfg(test)]
mod tests {
    use super::Entity;
    use crate::domain::attribute::AttributeValue;
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
}
