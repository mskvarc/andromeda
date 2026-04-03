//! Temporal response building: `build_entity_temporal`, `ensure_temporal_attrs_are_arrays`,
//! `transform_to_temporal_values`, `build_aggregated_data`, `expand_geojson`, `expand_coordinates`.

use crate::{
    domain::{
        attribute_type::AttributeType,
        iri,
        temporal::{AggrMethod, TeaEntry, TimeRel},
    },
    error::AndromedaError,
    query::temporal::CompiledTemporalQuery,
    service::temporal::format_datetime,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde_json::{Value, json};
use sqlx::Row;

type TimeRange = (DateTime<Utc>, DateTime<Utc>);
type TemporalAttributeInstances = Vec<(Option<DateTime<Utc>>, Value)>;
type TemporalAttrMap = IndexMap<String, TemporalAttributeInstances>;
type BuildEntityTemporalResult = (Value, bool, Option<TimeRange>, Option<i64>);

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

pub fn build_content_range(
    compiled: &CompiledTemporalQuery,
    time_range: Option<TimeRange>,
    partition_total: Option<i64>,
    user_specified_last_n: bool,
) -> Option<String> {
    let (min_time, max_time) = time_range?;
    let lrt = format_datetime(min_time);
    let mrt = format_datetime(max_time);

    let (start, end, size) = if user_specified_last_n {
        let upper = match compiled.timerel {
            Some(TimeRel::Before) => compiled.time_at.map(format_datetime).unwrap_or_else(|| mrt.clone()),
            Some(TimeRel::Between) => compiled.end_time_at.map(format_datetime).unwrap_or_else(|| mrt.clone()),
            Some(TimeRel::After) => mrt.clone(),
            None => mrt.clone(),
        };
        let size = partition_total.map(|n| n.to_string()).unwrap_or_else(|| "*".to_string());
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

/// Returns (entity_json, truncated, time_range, max_partition_total).
pub fn build_entity_temporal(
    entity_id: &str,
    entity_types: &[String],
    rows: &[sqlx::postgres::PgRow],
    compiled: &CompiledTemporalQuery,
) -> Result<BuildEntityTemporalResult, AndromedaError> {
    let mut attrs: TemporalAttrMap = IndexMap::with_capacity(rows.len().min(64));
    let mut truncated = false;
    let mut min_time: Option<DateTime<Utc>> = None;
    let mut max_time: Option<DateTime<Utc>> = None;
    let mut max_partition_total: Option<i64> = None;

    for row in rows {
        let attr_name: String = row.try_get("attribute_name")?;
        let attr_type: String = row.try_get("attribute_type")?;
        let dataset_id: Option<String> = row.try_get("dataset_id")?;
        let value_type: String = row.try_get("value_type")?;

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

        let tea = TeaEntry {
            id: row.try_get("tea_id")?,
            entity_id: entity_id.to_string(),
            attribute_name: attr_name.clone(),
            attribute_type: attr_type,
            dataset_id,
            value_type,
        };

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
    Ok((Value::Object(entity), truncated, time_range, max_partition_total))
}

pub fn build_entity_temporal_aggregated_data(
    rows: &[sqlx::postgres::PgRow],
    compiled: &CompiledTemporalQuery,
) -> Result<IndexMap<String, Value>, AndromedaError> {
    type GroupKey = (String, Option<String>);
    let mut groups: IndexMap<GroupKey, Vec<&sqlx::postgres::PgRow>> = IndexMap::with_capacity(rows.len().min(64));

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
                    row.try_get::<i64, _>(pg_col).map(|v| json!(v)).unwrap_or(Value::Null)
                } else {
                    row.try_get::<f64, _>(pg_col).map(|v| json!(v)).unwrap_or(Value::Null)
                };

                let bucket_start: Value = row
                    .try_get::<DateTime<Utc>, _>("bucket_start")
                    .map(|dt| json!(format_datetime(dt)))
                    .unwrap_or(Value::Null);
                let bucket_end: Value = row
                    .try_get::<DateTime<Utc>, _>("bucket_end")
                    .map(|dt| json!(format_datetime(dt)))
                    .unwrap_or(Value::Null);

                triples.push(json!([val, bucket_start, bucket_end]));
            }

            instance.insert(key.to_string(), Value::Array(triples));
        }

        attrs.entry(attr_name.clone()).or_default().push(Value::Object(instance));
    }

    let mut result: IndexMap<String, Value> = IndexMap::with_capacity(attrs.len());
    for (attr_name, instances) in attrs {
        if instances.len() == 1 {
            result.insert(attr_name, instances.into_iter().next().unwrap());
        } else {
            result.insert(attr_name, Value::Array(instances));
        }
    }

    Ok(result)
}

