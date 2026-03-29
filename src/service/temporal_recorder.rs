//! Shared temporal write-path logic for Entity API and Temporal API.
//!
//! Records attribute history whenever entities are created or modified.

use crate::{error::AndromedaError, repository::temporal::TemporalRepository};
use chrono::{DateTime, Utc};
use ngsi_ld_types::iri;
use serde_json::Value;
use std::sync::Arc;

/// NGSI-LD system-level expanded IRIs that should NOT be treated as user attributes.
///
/// IMPORTANT: This array MUST be kept sorted for binary_search.
const NGSI_LD_SYSTEM_ATTRS: &[&str] = &[
    iri::CREATED_AT,
    iri::DATASET_ID,
    iri::DELETED_AT,
    iri::INSTANCE_ID,
    iri::MODIFIED_AT,
    iri::OBSERVED_AT,
    iri::UNIT_CODE,
];

/// Named parameters for temporal value storage, replacing the 7-element tuple.
pub(crate) struct DbValueParams {
    pub value_number: Option<f64>,
    pub value_string: Option<String>,
    pub value_bool: Option<bool>,
    pub value_geo: Option<String>,
    pub value_object: Option<Value>,
    pub value_datetime: Option<DateTime<Utc>>,
    pub value_uri: Option<String>,
}

pub struct TemporalRecorder {
    repo: Arc<TemporalRepository>,
}

impl TemporalRecorder {
    pub fn new(repo: Arc<TemporalRepository>) -> Self {
        Self { repo }
    }

    /// Look up TEA entries for a given entity + attribute IRI.
    pub async fn get_teas_for_entity_attr(&self, entity_id: &str, attr_iri: &str) -> Result<Vec<crate::domain::temporal::TeaEntry>, AndromedaError> {
        self.repo.get_teas_for_entity_attr(entity_id, attr_iri).await
    }

