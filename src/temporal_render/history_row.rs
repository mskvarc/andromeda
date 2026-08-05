//! Decoding an `attribute_history` row into an expanded attribute instance.
//!
//! The temporal API answers with the expanded JSON-LD form of an attribute
//! instance, and the store keeps one instance per row with the value split
//! across seven typed columns. Turning one into the other is this module, and
//! it is the last place in the presentation layer that names a `PgRow` — the
//! store gains a neutral row type when the temporal ports are declared, and
//! this becomes a decoding of that instead.

use crate::{
    core::error::AndromedaError,
    repository::temporal::{entry::TeaEntry, value_type::ValueType},
    temporal_render::geojson_ld::expand_geojson,
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, rfc3339::whole_seconds};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
use andromeda_postgresql_encoding::ewkb::Ewkb;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;

/// The observed time span of a set of history rows.
pub type TimeRange = (DateTime<Utc>, DateTime<Utc>);

pub fn parse_tea_entry(row: &PgRow) -> Result<TeaEntry, AndromedaError> {
    let attribute_name_raw: String = row.try_get("attribute_name")?;
    let attribute_name = IriBuf::new(attribute_name_raw.clone())
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal attribute IRI '{}': {}", attribute_name_raw, e)))?;

    let attribute_type_raw: String = row.try_get("attribute_type")?;
    let attribute_type = AttributeType::from_str(&attribute_type_raw)
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal attribute type '{}': {}", attribute_type_raw, e)))?;

    let dataset_id = row
        .try_get::<Option<String>, _>("dataset_id")?
        .map(|raw| IriBuf::new(raw.clone()).map_err(|e| AndromedaError::internal(format!("Invalid temporal dataset id '{}': {}", raw, e))))
        .transpose()?;

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

/// Decide which typed column actually holds this row's value.
///
/// The catalogue's `value_type` is the *declared* type and can disagree with the
/// row in front of us. Recording an attribute deletion used to rewrite the TEA's
/// `value_type` to `String` (the tombstone sentinel lives in `value_string`),
/// and because rendering dispatches on that type, every pre-deletion numeric
/// observation of the attribute then rendered with no value at all — the
/// `String` arm asked for `value_string`, which is NULL on a numeric row. The
/// recorder no longer rewrites the type, but databases written by earlier builds
/// still carry rewritten rows, so detection is column-driven here rather than
/// trusting the catalogue.
///
/// `chk_attribute_history_*` guarantees exactly one value column is populated,
/// so the populated column identifies the type unambiguously. The declared type
/// is only a fallback for a row with no value at all.
fn effective_value_type(row: &PgRow, declared: ValueType) -> ValueType {
    if row.try_get::<Option<f64>, _>("value_number").ok().flatten().is_some() {
        return ValueType::Number;
    }
    if row.try_get::<Option<bool>, _>("value_bool").ok().flatten().is_some() {
        return ValueType::Boolean;
    }
    if row.try_get::<Option<DateTime<Utc>>, _>("value_datetime").ok().flatten().is_some() {
        return ValueType::DateTime;
    }
    if row.try_get::<Option<String>, _>("value_uri").ok().flatten().is_some() {
        return ValueType::Uri;
    }
    if row.try_get::<Option<Value>, _>("value_object").ok().flatten().is_some() {
        return ValueType::Object;
    }
    if row.try_get::<Option<Vec<u8>>, _>("value_geo_ewkb").ok().flatten().is_some() {
        return ValueType::Geometry;
    }
    if row.try_get::<Option<String>, _>("value_string").ok().flatten().is_some() {
        return ValueType::String;
    }
    declared
}

pub fn history_row_to_expanded_instance(row: &PgRow, tea: &TeaEntry, sys_attrs: SysAttrsInclusion) -> Result<Value, AndromedaError> {
    let mut instance = serde_json::Map::new();

    instance.insert("@type".to_string(), json!([tea.attribute_type.iri().as_str()]));

    let value_key = tea.attribute_type.value_key_str();

    let is_tombstone = row.try_get::<Option<String>, _>("value_string").ok().flatten().as_deref() == Some(expanded_properties::NULL_URI.as_str());

    if is_tombstone {
        if tea.attribute_name == expanded_properties::SCOPE.as_str() {
            instance.insert(value_key.to_string(), json!([]));
        } else if tea.attribute_type == AttributeType::Relationship {
            instance.insert(value_key.to_string(), json!([{"@id": expanded_properties::NULL_URI.as_str()}]));
        } else if tea.attribute_type == AttributeType::LanguageProperty {
            instance.insert(
                value_key.to_string(),
                json!([{"@value": expanded_properties::NULL_URI.as_str(), "@language": "@none"}]),
            );
        } else {
            instance.insert(value_key.to_string(), json!([{"@value": expanded_properties::NULL_URI.as_str()}]));
        }
        if let Ok(time) = row.try_get::<DateTime<Utc>, _>("time") {
            instance.insert(
                expanded_properties::DELETED_AT.as_str().to_string(),
                json!([{"@value": whole_seconds(time), "@type": expanded_classes::DATE_TIME.as_str()}]),
            );
        }
        if tea.attribute_name != expanded_properties::SCOPE.as_str() {
            let instance_id: String = row.try_get("instance_id")?;
            instance.insert(expanded_properties::INSTANCE_ID.as_str().to_string(), json!([{"@id": instance_id}]));
        }
        if let Some(ds_id) = &tea.dataset_id {
            instance.insert(expanded_properties::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id.as_str()}]));
        }
        return Ok(Value::Object(instance));
    }

    let effective_value_type = effective_value_type(row, tea.value_type);

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
                let ewkb = Ewkb::try_from(bytes.as_slice()).map_err(|e| AndromedaError::internal(format!("Failed to read temporal geometry EWKB: {}", e)))?;
                let geometry = Geometry::try_from(&ewkb)
                    .map_err(|e| AndromedaError::internal(format!("Failed to decode temporal geometry EWKB: {}", e)))?
                    .to_geojson();
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
                    json!([{"@value": whole_seconds(v), "@type": expanded_classes::DATE_TIME.as_str()}]),
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
            expanded_properties::OBSERVED_AT.as_str().to_string(),
            json!([{"@value": whole_seconds(observed_at), "@type": expanded_classes::DATE_TIME.as_str()}]),
        );
    }

    if sys_attrs == SysAttrsInclusion::Include
        && let Ok(time) = row.try_get::<DateTime<Utc>, _>("time")
    {
        instance.insert(
            expanded_properties::CREATED_AT.as_str().to_string(),
            json!([{"@value": whole_seconds(time), "@type": expanded_classes::DATE_TIME.as_str()}]),
        );
    }

    let instance_id: String = row.try_get("instance_id")?;
    instance.insert(expanded_properties::INSTANCE_ID.as_str().to_string(), json!([{"@id": instance_id}]));

    if let Some(ds_id) = &tea.dataset_id {
        instance.insert(expanded_properties::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id.as_str()}]));
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
pub fn extract_typed_agg(row: &PgRow, value_type: ValueType, num_col: &str, str_col: &str, dt_col: &str) -> Value {
    match value_type {
        ValueType::String => row.try_get::<String, _>(str_col).map_or(Value::Null, |v| json!(v)),
        ValueType::DateTime => row.try_get::<DateTime<Utc>, _>(dt_col).map_or(Value::Null, |dt| json!(whole_seconds(dt))),
        ValueType::Number | ValueType::Boolean | ValueType::Object => row.try_get::<f64, _>(num_col).map_or(Value::Null, |v| json!(v)),
        ValueType::Uri | ValueType::Geometry => Value::Null,
    }
}