fn history_row_to_expanded_instance(row: &sqlx::postgres::PgRow, tea: &TeaEntry, sys_attrs: bool) -> Result<Value, AndromedaError> {
    let mut instance = serde_json::Map::new();

    let attr_type: Option<AttributeType> = tea.attribute_type.parse().ok();
    let type_iri = attr_type.map(|t| t.iri().as_str()).unwrap_or(tea.attribute_type.as_str());
    instance.insert("@type".to_string(), json!([type_iri]));

    let value_key = attr_type.map(|t| t.value_key().as_str()).unwrap_or(iri::HAS_VALUE.as_str());

    let is_tombstone = row.try_get::<Option<String>, _>("value_string").ok().flatten().as_deref() == Some(iri::NULL_URI.as_str());

    if is_tombstone {
        if tea.attribute_name == iri::SCOPE.as_str() {
            instance.insert(value_key.to_string(), json!([]));
        } else if attr_type == Some(AttributeType::Relationship) {
            instance.insert(value_key.to_string(), json!([{"@id": iri::NULL_URI.as_str()}]));
        } else if attr_type == Some(AttributeType::LanguageProperty) {
            instance.insert(value_key.to_string(), json!([{"@value": iri::NULL_URI.as_str(), "@language": "@none"}]));
        } else {
            instance.insert(value_key.to_string(), json!([{"@value": iri::NULL_URI.as_str()}]));
        }
        if let Ok(time) = row.try_get::<DateTime<Utc>, _>("time") {
            instance.insert(
                iri::DELETED_AT.as_str().to_string(),
                json!([{"@value": format_datetime(time), "@type": iri::DATETIME_TYPE.as_str()}]),
            );
        }
        if tea.attribute_name != iri::SCOPE.as_str() {
            let instance_id: String = row.try_get("instance_id")?;
            instance.insert(iri::INSTANCE_ID.as_str().to_string(), json!([{"@id": instance_id}]));
        }
        if let Some(ds_id) = &tea.dataset_id {
            instance.insert(iri::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id}]));
        }
        return Ok(Value::Object(instance));
    }

    let effective_value_type = if tea.value_type == "Object" {
        match row.try_get::<Option<Value>, _>("value_object") {
            Ok(Some(_)) => "Object",
            _ => {
                if row.try_get::<Option<String>, _>("value_string").ok().flatten().is_some() {
                    "String"
                } else if row.try_get::<Option<f64>, _>("value_number").ok().flatten().is_some() {
                    "Number"
                } else {
                    "Object"
                }
            }
        }
    } else {
        tea.value_type.as_str()
    };

    match effective_value_type {
        "Number" => {
            if let Ok(v) = row.try_get::<f64, _>("value_number") {
                let json_val = if v.fract() == 0.0 && v >= i64::MIN as f64 && v <= i64::MAX as f64 {
                    json!(v as i64)
                } else {
                    json!(v)
                };
                instance.insert(value_key.to_string(), json!([{"@value": json_val}]));
            }
        }
        "String" => {
            if let Ok(v) = row.try_get::<String, _>("value_string") {
                if attr_type == Some(AttributeType::VocabProperty) {
                    instance.insert(value_key.to_string(), json!([{"@id": v}]));
                } else {
                    instance.insert(value_key.to_string(), json!([{"@value": v}]));
                }
            }
        }
        "Boolean" => {
            if let Ok(v) = row.try_get::<bool, _>("value_bool") {
                instance.insert(value_key.to_string(), json!([{"@value": v}]));
            }
        }
        "Geometry" => {
            if let Ok(v) = row.try_get::<String, _>("value_geo_json") {
                let geo: Value = serde_json::from_str(&v).unwrap_or(Value::Null);
                let expanded = expand_geojson(&geo);
                instance.insert(value_key.to_string(), json!([expanded]));
            }
        }
        "Object" => {
            if let Ok(v) = row.try_get::<Value, _>("value_object") {
                if attr_type == Some(AttributeType::JsonProperty) {
                    instance.insert(value_key.to_string(), json!([{"@value": v, "@type": "@json"}]));
                } else if attr_type == Some(AttributeType::LanguageProperty) {
                    instance.insert(value_key.to_string(), v);
                } else if attr_type == Some(AttributeType::VocabProperty) {
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
        "DateTime" => {
            if let Ok(v) = row.try_get::<DateTime<Utc>, _>("value_datetime") {
                instance.insert(
                    value_key.to_string(),
                    json!([{"@value": format_datetime(v), "@type": iri::DATETIME_TYPE.as_str()}]),
                );
            }
        }
        "Uri" => {
            if let Ok(v) = row.try_get::<String, _>("value_uri") {
                instance.insert(value_key.to_string(), json!([{"@id": v}]));
            }
        }
        _ => {}
    }

    if let Ok(Some(observed_at)) = row.try_get::<Option<DateTime<Utc>>, _>("observed_at") {
        instance.insert(
            iri::OBSERVED_AT.as_str().to_string(),
            json!([{"@value": format_datetime(observed_at), "@type": iri::DATETIME_TYPE.as_str()}]),
        );
    }

    if sys_attrs && let Ok(time) = row.try_get::<DateTime<Utc>, _>("time") {
        instance.insert(
            iri::CREATED_AT.as_str().to_string(),
            json!([{"@value": format_datetime(time), "@type": iri::DATETIME_TYPE.as_str()}]),
        );
    }

    let instance_id: String = row.try_get("instance_id")?;
    instance.insert(iri::INSTANCE_ID.as_str().to_string(), json!([{"@id": instance_id}]));

    if let Some(ds_id) = &tea.dataset_id {
        instance.insert(iri::DATASET_ID.as_str().to_string(), json!([{"@id": ds_id}]));
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

const GEOJSON_VOCAB: &str = "https://purl.org/geojson/vocab#";

fn expand_geojson(geojson: &Value) -> Value {
    let Some(obj) = geojson.as_object() else {
        return geojson.clone();
    };

    let geo_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("Point");
    let coordinates = obj.get("coordinates").cloned().unwrap_or(json!([]));
    let expanded_coords = expand_coordinates(&coordinates);

    json!({
        "@type": [format!("{}{}", GEOJSON_VOCAB, geo_type)],
        format!("{}coordinates", GEOJSON_VOCAB): [{"@list": expanded_coords}]
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
            let (value_key, extract_key) = parsed_attr_type.map(|t| t.temporal_values_keys()).unwrap_or(("values", "value"));

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
            result.insert(key.clone(), tv_objects.into_iter().next().unwrap());
        } else {
            result.insert(key.clone(), Value::Array(tv_objects));
        }
    }

    Value::Object(result)
}
