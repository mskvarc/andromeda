//! Projecting one backend-neutral temporal observation into an expanded attribute instance.

use crate::temporal::geojson_ld::expand_geojson;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, rfc3339::whole_seconds};
use andromeda_ngsi_ld_model::temporal::{TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// The observed time span of a set of history rows.
pub type TimeRange = (DateTime<Utc>, DateTime<Utc>);

/// Build an expanded NGSI-LD attribute instance from a backend-neutral
/// [`TemporalHistoryRow`].
///
/// The row already carries decoded, strongly typed values. The only work left is the JSON-LD structural expansion that the specification prescribes.
pub fn neutral_row_to_expanded_instance(row: &TemporalHistoryRow, sys_attrs: SysAttrsInclusion) -> Value {
    let mut instance = serde_json::Map::new();

    instance.insert("@type".to_string(), json!([row.attribute_type.iri().as_str()]));

    let value_key = row.attribute_type.value_key_str();

    if matches!(row.value, TemporalValue::Tombstone) {
        if row.attribute_name.as_str() == expanded_properties::SCOPE.as_str() {
            instance.insert(value_key.to_string(), json!([]));
        } else if row.attribute_type == AttributeType::Relationship {
            instance.insert(value_key.to_string(), json!([{"@id": expanded_properties::NULL_URI.as_str()}]));
        } else if row.attribute_type == AttributeType::LanguageProperty {
            instance.insert(
                value_key.to_string(),
                json!([{"@value": expanded_properties::NULL_URI.as_str(), "@language": "@none"}]),
            );
        } else {
            instance.insert(value_key.to_string(), json!([{"@value": expanded_properties::NULL_URI.as_str()}]));
        }
        instance.insert(
            expanded_properties::DELETED_AT.as_str().to_string(),
            json!([{"@value": whole_seconds(row.time), "@type": expanded_classes::DATE_TIME.as_str()}]),
        );
        if row.attribute_name.as_str() != expanded_properties::SCOPE.as_str() {
            instance.insert(expanded_properties::INSTANCE_ID.as_str().to_string(), json!([{"@id": &row.instance_id}]));
        }
        if let Some(ds_id) = &row.dataset_id {
            instance.insert(expanded_properties::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id.as_str()}]));
        }
        return Value::Object(instance);
    }

    match &row.value {
        TemporalValue::Number(v) => {
            let json_val = if v.fract() == 0.0 && *v >= i64::MIN as f64 && *v <= i64::MAX as f64 {
                json!(*v as i64)
            } else {
                json!(v)
            };
            instance.insert(value_key.to_string(), json!([{"@value": json_val}]));
        }
        TemporalValue::String(v) => {
            if row.attribute_type == AttributeType::VocabProperty {
                instance.insert(value_key.to_string(), json!([{"@id": v}]));
            } else {
                instance.insert(value_key.to_string(), json!([{"@value": v}]));
            }
        }
        TemporalValue::Boolean(v) => {
            instance.insert(value_key.to_string(), json!([{"@value": v}]));
        }
        TemporalValue::Geometry(geo) => {
            let expanded = expand_geojson(geo);
            instance.insert(value_key.to_string(), json!([expanded]));
        }
        TemporalValue::Object(v) => {
            if row.attribute_type == AttributeType::JsonProperty {
                instance.insert(value_key.to_string(), json!([{"@value": v, "@type": "@json"}]));
            } else if row.attribute_type == AttributeType::LanguageProperty {
                instance.insert(value_key.to_string(), v.clone());
            } else if row.attribute_type == AttributeType::VocabProperty {
                if let Some(arr) = v.as_array() {
                    let nodes: Vec<Value> = arr.iter().map(|val| json!({"@id": val})).collect();
                    instance.insert(value_key.to_string(), Value::Array(nodes));
                } else {
                    instance.insert(value_key.to_string(), json!([{"@id": v}]));
                }
            } else {
                instance.insert(value_key.to_string(), json!([{"@value": v}]));
            }
        }
        TemporalValue::DateTime(v) => {
            instance.insert(
                value_key.to_string(),
                json!([{"@value": whole_seconds(*v), "@type": expanded_classes::DATE_TIME.as_str()}]),
            );
        }
        TemporalValue::Uri(v) => {
            instance.insert(value_key.to_string(), json!([{"@id": v}]));
        }
        TemporalValue::Tombstone => {} // handled above
    }

    if let Some(observed_at) = row.observed_at {
        instance.insert(
            expanded_properties::OBSERVED_AT.as_str().to_string(),
            json!([{"@value": whole_seconds(observed_at), "@type": expanded_classes::DATE_TIME.as_str()}]),
        );
    }

    if sys_attrs == SysAttrsInclusion::Include {
        instance.insert(
            expanded_properties::CREATED_AT.as_str().to_string(),
            json!([{"@value": whole_seconds(row.time), "@type": expanded_classes::DATE_TIME.as_str()}]),
        );
    }

    instance.insert(expanded_properties::INSTANCE_ID.as_str().to_string(), json!([{"@id": &row.instance_id}]));

    if let Some(ds_id) = &row.dataset_id {
        instance.insert(expanded_properties::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id.as_str()}]));
    }

    if let Some(meta) = &row.metadata
        && let Some(obj) = meta.as_object()
    {
        for (k, v) in obj {
            instance.insert(k.clone(), v.clone());
        }
    }

    Value::Object(instance)
}
