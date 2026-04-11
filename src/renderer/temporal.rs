//! Temporal response building: `build_entity_temporal`, `ensure_temporal_attrs_are_arrays`,
//! `transform_to_temporal_values`, `build_aggregated_data`, `expand_geojson`, `expand_coordinates`.

use crate::{
    domain::{
        attribute_type::AttributeType,
        entity::Entity,
        temporal::{AggrMethod, TeaEntry, TimeRel, ValueType},
        uri::Uri,
    },
    error::AndromedaError,
    jsonld::vocab,
    query::compiled::{CompiledTemporalQuery, LastNOrigin},
    renderer::SysAttrsInclusion,
    service::temporal::format_datetime,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use iref::IriBuf;
use serde_json::{Value, json};
use sqlx::{Row, postgres::PgRow};
use std::str::FromStr;

type TimeRange = (DateTime<Utc>, DateTime<Utc>);
type TemporalAttributeInstances = Vec<(Option<DateTime<Utc>>, Value)>;
type TemporalAttrMap = IndexMap<String, TemporalAttributeInstances>;

/// Result of [`build_entity_temporal`]: the assembled entity JSON together with
/// pagination metadata needed for Content-Range header construction.
#[derive(Debug)]
pub struct TemporalEntityBuildResult {
    /// The fully-assembled temporal entity JSON value (expanded form).
    pub entity: Value,
    /// Whether the result set was truncated by the system-applied lastN cap.
    pub truncated: bool,
    /// The observed time range across all attribute instances.
    pub time_range: Option<TimeRange>,
    /// The maximum partition total across all attributes (for Content-Range size).
    pub max_partition_total: Option<i64>,
}

/// After JSON-LD compaction, single-element arrays may be collapsed into plain objects.
/// NGSI-LD temporal attributes must always be arrays, so re-wrap them.
pub fn ensure_temporal_attrs_are_arrays(compacted: &mut Value) {
    const RESERVED: &[&str] = &["id", "type", "@context", "createdAt", "modifiedAt", "deletedAt"];

    let Some(obj) = compacted.as_object_mut() else {
        return;
    };

    for (key, value) in obj.iter_mut() {
        if RESERVED.contains(&key.as_str()) {
            continue;
        }
        if !value.is_array() {
            let taken = std::mem::replace(value, Value::Null);
            *value = Value::Array(vec![taken]);
        }
    }
}

/// Build a `Content-Range` header value for temporal pagination (NGSI-LD clause 6.3.10).
///
/// Returns `None` when no time range is available (empty result set).
/// The format is `date-time <start>-<end>/<size>` where size is `*` when the
/// total is unknown.
pub fn build_content_range(
    compiled: &CompiledTemporalQuery,
    time_range: Option<TimeRange>,
    partition_total: Option<i64>,
    last_n_origin: LastNOrigin,
) -> Option<String> {
    let (min_time, max_time) = time_range?;
    let lrt = format_datetime(min_time);
    let mrt = format_datetime(max_time);

    let (start, end, size) = if last_n_origin.is_user_specified() {
        let upper = match compiled.timerel {
            Some(TimeRel::Before) => compiled.time_at.map_or_else(|| mrt.clone(), format_datetime),
            Some(TimeRel::Between) => compiled.end_time_at.map_or_else(|| mrt.clone(), format_datetime),
            Some(TimeRel::After) => mrt,
            None => mrt,
        };
        let size = partition_total.map_or_else(|| "*".to_string(), |n| n.to_string());
        (upper, lrt, size)
    } else {
        let lower = match compiled.timerel {
            Some(_) => compiled.time_at.map(format_datetime).unwrap_or(lrt),
            None => lrt,
        };
        (lower, mrt, "*".to_string())
    };

    Some(format!("date-time {}-{}/{}", start, end, size))
}

/// Build a temporal entity representation from database rows.
///
/// Assembles attribute instances grouped by attribute name, applies time range
/// cutting for unsynchronized attributes (NGSI-LD clause 6.3.10), and returns
/// the entity JSON along with truncation/pagination metadata.
///
/// Returns `(entity_json, truncated, time_range, max_partition_total)`.
pub fn build_entity_temporal(
    entity_id: &str,
    entity_types: &[String],
    rows: &[PgRow],
    compiled: &CompiledTemporalQuery,
) -> Result<TemporalEntityBuildResult, AndromedaError> {
    let mut attrs: TemporalAttrMap = IndexMap::with_capacity(rows.len().min(64));
    let mut truncated = false;
    let mut min_time: Option<DateTime<Utc>> = None;
    let mut max_time: Option<DateTime<Utc>> = None;
    let mut max_partition_total: Option<i64> = None;

    for row in rows {
        let tea = parse_tea_entry(row)?;
        let attr_name = tea.attribute_name.as_str().to_string();

        if compiled.last_n_origin.is_system_capped()
            && let Some(last_n) = compiled.last_n
            && let Ok(total) = row.try_get::<i64, _>("partition_total")
        {
            max_partition_total = Some(max_partition_total.map_or(total, |m: i64| m.max(total)));
            if total > last_n as i64 {
                truncated = true;
            }
        }

        let obs = row
            .try_get::<Option<DateTime<Utc>>, _>("observed_at")
            .ok()
            .flatten()
            .or_else(|| row.try_get::<DateTime<Utc>, _>("time").ok());
        if let Some(t) = obs {
            min_time = Some(min_time.map_or(t, |m: DateTime<Utc>| m.min(t)));
            max_time = Some(max_time.map_or(t, |m: DateTime<Utc>| m.max(t)));
        }

        let instance = history_row_to_expanded_instance(row, &tea, compiled.sys_attrs)?;
        attrs.entry(attr_name).or_default().push((obs, instance));
    }

    // Time range cutting for unsynchronized attributes (NGSI-LD 6.3.10)
    if truncated && attrs.len() > 1 {
        let attr_ranges: Vec<(&String, DateTime<Utc>, DateTime<Utc>)> = attrs
            .iter()
            .filter_map(|(name, instances)| {
                let times: Vec<DateTime<Utc>> = instances.iter().filter_map(|(t, _)| *t).collect();
                let min = times.iter().min().copied()?;
                let max = times.iter().max().copied()?;
                Some((name, min, max))
            })
            .collect();

        if attr_ranges.len() > 1 {
            let global_min = attr_ranges.iter().map(|(_, min, _)| *min).min();
            let global_max = attr_ranges.iter().map(|(_, _, max)| *max).max();
            if let (Some(g_min), Some(g_max)) = (global_min, global_max) {
                let has_gap = attr_ranges.iter().any(|(_, a_min, _)| attr_ranges.iter().any(|(_, _, b_max)| *b_max < *a_min));

                if has_gap {
                    let cut_max = if compiled.last_n_origin.is_user_specified() { g_max } else { g_min };
                    let keep_range = attr_ranges.iter().find(|(_, min, max)| *min <= cut_max && cut_max <= *max);

                    if let Some((_, keep_min, keep_max)) = keep_range {
                        let (keep_min, keep_max) = (*keep_min, *keep_max);
                        for (_name, instances) in attrs.iter_mut() {
                            instances.retain(|(t, _)| t.is_some_and(|t| t >= keep_min && t <= keep_max));
                        }
                        min_time = Some(keep_min);
                        max_time = Some(keep_max);
                    }
                }
            }
        }
    }

    let mut entity = serde_json::Map::with_capacity(attrs.len() + 2);
    entity.insert("@id".to_string(), json!(entity_id));
    entity.insert("@type".to_string(), json!(entity_types));

    for (attr_name, instances) in attrs {
        let values: Vec<Value> = instances.into_iter().map(|(_, v)| v).collect();
        entity.insert(attr_name, Value::Array(values));
    }

    let time_range = min_time.zip(max_time);
    Ok(TemporalEntityBuildResult {
        entity: Value::Object(entity),
        truncated,
        time_range,
        max_partition_total,
    })
}

/// Build aggregated temporal data from database rows.
///
/// Groups rows by `(attribute_name, dataset_id)` and extracts aggregation
/// method values (avg, min, max, sum, etc.) with their bucket time ranges.
/// Returns an ordered map of attribute name to aggregated instance(s).
pub fn build_entity_temporal_aggregated_data(rows: &[PgRow], compiled: &CompiledTemporalQuery) -> Result<IndexMap<String, Value>, AndromedaError> {
    type GroupKey = (String, Option<String>);
    let mut groups: IndexMap<GroupKey, Vec<&PgRow>> = IndexMap::with_capacity(rows.len().min(64));

    for row in rows {
        let attr_name: String = row.try_get("attribute_name")?;
        let dataset_id: Option<String> = row.try_get("dataset_id")?;
        groups.entry((attr_name, dataset_id)).or_default().push(row);
    }

    let mut attrs: IndexMap<String, Vec<Value>> = IndexMap::with_capacity(groups.len());

    for ((attr_name, dataset_id), bucket_rows) in &groups {
        let mut instance = serde_json::Map::with_capacity(2 + compiled.aggr_methods.len());
        instance.insert("type".to_string(), json!("Property"));

        if let Some(ds_id) = dataset_id {
            instance.insert("datasetId".to_string(), json!(ds_id));
        }

        for method in &compiled.aggr_methods {
            let (key, pg_col) = match method {
                AggrMethod::Avg => ("avg", "agg_avg"),
                AggrMethod::Min => ("min", "agg_min"),
                AggrMethod::Max => ("max", "agg_max"),
                AggrMethod::Sum => ("sum", "agg_sum"),
                AggrMethod::SumSq => ("sumsq", "agg_sumsq"),
                AggrMethod::StdDev => ("stddev", "agg_stddev"),
                AggrMethod::TotalCount => ("totalCount", "agg_total_count"),
                AggrMethod::DistinctCount => ("distinctCount", "agg_distinct_count_num"),
            };

            let mut triples: Vec<Value> = Vec::with_capacity(bucket_rows.len());
            for row in bucket_rows {
                let val: Value = if *method == AggrMethod::TotalCount || *method == AggrMethod::DistinctCount {
                    row.try_get::<i64, _>(pg_col).map_or(Value::Null, |v| json!(v))
                } else {
                    row.try_get::<f64, _>(pg_col).map_or(Value::Null, |v| json!(v))
                };

                let bucket_start: Value = row
                    .try_get::<DateTime<Utc>, _>("bucket_start")
                    .map_or(Value::Null, |dt| json!(format_datetime(dt)));
                let bucket_end: Value = row
                    .try_get::<DateTime<Utc>, _>("bucket_end")
                    .map_or(Value::Null, |dt| json!(format_datetime(dt)));

                triples.push(json!([val, bucket_start, bucket_end]));
            }

            instance.insert(key.to_string(), Value::Array(triples));
        }

        attrs.entry(attr_name.clone()).or_default().push(Value::Object(instance));
    }

    let mut result: IndexMap<String, Value> = IndexMap::with_capacity(attrs.len());
    for (attr_name, instances) in attrs {
        if instances.len() == 1 {
            if let Some(single) = instances.into_iter().next() {
                result.insert(attr_name, single);
            }
        } else {
            result.insert(attr_name, Value::Array(instances));
        }
    }

    Ok(result)
}

fn parse_tea_entry(row: &PgRow) -> Result<TeaEntry, AndromedaError> {
    let attribute_name_raw: String = row.try_get("attribute_name")?;
    let attribute_name = IriBuf::new(attribute_name_raw.clone())
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal attribute IRI '{}': {}", attribute_name_raw, e)))?;

    let attribute_type_raw: String = row.try_get("attribute_type")?;
    let attribute_type = AttributeType::from_str(&attribute_type_raw)
        .map_err(|e| AndromedaError::internal(format!("Invalid temporal attribute type '{}': {}", attribute_type_raw, e)))?;

    let dataset_id = row
        .try_get::<Option<String>, _>("dataset_id")?
        .map(|raw| Uri::from_str(&raw).map_err(|_| AndromedaError::internal(format!("Invalid temporal datasetId URI '{}'", raw))))
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

fn history_row_to_expanded_instance(row: &PgRow, tea: &TeaEntry, sys_attrs: SysAttrsInclusion) -> Result<Value, AndromedaError> {
    let mut instance = serde_json::Map::new();

    instance.insert("@type".to_string(), json!([tea.attribute_type.iri().as_str()]));

    let value_key = tea.attribute_type.value_key().as_str();

    let is_tombstone = row.try_get::<Option<String>, _>("value_string").ok().flatten().as_deref() == Some(vocab::NULL_URI.as_str());

    if is_tombstone {
        if tea.attribute_name == vocab::SCOPE.as_str() {
            instance.insert(value_key.to_string(), json!([]));
        } else if tea.attribute_type == AttributeType::Relationship {
            instance.insert(value_key.to_string(), json!([{"@id": vocab::NULL_URI.as_str()}]));
        } else if tea.attribute_type == AttributeType::LanguageProperty {
            instance.insert(value_key.to_string(), json!([{"@value": vocab::NULL_URI.as_str(), "@language": "@none"}]));
        } else {
            instance.insert(value_key.to_string(), json!([{"@value": vocab::NULL_URI.as_str()}]));
        }
        if let Ok(time) = row.try_get::<DateTime<Utc>, _>("time") {
            instance.insert(
                vocab::DELETED_AT.as_str().to_string(),
                json!([{"@value": format_datetime(time), "@type": vocab::DATE_TIME.as_str()}]),
            );
        }
        if tea.attribute_name != vocab::SCOPE.as_str() {
            let instance_id: String = row.try_get("instance_id")?;
            instance.insert(vocab::INSTANCE_ID.as_str().to_string(), json!([{"@id": instance_id}]));
        }
        if let Some(ds_id) = &tea.dataset_id {
            instance.insert(vocab::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id}]));
        }
        return Ok(Value::Object(instance));
    }

    let effective_value_type = if tea.value_type == ValueType::Object {
        match row.try_get::<Option<Value>, _>("value_object") {
            Ok(Some(_)) => ValueType::Object,
            _ => {
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
            if let Ok(v) = row.try_get::<String, _>("value_geo_json") {
                let geo: Value =
                    serde_json::from_str(&v).map_err(|e| AndromedaError::internal("Failed to parse GeoJSON from temporal value").with_source(e))?;
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
                    json!([{"@value": format_datetime(v), "@type": vocab::DATE_TIME.as_str()}]),
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
            vocab::OBSERVED_AT.as_str().to_string(),
            json!([{"@value": format_datetime(observed_at), "@type": vocab::DATE_TIME.as_str()}]),
        );
    }

    if sys_attrs == SysAttrsInclusion::Include
        && let Ok(time) = row.try_get::<DateTime<Utc>, _>("time")
    {
        instance.insert(
            vocab::CREATED_AT.as_str().to_string(),
            json!([{"@value": format_datetime(time), "@type": vocab::DATE_TIME.as_str()}]),
        );
    }

    let instance_id: String = row.try_get("instance_id")?;
    instance.insert(vocab::INSTANCE_ID.as_str().to_string(), json!([{"@id": instance_id}]));

    if let Some(ds_id) = &tea.dataset_id {
        instance.insert(vocab::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id}]));
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

