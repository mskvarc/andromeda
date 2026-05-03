//! Private helpers shared across temporal renderers.

use crate::{
    core::error::AndromedaError,
    domain::{
        attribute_type::AttributeType,
        geometry::Wkb,
        temporal::{TeaEntry, ValueType},
    },
    jsonld::vocab,
    renderer::SysAttrsInclusion,
    service::temporal::format_datetime,
};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;
use urn_rs::Urn;

pub(crate) type TimeRange = (DateTime<Utc>, DateTime<Utc>);

pub(crate) fn parse_tea_entry(row: &PgRow) -> Result<TeaEntry, AndromedaError> {
    let attribute_name_raw: String = row.try_get("attribute_name")?;
    let attribute_name = IriBuf::new(attribute_name_raw.clone())
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal attribute IRI '{}': {}", attribute_name_raw, e)))?;

    let attribute_type_raw: String = row.try_get("attribute_type")?;
    let attribute_type = AttributeType::from_str(&attribute_type_raw)
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal attribute type '{}': {}", attribute_type_raw, e)))?;

    let dataset_id = row.try_get::<Option<String>, _>("dataset_id")?.map(|raw| Urn::from_str(&raw)).transpose()?;

    let value_type_raw: String = row.try_get("value_type")?;
    let value_type =
        ValueType::from_str(&value_type_raw).map_err(|e| AndromedaError::internal(format!("Invalid temporal value type '{}': {}", value_type_raw, e)))?;

    Ok(TeaEntry {
        attribute_name,
        attribute_type,
        dataset_id,
        value_type,
    })
}

pub(crate) fn history_row_to_expanded_instance(row: &PgRow, tea: &TeaEntry, sys_attrs: SysAttrsInclusion) -> Result<Value, AndromedaError> {
    let mut instance = serde_json::Map::new();

    instance.insert("@type".to_string(), json!([tea.attribute_type.iri().as_str()]));

    let value_key = tea.attribute_type.value_key_str();

    let is_tombstone = row.try_get::<Option<String>, _>("value_string").ok().flatten().as_deref() == Some(vocab::expanded::NULL_URI.as_str());

    if is_tombstone {
        if tea.attribute_name == vocab::expanded::SCOPE.as_str() {
            instance.insert(value_key.to_string(), json!([]));
        } else if tea.attribute_type == AttributeType::Relationship {
            instance.insert(value_key.to_string(), json!([{"@id": vocab::expanded::NULL_URI.as_str()}]));
        } else if tea.attribute_type == AttributeType::LanguageProperty {
            instance.insert(
                value_key.to_string(),
                json!([{"@value": vocab::expanded::NULL_URI.as_str(), "@language": "@none"}]),
            );
        } else {
            instance.insert(value_key.to_string(), json!([{"@value": vocab::expanded::NULL_URI.as_str()}]));
        }
        if let Ok(time) = row.try_get::<DateTime<Utc>, _>("time") {
            instance.insert(
                vocab::expanded::DELETED_AT.as_str().to_string(),
                json!([{"@value": format_datetime(time), "@type": vocab::expanded::DATE_TIME.as_str()}]),
            );
        }
        if tea.attribute_name != vocab::expanded::SCOPE.as_str() {
            let instance_id: String = row.try_get("instance_id")?;
            instance.insert(vocab::expanded::INSTANCE_ID.as_str().to_string(), json!([{"@id": instance_id}]));
        }
        if let Some(ds_id) = &tea.dataset_id {
            instance.insert(vocab::expanded::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id}]));
        }
        return Ok(Value::Object(instance));
    }

    let effective_value_type = if tea.value_type == ValueType::Object {
        match row.try_get::<Option<Value>, _>("value_object") {
            Ok(Some(_)) => ValueType::Object,
            Ok(None) | Err(_) => {
                if row.try_get::<Option<String>, _>("value_string").ok().flatten().is_some() {
                    ValueType::String
                } else if row.try_get::<Option<f64>, _>("value_number").ok().flatten().is_some() {
                    ValueType::Number
                } else {
                    ValueType::Object
                }
            }
        }
    } else {
        tea.value_type
    };

    match effective_value_type {
        ValueType::Number => {
            if let Ok(v) = row.try_get::<f64, _>("value_number") {
                let json_val = if v.fract() == 0.0 && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
                    json!(v as i64)
                } else {
                    json!(v)
                };
                instance.insert(value_key.to_string(), json!([{"@value": json_val}]));
            }
        }
        ValueType::String => {
            if let Ok(v) = row.try_get::<String, _>("value_string") {
                if tea.attribute_type == AttributeType::VocabProperty {
                    instance.insert(value_key.to_string(), json!([{"@id": v}]));
                } else {
                    instance.insert(value_key.to_string(), json!([{"@value": v}]));
                }
            }
        }
        ValueType::Boolean => {
            if let Ok(v) = row.try_get::<bool, _>("value_bool") {
                instance.insert(value_key.to_string(), json!([{"@value": v}]));
            }
        }
        ValueType::Geometry => {
            if let Ok(bytes) = row.try_get::<Vec<u8>, _>("value_geo_ewkb") {
                let wkb = Wkb::from_postgis_bytes(Bytes::from(bytes));
                let geometry = wkb
                    .to_geojson()
                    .map_err(|e| AndromedaError::internal(format!("Failed to decode temporal geometry EWKB: {}", e)))?;
                let geo = serde_json::to_value(&geometry).map_err(|e| AndromedaError::internal(format!("Failed to serialize temporal geometry: {}", e)))?;
                let expanded = expand_geojson(&geo);
                instance.insert(value_key.to_string(), json!([expanded]));
            }
        }
        ValueType::Object => {
            if let Ok(v) = row.try_get::<Value, _>("value_object") {
                if tea.attribute_type == AttributeType::JsonProperty {
                    instance.insert(value_key.to_string(), json!([{"@value": v, "@type": "@json"}]));
                } else if tea.attribute_type == AttributeType::LanguageProperty {
                    instance.insert(value_key.to_string(), v);
                } else if tea.attribute_type == AttributeType::VocabProperty {
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
        }
        ValueType::DateTime => {
            if let Ok(v) = row.try_get::<DateTime<Utc>, _>("value_datetime") {
                instance.insert(
                    value_key.to_string(),
                    json!([{"@value": format_datetime(v), "@type": vocab::expanded::DATE_TIME.as_str()}]),
                );
            }
        }
        ValueType::Uri => {
            if let Ok(v) = row.try_get::<String, _>("value_uri") {
                instance.insert(value_key.to_string(), json!([{"@id": v}]));
            }
        }
    }

    if let Ok(Some(observed_at)) = row.try_get::<Option<DateTime<Utc>>, _>("observed_at") {
        instance.insert(
            vocab::expanded::OBSERVED_AT.as_str().to_string(),
            json!([{"@value": format_datetime(observed_at), "@type": vocab::expanded::DATE_TIME.as_str()}]),
        );
    }

    if sys_attrs == SysAttrsInclusion::Include
        && let Ok(time) = row.try_get::<DateTime<Utc>, _>("time")
    {
        instance.insert(
            vocab::expanded::CREATED_AT.as_str().to_string(),
            json!([{"@value": format_datetime(time), "@type": vocab::expanded::DATE_TIME.as_str()}]),
        );
    }

    let instance_id: String = row.try_get("instance_id")?;
    instance.insert(vocab::expanded::INSTANCE_ID.as_str().to_string(), json!([{"@id": instance_id}]));

    if let Some(ds_id) = &tea.dataset_id {
        instance.insert(vocab::expanded::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id}]));
    }

    if let Ok(Some(meta)) = row.try_get::<Option<Value>, _>("metadata")
        && let Some(obj) = meta.as_object()
    {
        for (k, v) in obj {
            instance.insert(k.clone(), v.clone());
        }
    }

    Ok(Value::Object(instance))
}