    /// Record all attributes from an expanded JSON-LD entity into temporal tables.
    ///
    /// Invalid attributes are skipped with a warning log rather than failing
    /// the entire operation. This allows valid attributes to be recorded even
    /// when some attributes have issues.
    pub async fn record_entity_attributes(&self, entity_id: &str, expanded: &Value) -> Result<(), AndromedaError> {
        let obj = match expanded {
            Value::Array(arr) => arr.first().and_then(|v| v.as_object()),
            Value::Object(obj) => Some(obj),
            _ => None,
        };

        let Some(obj) = obj else {
            return Ok(());
        };

        for (attr_name, attr_value) in obj {
            if attr_name.starts_with('@') {
                continue;
            }

            // Skip NGSI-LD system-level attributes (observedAt, createdAt, etc.)
            if NGSI_LD_SYSTEM_ATTRS.binary_search(&attr_name.as_str()).is_ok() {
                continue;
            }

            // Handle scope specially: scope entries are @value nodes, not typed attribute instances
            if attr_name == iri::SCOPE {
                if let Some(scope_arr) = attr_value.as_array() {
                    for scope_entry in scope_arr {
                        if let Some(scope_val) = scope_entry.as_object().and_then(|o| o.get("@value")).and_then(|v| v.as_str()) {
                            // Record as a synthetic Property-like entry
                            let synthetic = serde_json::json!({
                                "@type": [iri::PROPERTY],
                                iri::HAS_VALUE: [{"@value": scope_val}]
                            });
                            match self.record_attribute_instance(entity_id, attr_name, &synthetic).await {
                                Ok(()) => {}
                                Err(e) => {
                                    tracing::warn!("Failed to record scope temporal data for entity {}: {}", entity_id, e.message());
                                }
                            }
                        }
                    }
                }
                continue;
            }

            let instances = match attr_value {
                Value::Array(arr) => arr,
                _ => continue,
            };

            for instance in instances {
                if let Some(inst_obj) = instance.as_object() {
                    // Skip instances that don't look like NGSI-LD attribute instances
                    // (they must have an @type indicating Property, Relationship, etc.)
                    if !is_ngsi_ld_attribute_instance(inst_obj) {
                        continue;
                    }

                    match self.record_attribute_instance(entity_id, attr_name, &Value::Object(inst_obj.clone())).await {
                        Ok(()) => {}
                        Err(e) => {
                            tracing::warn!("Failed to record temporal data for entity {}: {}", entity_id, e.message());
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Record a single expanded attribute instance.
    pub async fn record_attribute_instance(&self, entity_id: &str, attr_name: &str, instance: &Value) -> Result<(), AndromedaError> {
        let attribute_type = detect_attribute_type(instance);
        let typed_value = extract_typed_value(instance, attribute_type)?;
        let value_type = typed_value.value_type();
        let dataset_id = extract_dataset_id(instance);
        let observed_at_str = extract_observed_at(instance);
        let instance_id = extract_or_generate_instance_id(instance);
        let metadata = collect_metadata(instance);

        let time = observed_at_str
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)).ok())
            .unwrap_or_else(Utc::now);

        let observed_at = observed_at_str
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).map(|dt| dt.with_timezone(&Utc)).ok());

        let tea_id = self
            .repo
            .upsert_tea(entity_id, attr_name, attribute_type, dataset_id.as_deref(), value_type)
            .await?;

        let p = typed_value.to_db_params();

        self.repo
            .insert_history(
                time,
                tea_id,
                p.value_number,
                p.value_string.as_deref(),
                p.value_bool,
                p.value_geo.as_deref(),
                p.value_object.as_ref(),
                p.value_datetime,
                p.value_uri.as_deref(),
                &instance_id,
                observed_at,
                metadata.as_ref(),
            )
            .await?;

        Ok(())
    }

    /// Record a tombstone for a deleted attribute (soft-delete).
    pub async fn record_attribute_deletion(&self, entity_id: &str, attr_iri: &str, dataset_id: Option<&str>) -> Result<(), AndromedaError> {
        // Look up the existing TEA to preserve the attribute type
        let teas = self.repo.get_teas_for_entity_attr(entity_id, attr_iri).await?;
        let attr_type = teas
            .iter()
            .find(|t| t.dataset_id.as_deref() == dataset_id)
            .or_else(|| teas.first())
            .map(|t| t.attribute_type.as_str())
            .unwrap_or(iri::PROPERTY_SHORT);

        let tea_id = self.repo.upsert_tea(entity_id, attr_iri, attr_type, dataset_id, "String").await?;

        let now = Utc::now();
        let instance_id = format!("urn:ngsi-ld:attribute:instance:{}", uuid::Uuid::new_v4());

        self.repo
            .insert_history(
                now,
                tea_id,
                None,
                Some("urn:ngsi-ld:null"),
                None,
                None,
                None,
                None,
                None,
                &instance_id,
                None,
                None,
            )
            .await?;

        Ok(())
    }

    /// Update an existing attribute instance (full replacement, preserves createdAt).
    pub async fn update_attribute_instance(&self, entity_id: &str, attr_name: &str, instance_id: &str, instance: &Value) -> Result<(), AndromedaError> {
        let attribute_type = detect_attribute_type(instance);
        let typed_value = extract_typed_value(instance, attribute_type)?;
        let observed_at = extract_observed_at(instance).and_then(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok());
        let metadata = collect_metadata(instance);

        let p = typed_value.to_db_params();

        self.repo
            .update_instance(
                instance_id,
                entity_id,
                attr_name,
                p.value_number,
                p.value_string.as_deref(),
                p.value_bool,
                p.value_geo.as_deref(),
                p.value_object.as_ref(),
                p.value_datetime,
                p.value_uri.as_deref(),
                observed_at,
                metadata.as_ref(),
            )
            .await
    }
}

// -----------------------------------------------------------------------------
// JSON-LD value extraction (shared with TemporalService)
// -----------------------------------------------------------------------------

/// Check if an expanded JSON-LD object is a recognized NGSI-LD attribute instance
/// (Property, Relationship, GeoProperty, VocabProperty, LanguageProperty, or JsonProperty).
fn is_ngsi_ld_attribute_instance(obj: &serde_json::Map<String, Value>) -> bool {
    let Some(types) = obj.get("@type").and_then(|v| v.as_array()) else {
        return false;
    };
    types.iter().any(|t| {
        t.as_str().is_some_and(|s| {
            matches!(
                s,
                iri::PROPERTY | iri::RELATIONSHIP | iri::GEO_PROPERTY | iri::VOCAB_PROPERTY | iri::LANGUAGE_PROPERTY | iri::JSON_PROPERTY
            )
        })
    })
}

fn detect_attribute_type(instance: &Value) -> &'static str {
    let empty: Vec<serde_json::Value> = vec![];
    let types = instance.get("@type").and_then(|v| v.as_array()).unwrap_or(&empty);

    for t in types {
        if let Some(s) = t.as_str() {
            match s {
                iri::PROPERTY => return iri::PROPERTY_SHORT,
                iri::RELATIONSHIP => return iri::RELATIONSHIP_SHORT,
                iri::GEO_PROPERTY => return iri::GEO_PROPERTY_SHORT,
                iri::VOCAB_PROPERTY => return iri::VOCAB_PROPERTY_SHORT,
                iri::LANGUAGE_PROPERTY => return iri::LANGUAGE_PROPERTY_SHORT,
                iri::JSON_PROPERTY => return iri::JSON_PROPERTY_SHORT,
                _ => {}
            }
        }
    }
    iri::PROPERTY_SHORT
}

