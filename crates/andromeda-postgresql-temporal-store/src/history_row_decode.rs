//! Decoding `sqlx::postgres::PgRow` into the backend-neutral
//! [`TemporalHistoryRow`] and [`TemporalAggregationRow`] types declared by the
//! temporal store port.
//!
//! Every column probe, EWKB-to-GeoJSON conversion, tombstone detection, and
//! legacy `effective_value_type` heuristic lives here so that neither the
//! service layer nor the renderers ever import `sqlx`.

use crate::{error::TemporalStoreError, mapper::row_to_tea, value_type::ValueType};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, instance_id::InstanceId, uri::Uri};
use andromeda_ngsi_ld_model::temporal::{AggregationMinMax, TemporalAggregationRow, TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_postgresql_encoding::ewkb::Ewkb;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;

/// Decode one `attribute_history` row into a [`TemporalHistoryRow`].
///
/// Mirrors the column-reading logic previously spread across
/// `parse_tea_entry` and `history_row_to_expanded_instance`, but returns
/// the neutral row type instead of expanded JSON. The EWKB-to-GeoJSON
/// conversion and the legacy `effective_value_type` heuristic are handled
/// here so the renderer never touches a database type.
pub(crate) fn decode_history_row(row: &PgRow) -> Result<TemporalHistoryRow, TemporalStoreError> {
    let tea = row_to_tea(row)?;

    let is_tombstone = row.try_get::<Option<String>, _>("value_string").ok().flatten().as_deref() == Some(expanded_properties::NULL_URI.as_str());

    let value = if is_tombstone {
        TemporalValue::Tombstone
    } else {
        decode_typed_value(row, tea.value_type)?
    };

    let observed_at = row.try_get::<Option<DateTime<Utc>>, _>("observed_at").unwrap_or(None);
    let time: DateTime<Utc> = row.try_get("time").map_err(|e| TemporalStoreError::Decode(e.to_string()))?;
    let instance_id_raw: String = row.try_get("instance_id").map_err(|e| TemporalStoreError::Decode(e.to_string()))?;
    let instance_id = instance_id_raw
        .parse::<InstanceId>()
        .map_err(|error| TemporalStoreError::Decode(format!("Invalid attribute_history.instance_id '{instance_id_raw}': {error}")))?;
    let metadata: Option<Value> = row.try_get("metadata").unwrap_or(None);
    let partition_total: Option<i64> = row.try_get("partition_total").ok();

    Ok(TemporalHistoryRow {
        attribute_name: tea.attribute_name,
        attribute_type: tea.attribute_type,
        dataset_id: tea.dataset_id,
        value,
        observed_at,
        time,
        instance_id,
        metadata,
        partition_total,
    })
}

/// Decode one aggregation row into a [`TemporalAggregationRow`].
pub(crate) fn decode_aggregation_row(row: &PgRow) -> Result<TemporalAggregationRow, TemporalStoreError> {
    let attribute_name_raw: String = row.try_get("attribute_name").map_err(|e| TemporalStoreError::Decode(e.to_string()))?;
    let attribute_name = attribute_name_raw
        .parse::<AttributeName>()
        .map_err(|error| TemporalStoreError::Decode(format!("Invalid aggregation attribute_name '{attribute_name_raw}': {error}")))?;
    let attribute_type_raw: String = row.try_get("attribute_type").map_err(|e| TemporalStoreError::Decode(e.to_string()))?;
    let attribute_type = AttributeType::from_str(&attribute_type_raw)
        .map_err(|error| TemporalStoreError::Decode(format!("Invalid aggregation attribute_type '{attribute_type_raw}': {error}")))?;
    let dataset_id = row
        .try_get::<Option<String>, _>("dataset_id")
        .map_err(|error| TemporalStoreError::Decode(error.to_string()))?
        .map(|raw| {
            raw.parse::<Uri>()
                .map_err(|error| TemporalStoreError::Decode(format!("Invalid aggregation dataset_id '{raw}': {error}")))
        })
        .transpose()?;

    let value_type_raw: String = row.try_get("value_type").map_err(|e| TemporalStoreError::Decode(e.to_string()))?;
    let value_type = ValueType::from_str(&value_type_raw).map_err(|e| TemporalStoreError::Decode(format!("Invalid value type '{}': {}", value_type_raw, e)))?;

    let bucket_start: Option<DateTime<Utc>> = row.try_get("bucket_start").ok();
    let bucket_end: Option<DateTime<Utc>> = row.try_get("bucket_end").ok();

    Ok(TemporalAggregationRow {
        attribute_name,
        attribute_type,
        dataset_id,
        bucket_start,
        bucket_end,
        avg: row.try_get("agg_avg").ok(),
        min: decode_agg_min_max(row, value_type, "agg_min", "agg_min_str", "agg_min_dt"),
        max: decode_agg_min_max(row, value_type, "agg_max", "agg_max_str", "agg_max_dt"),
        sum: row.try_get("agg_sum").ok(),
        sumsq: row.try_get("agg_sumsq").ok(),
        stddev: row.try_get("agg_stddev").ok(),
        total_count: row.try_get("agg_total_count").ok(),
        distinct_count: row.try_get("agg_distinct_count").ok(),
    })
}

/// Decide which typed column actually holds this row's value and decode it.
///
/// See the neutral temporal renderer for the
/// rationale: the catalogue's `value_type` can disagree with the populated
/// column on legacy rows, so detection is column-driven.
fn decode_typed_value(row: &PgRow, declared: ValueType) -> Result<TemporalValue, TemporalStoreError> {
    if let Some(v) = row.try_get::<Option<f64>, _>("value_number").ok().flatten() {
        return Ok(TemporalValue::Number(v));
    }
    if let Some(v) = row.try_get::<Option<bool>, _>("value_bool").ok().flatten() {
        return Ok(TemporalValue::Boolean(v));
    }
    if let Some(v) = row.try_get::<Option<DateTime<Utc>>, _>("value_datetime").ok().flatten() {
        return Ok(TemporalValue::DateTime(v));
    }
    if let Some(v) = row.try_get::<Option<String>, _>("value_uri").ok().flatten() {
        let uri = v
            .parse::<Uri>()
            .map_err(|error| TemporalStoreError::Decode(format!("Invalid attribute_history.value_uri '{v}': {error}")))?;
        return Ok(TemporalValue::Uri(uri));
    }
    if let Some(v) = row.try_get::<Option<Value>, _>("value_object").ok().flatten() {
        return Ok(TemporalValue::Object(v));
    }
    if let Some(bytes) = row.try_get::<Option<Vec<u8>>, _>("value_geo_ewkb").ok().flatten() {
        let ewkb = Ewkb::try_from(bytes.as_slice()).map_err(|e| TemporalStoreError::Decode(format!("EWKB read: {e}")))?;
        let geometry = andromeda_ngsi_ld_geometry::geometry::Geometry::try_from(&ewkb)
            .map_err(|e| TemporalStoreError::Decode(format!("EWKB decode: {e}")))?
            .to_geojson();
        let geo_json = serde_json::to_value(&geometry).map_err(|e| TemporalStoreError::Decode(format!("GeoJSON serialize: {e}")))?;
        return Ok(TemporalValue::Geometry(geo_json));
    }
    if let Some(v) = row.try_get::<Option<String>, _>("value_string").ok().flatten() {
        return Ok(TemporalValue::String(v));
    }

    Err(TemporalStoreError::Decode(format!(
        "attribute_history row declares {declared} but contains no typed value"
    )))
}

/// Decode a typed min/max aggregation column into [`AggregationMinMax`].
fn decode_agg_min_max(row: &PgRow, value_type: ValueType, num_col: &str, str_col: &str, dt_col: &str) -> Option<AggregationMinMax> {
    match value_type {
        ValueType::String => row.try_get::<String, _>(str_col).ok().map(AggregationMinMax::String),
        ValueType::DateTime => row.try_get::<DateTime<Utc>, _>(dt_col).ok().map(AggregationMinMax::DateTime),
        ValueType::Number | ValueType::Boolean | ValueType::Object => row.try_get::<f64, _>(num_col).ok().map(AggregationMinMax::Number),
        ValueType::Uri | ValueType::Geometry => None,
    }
}