// ---------------------------------------------------------------------------
// GeoJSON <-> expanded GeoJSON-LD conversion
// ---------------------------------------------------------------------------

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
        _ => json!({"@value": value}),
    }
}

// ---------------------------------------------------------------------------
// Simplified temporal representation (temporalValues format)
// ---------------------------------------------------------------------------

/// Transform a compacted temporal entity into the `temporalValues` simplified
/// representation (NGSI-LD clause 4.5.5).
///
/// Each attribute is collapsed into `[value, observedAt]` pairs grouped by
/// dataset ID. Reserved entity-level keys (`id`, `type`, `@context`, timestamps)
/// pass through unchanged.
pub fn transform_to_temporal_values(compacted: Value) -> Value {
    let obj = match compacted {
        Value::Object(m) => m,
        other => return other,
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

    Value::Object(result)
}

/// Simplify a single temporal entity value to key-values representation.
///
/// Deserializes the JSON value into an [`Entity`], calls `simplify()` to
/// produce the simplified output, and returns the result. Used by temporal
/// handlers that need to apply key-values mode to temporal entity responses.
pub fn simplify_temporal_value(entity_value: Value) -> Result<Value, AndromedaError> {
    let entity: Entity = serde_json::from_value(entity_value)
        .map_err(|e| AndromedaError::bad_request_data(format!("Failed to parse temporal entity for simplification: {}", e)))?;
    Ok(entity.simplify())
}

/// Simplify a collection of temporal entity values to key-values representation.
///
/// Applies [`simplify_temporal_value`] to each entity in the collection.
/// Propagates the first deserialization error encountered.
pub fn simplify_temporal_values(entities: Vec<Value>) -> Result<Vec<Value>, AndromedaError> {
    entities.into_iter().map(simplify_temporal_value).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn simplify_temporal_value_with_property() {
        let entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {
                "type": "Property",
                "value": 100
            }
        });
        let result = simplify_temporal_value(entity).expect("simplification succeeds");
        assert_eq!(result["id"], "urn:ngsi-ld:Vehicle:1");
        assert_eq!(result["type"], "Vehicle");
        assert_eq!(result["speed"], 100);
    }

    #[test]
    fn simplify_temporal_value_with_relationship() {
        let entity = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "isParked": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Parking:1"
            }
        });
        let result = simplify_temporal_value(entity).expect("simplification succeeds");
        assert_eq!(result["isParked"], "urn:ngsi-ld:Parking:1");
    }

    #[test]
    fn simplify_temporal_value_invalid_json_returns_error() {
        let not_entity = json!({"not": "an entity"});
        assert!(simplify_temporal_value(not_entity).is_err());
    }

    #[test]
    fn simplify_temporal_values_empty_vec() {
        let result = simplify_temporal_values(Vec::new()).expect("empty vec succeeds");
        assert!(result.is_empty());
    }

    #[test]
    fn simplify_temporal_values_multiple_entities() {
        let entities = vec![
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": { "type": "Property", "value": 100 }
            }),
            json!({
                "id": "urn:ngsi-ld:Vehicle:2",
                "type": "Vehicle",
                "speed": { "type": "Property", "value": 200 }
            }),
        ];
        let result = simplify_temporal_values(entities).expect("simplification succeeds");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0]["speed"], 100);
        assert_eq!(result[1]["speed"], 200);
    }

    #[test]
    fn simplify_temporal_values_propagates_error() {
        let entities = vec![
            json!({
                "id": "urn:ngsi-ld:Vehicle:1",
                "type": "Vehicle",
                "speed": { "type": "Property", "value": 100 }
            }),
            json!({"not": "an entity"}),
        ];
        assert!(simplify_temporal_values(entities).is_err());
    }

    // --- ensure_temporal_attrs_are_arrays tests (NGSI-LD 6.3.10) ---

    #[test]
    fn ensure_temporal_attrs_wraps_objects() {
        let mut value = json!({"id": "urn:x", "type": "T", "speed": {"type": "Property", "value": 25}});
        ensure_temporal_attrs_are_arrays(&mut value);
        assert!(value["speed"].is_array());
        assert_eq!(value["speed"][0]["value"], 25);
    }

    #[test]
    fn ensure_temporal_attrs_leaves_arrays() {
        let mut value = json!({"id": "urn:x", "type": "T", "speed": [{"type": "Property", "value": 25}]});
        let original = value.clone();
        ensure_temporal_attrs_are_arrays(&mut value);
        assert_eq!(value, original);
    }

    #[test]
    fn ensure_temporal_attrs_skips_reserved() {
        let mut value = json!({"id": "urn:x", "type": "T", "createdAt": "2025-01-01T00:00:00Z"});
        ensure_temporal_attrs_are_arrays(&mut value);
        assert!(value["createdAt"].is_string());
    }

    // --- transform_to_temporal_values tests (NGSI-LD 4.5.5) ---

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
        let result = transform_to_temporal_values(input);
        assert_eq!(result["speed"]["type"], "Property");
        let values = result["speed"]["values"].as_array().expect("values array");
        assert_eq!(values.len(), 2);
        assert_eq!(values[0][0], 25);
        assert_eq!(values[1][0], 30);
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
        let result = transform_to_temporal_values(input);
        let arr = result["speed"].as_array().expect("multi-dataset array");
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn temporal_values_preserves_reserved() {
        let input = json!({
            "id": "urn:x",
            "type": "Vehicle",
            "@context": "https://example.org/ctx",
            "speed": [{"type": "Property", "value": 10, "observedAt": "t1"}]
        });
        let result = transform_to_temporal_values(input);
        assert_eq!(result["id"], "urn:x");
        assert_eq!(result["type"], "Vehicle");
        assert_eq!(result["@context"], "https://example.org/ctx");
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
        let result = transform_to_temporal_values(input);
        assert_eq!(result["isParked"]["type"], "Relationship");
        let objects = result["isParked"]["objects"].as_array().expect("objects array");
        assert_eq!(objects.len(), 2);
        assert_eq!(objects[0][0], "urn:p1");
    }
}