#[derive(Debug)]
enum TypedValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Geometry(String),
    Object(Value),
    DateTime(String),
    Uri(String),
}

impl TypedValue {
    fn value_type(&self) -> &'static str {
        match self {
            Self::Number(_) => "Number",
            Self::String(_) => "String",
            Self::Boolean(_) => "Boolean",
            Self::Geometry(_) => "Geometry",
            Self::Object(_) => "Object",
            Self::DateTime(_) => "DateTime",
            Self::Uri(_) => "Uri",
        }
    }

    fn to_db_params(self) -> DbValueParams {
        let mut p = DbValueParams {
            value_number: None,
            value_string: None,
            value_bool: None,
            value_geo: None,
            value_object: None,
            value_datetime: None,
            value_uri: None,
        };
        match self {
            Self::Number(n) => p.value_number = Some(n),
            Self::String(s) => p.value_string = Some(s),
            Self::Boolean(b) => p.value_bool = Some(b),
            Self::Geometry(g) => p.value_geo = Some(g),
            Self::Object(o) => p.value_object = Some(o),
            Self::DateTime(s) => {
                p.value_datetime = DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)).ok();
            }
            Self::Uri(u) => p.value_uri = Some(u),
        }
        p
    }
}

fn extract_typed_value(instance: &Value, attribute_type: &str) -> Result<TypedValue, AndromedaError> {
    match attribute_type {
        iri::RELATIONSHIP_SHORT => {
            let uri = instance
                .get(iri::HAS_OBJECT)
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@id"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| AndromedaError::bad_request_data("Relationship missing object"))?;
            Ok(TypedValue::Uri(uri.to_string()))
        }
        iri::GEO_PROPERTY_SHORT => {
            let geo_node = instance
                .get(iri::HAS_VALUE)
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| AndromedaError::bad_request_data("GeoProperty missing value"))?;

            // In expanded JSON-LD, GeoJSON may appear as either:
            // 1. A literal @value (string or object) — e.g. {"@value": "{\"type\":\"Point\",...}"}
            // 2. An expanded JSON-LD node (GeoJSON gets expanded by the processor)
            //    e.g. {"@type": ["https://purl.org/geojson/vocab#Point"],
            //           "https://purl.org/geojson/vocab#coordinates": [{"@list": [...]}]}
            let geo_str = if let Some(val) = geo_node.get("@value") {
                // Case 1: literal value
                if val.is_string() {
                    val.as_str().unwrap().to_string()
                } else {
                    val.to_string()
                }
            } else {
                // Case 2: expanded GeoJSON node — reconstruct compact GeoJSON
                reconstruct_geojson(geo_node)
            };
            Ok(TypedValue::Geometry(geo_str))
        }
        iri::VOCAB_PROPERTY_SHORT => {
            // VocabProperty uses hasVocab with @id nodes
            let arr = instance
                .get(iri::HAS_VOCAB)
                .and_then(|v| v.as_array())
                .ok_or_else(|| AndromedaError::bad_request_data("VocabProperty missing vocab value"))?;
            if arr.len() > 1 {
                // Multi-value vocab: collect all @id values into a JSON array
                let values: Vec<Value> = arr.iter().filter_map(|obj| obj.get("@id").or(obj.get("@value")).cloned()).collect();
                Ok(TypedValue::Object(Value::Array(values)))
            } else {
                let vocab = arr
                    .first()
                    .and_then(|obj| obj.get("@id").or(obj.get("@value")))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AndromedaError::bad_request_data("VocabProperty missing vocab value"))?;
                Ok(TypedValue::String(vocab.to_string()))
            }
        }
        iri::LANGUAGE_PROPERTY_SHORT => {
            // LanguageProperty uses hasLanguageMap with @language containers
            let lang_map = instance
                .get(iri::HAS_LANGUAGE_MAP)
                .ok_or_else(|| AndromedaError::bad_request_data("LanguageProperty missing languageMap"))?;
            Ok(TypedValue::Object(lang_map.clone()))
        }
        iri::JSON_PROPERTY_SHORT => {
            // JsonProperty uses hasJSON with @json typed value
            let json_val = instance
                .get(iri::HAS_JSON)
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|obj| obj.get("@value"))
                .ok_or_else(|| AndromedaError::bad_request_data("JsonProperty missing json value"))?;
            Ok(TypedValue::Object(json_val.clone()))
        }
        _ => {
            // Property — uses hasValue
            let has_value = instance
                .get(iri::HAS_VALUE)
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .ok_or_else(|| AndromedaError::bad_request_data("Property missing value"))?;

            // Value may be a literal (@value node) or an expanded JSON-LD node (object values)
            if let Some(value) = has_value.get("@value") {
                let type_hint = has_value.get("@type").and_then(|t| t.as_str());

                if type_hint == Some(iri::DATETIME_TYPE) || type_hint == Some("http://www.w3.org/2001/XMLSchema#dateTime") {
                    if let Some(s) = value.as_str() {
                        return Ok(TypedValue::DateTime(s.to_string()));
                    }
                }

                match value {
                    Value::Number(n) => Ok(TypedValue::Number(n.as_f64().unwrap_or(0.0))),
                    Value::Bool(b) => Ok(TypedValue::Boolean(*b)),
                    Value::String(s) => Ok(TypedValue::String(s.clone())),
                    Value::Object(_) | Value::Array(_) => Ok(TypedValue::Object(value.clone())),
                    Value::Null => Ok(TypedValue::String("null".to_string())),
                }
            } else {
                // Expanded JSON-LD node without @value — store as Object
                Ok(TypedValue::Object(has_value.clone()))
            }
        }
    }
}