/// Extract a min/max aggregation value based on the attribute's value type.
///
/// For String values, reads the lexicographic result from the text column.
/// For DateTime values, reads from the timestamptz column.
/// For numeric-compatible types (Number, Boolean, Object/Array), reads the float column.
/// Returns null for types where min/max is not applicable (Uri, Geometry).
pub(crate) fn extract_typed_agg(row: &PgRow, value_type: ValueType, num_col: &str, str_col: &str, dt_col: &str) -> Value {
    match value_type {
        ValueType::String => row.try_get::<String, _>(str_col).map_or(Value::Null, |v| json!(v)),
        ValueType::DateTime => row.try_get::<DateTime<Utc>, _>(dt_col).map_or(Value::Null, |dt| json!(format_datetime(dt))),
        ValueType::Number | ValueType::Boolean | ValueType::Object => row.try_get::<f64, _>(num_col).map_or(Value::Null, |v| json!(v)),
        ValueType::Uri | ValueType::Geometry => Value::Null,
    }
}

fn expand_geojson(geojson: &Value) -> Value {
    let Some(obj) = geojson.as_object() else {
        return geojson.clone();
    };

    let geo_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("Point");
    let coordinates = obj.get("coordinates").cloned().unwrap_or(json!([]));
    let expanded_coords = expand_coordinates(&coordinates);

    json!({
        "@type": [format!("{}{}", vocab::prefix::GEOJSON.as_str(), geo_type)],
        format!("{}coordinates", vocab::prefix::GEOJSON.as_str()): [{"@list": expanded_coords}]
    })
}

fn expand_coordinates(value: &Value) -> Value {
    match value {
        Value::Array(arr) => {
            if arr.first().is_some_and(|v| v.is_number()) {
                Value::Array(arr.iter().map(|v| json!({"@value": v})).collect())
            } else {
                Value::Array(arr.iter().map(|v| json!({"@list": expand_coordinates(v)})).collect())
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_) => json!({"@value": value}),
    }
}
