//! Expanding a GeoJSON geometry into its JSON-LD form.
//!
//! A geometry read back out of the store is plain GeoJSON, and the temporal
//! API answers in expanded JSON-LD, where a coordinate array is an `@list` of
//! `@value` nodes and the geometry type is a `@type` under the GeoJSON
//! vocabulary. This is that expansion, and it knows nothing about rows: it is
//! separated from the row decoding because the two change for different
//! reasons -- one when RFC 7946 or the GeoJSON context changes, the other when
//! the history table does.

use andromeda_ngsi_ld_vocabulary::prefix;
use serde_json::{Value, json};

/// Expand one canonical GeoJSON geometry into its JSON-LD vocabulary form.
pub fn expand_geojson(geojson: &Value) -> Value {
    let Some(obj) = geojson.as_object() else {
        return geojson.clone();
    };

    let geo_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("Point");
    let coordinates = obj.get("coordinates").cloned().unwrap_or(json!([]));
    let expanded_coords = expand_coordinates(&coordinates);

    json!({
        "@type": [format!("{}{}", prefix::GEOJSON.as_str(), geo_type)],
        format!("{}coordinates", prefix::GEOJSON.as_str()): [{"@list": expanded_coords}]
    })
}

/// Recursively convert coordinate arrays into JSON-LD list/value nodes.
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
