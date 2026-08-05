//! Temporal values renderer (NGSI-LD clause 4.5.5).
//!
//! [`TemporalValuesRenderer`] transforms a compacted temporal entity into the
//! `temporalValues` simplified representation.

use crate::{core::error::AndromedaError, renderer::Renderer};
use andromeda_domain::attribute_type::AttributeType;
use indexmap::IndexMap;
use serde_json::{Value, json};

/// Transforms a compacted temporal entity into `temporalValues` form
/// (NGSI-LD clause 4.5.5).
///
/// Each attribute is collapsed into `[value, observedAt]` pairs grouped by
/// dataset ID. Reserved entity-level keys (`id`, `type`, `@context`,
/// timestamps) pass through unchanged.
#[derive(Debug)]
pub struct TemporalValuesRenderer {
    payload: Value,
}

impl TemporalValuesRenderer {
    /// Create a renderer for the given compacted payload.
    pub const fn new(payload: Value) -> TemporalValuesRenderer {
        TemporalValuesRenderer { payload }
    }
}

impl Renderer for TemporalValuesRenderer {
    type Output = Value;

    fn render(self) -> Result<Value, AndromedaError> {
        let obj = match self.payload {
            Value::Object(m) => m,
            other => return Ok(other),
        };

        const RESERVED: &[&str] = &["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"];

        let mut result = serde_json::Map::new();

        for key in RESERVED {
            if let Some(v) = obj.get(*key) {
                result.insert(key.to_string(), v.clone());
            }
        }

        for (key, value) in &obj {
            if RESERVED.contains(&key.as_str()) {
                continue;
            }

            let instances = match value {
                Value::Array(arr) => arr,
                other => {
                    result.insert(key.clone(), other.clone());
                    continue;
                }
            };

            if instances.is_empty() {
                continue;
            }

            let mut groups: IndexMap<Option<String>, Vec<&Value>> = IndexMap::new();
            for inst in instances {
                let dataset_id = inst.get("datasetId").and_then(|v| v.as_str()).map(|s| s.to_string());
                groups.entry(dataset_id).or_default().push(inst);
            }

            let mut tv_objects: Vec<Value> = Vec::new();

            for (dataset_id, insts) in &groups {
                let first = insts[0];
                let attr_type = first.get("type").and_then(|v| v.as_str()).unwrap_or("Property");

                let parsed_attr_type: Option<AttributeType> = attr_type.parse().ok();
                let (value_key, extract_key) = parsed_attr_type.map_or(("values", "value"), |t| t.temporal_values_keys());

                let mut pairs: Vec<Value> = Vec::new();
                for inst in insts {
                    let val = inst.get(extract_key).cloned().unwrap_or(Value::Null);
                    let val = if matches!(
                        parsed_attr_type,
                        Some(AttributeType::LanguageProperty) | Some(AttributeType::JsonProperty) | Some(AttributeType::VocabProperty)
                    ) {
                        json!({extract_key: val})
                    } else {
                        val
                    };
                    let obs = inst.get("observedAt").or_else(|| inst.get("deletedAt")).cloned().unwrap_or(Value::Null);
                    pairs.push(json!([val, obs]));
                }

                let mut tv_obj = serde_json::Map::new();
                tv_obj.insert("type".to_string(), json!(attr_type));
                if let Some(ds_id) = dataset_id {
                    tv_obj.insert("datasetId".to_string(), json!(ds_id));
                }
                tv_obj.insert(value_key.to_string(), Value::Array(pairs));
                tv_objects.push(Value::Object(tv_obj));
            }

            if tv_objects.len() == 1 {
                if let Some(single) = tv_objects.into_iter().next() {
                    result.insert(key.clone(), single);
                }
            } else {
                result.insert(key.clone(), Value::Array(tv_objects));
            }
        }

        Ok(Value::Object(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transform(input: Value) -> Value {
        TemporalValuesRenderer::new(input).render().expect("render")
    }

    #[test]
    fn temporal_values_single_property() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 25, "observedAt": "2025-01-01T00:00:00Z"},
                {"type": "Property", "value": 30, "observedAt": "2025-01-01T01:00:00Z"}
            ]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["speed"]["type"], "Property");
        let values = result["speed"]["values"].as_array().expect("values array");
        pretty_assertions::assert_eq!(values.len(), 2);
        pretty_assertions::assert_eq!(values[0][0], 25);
        pretty_assertions::assert_eq!(values[1][0], 30);
    }

    #[test]
    fn temporal_values_multi_dataset() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 25, "observedAt": "t1", "datasetId": "urn:ds:1"},
                {"type": "Property", "value": 30, "observedAt": "t2", "datasetId": "urn:ds:2"}
            ]
        });
        let result = transform(input);
        let arr = result["speed"].as_array().expect("multi-dataset array");
        pretty_assertions::assert_eq!(arr.len(), 2);
    }

    #[test]
    fn temporal_values_preserves_reserved() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "@context": "https://example.org/ctx",
            "speed": [{"type": "Property", "value": 10, "observedAt": "t1"}]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["id"], "urn:x");
        pretty_assertions::assert_eq!(result["type"], "Vehicle");
        pretty_assertions::assert_eq!(result["@context"], "https://example.org/ctx");
    }

    #[test]
    fn temporal_values_relationship() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "isParked": [
                {"type": "Relationship", "object": "urn:p1", "observedAt": "t1"},
                {"type": "Relationship", "object": "urn:p2", "observedAt": "t2"}
            ]
        });
        let result = transform(input);
        pretty_assertions::assert_eq!(result["isParked"]["type"], "Relationship");
        let objects = result["isParked"]["objects"].as_array().expect("objects array");
        pretty_assertions::assert_eq!(objects.len(), 2);
        pretty_assertions::assert_eq!(objects[0][0], "urn:p1");
    }
}