/// Reconstruct compact GeoJSON from an expanded JSON-LD GeoJSON node.
///
/// Expanded GeoJSON has structure like:
/// ```json
/// {
///   "@type": ["https://purl.org/geojson/vocab#Point"],
///   "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 16.37}, {"@value": 48.20}]}]
/// }
/// ```
/// We reconstruct: `{"type": "Point", "coordinates": [16.37, 48.20]}`
pub(crate) fn reconstruct_geojson(node: &Value) -> String {
    const GEOJSON_VOCAB: &str = "https://purl.org/geojson/vocab#";
    const GEOJSON_COORDINATES: &str = "https://purl.org/geojson/vocab#coordinates";

    let Some(obj) = node.as_object() else {
        return node.to_string();
    };

    // Extract the geometry type from @type
    let geo_type = obj
        .get("@type")
        .and_then(|t| t.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .and_then(|s| s.strip_prefix(GEOJSON_VOCAB))
        .unwrap_or("Point");

    // Extract coordinates from the expanded form
    let coordinates = obj
        .get(GEOJSON_COORDINATES)
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("@list").or(Some(v)))
        .map(|list| extract_coordinates(list))
        .unwrap_or(Value::Array(vec![]));

    let result = serde_json::json!({
        "type": geo_type,
        "coordinates": coordinates
    });
    result.to_string()
}

/// Recursively extract coordinate values from expanded JSON-LD @list structures.
pub(crate) fn extract_coordinates(value: &Value) -> Value {
    match value {
        Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().map(|v| extract_coordinates(v)).collect();
            Value::Array(items)
        }
        Value::Object(obj) => {
            if let Some(val) = obj.get("@value") {
                val.clone()
            } else if let Some(list) = obj.get("@list") {
                extract_coordinates(list)
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

fn extract_observed_at(instance: &Value) -> Option<String> {
    instance
        .get(iri::OBSERVED_AT)
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@value"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn extract_dataset_id(instance: &Value) -> Option<String> {
    instance
        .get(iri::DATASET_ID)
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@id"))
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn extract_or_generate_instance_id(instance: &Value) -> String {
    instance
        .get(iri::INSTANCE_ID)
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|obj| obj.get("@id"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("urn:ngsi-ld:attribute:instance:{}", uuid::Uuid::new_v4()))
}

/// Keys to skip when collecting sub-attribute metadata from an expanded instance.
/// These are either JSON-LD keywords or NGSI-LD value/system properties that are
/// stored in dedicated columns rather than the metadata blob.
///
/// IMPORTANT: This array MUST be kept sorted for binary_search.
const METADATA_SKIP_KEYS: &[&str] = &[
    "@id",
    "@type",
    iri::DATASET_ID,
    iri::HAS_JSON,
    iri::HAS_LANGUAGE_MAP,
    iri::HAS_OBJECT,
    iri::HAS_VALUE,
    iri::HAS_VOCAB,
    iri::INSTANCE_ID,
    iri::OBSERVED_AT,
];

fn collect_metadata(instance: &Value) -> Option<Value> {
    let obj = instance.as_object()?;

    let metadata: serde_json::Map<String, Value> = obj
        .iter()
        .filter(|(k, _)| METADATA_SKIP_KEYS.binary_search(&k.as_str()).is_err())
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    if metadata.is_empty() { None } else { Some(Value::Object(metadata)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_skip_keys_is_sorted() {
        for window in METADATA_SKIP_KEYS.windows(2) {
            assert!(window[0] < window[1], "METADATA_SKIP_KEYS must be sorted: {:?} >= {:?}", window[0], window[1]);
        }
    }

    #[test]
    fn ngsi_ld_system_attrs_is_sorted() {
        for window in NGSI_LD_SYSTEM_ATTRS.windows(2) {
            assert!(window[0] < window[1], "NGSI_LD_SYSTEM_ATTRS must be sorted: {:?} >= {:?}", window[0], window[1]);
        }
    }
}
