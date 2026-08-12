//! Temporal entity integration tests for the NGSI-LD context broker.
//!
//! Tests temporal entity retrieve and query operations:
//! - Retrieve: basic, with context, specific attributes, temporal queries, lastN, pagination
//! - Query: by type, id, idPattern, attrs, geoquery, q filter, lastN, limit, pagination
//! - Representations: normalized, temporalValues, aggregatedValues
//! - Property types: Property, LanguageProperty, JsonProperty, VocabProperty
//! - DeletedAt handling
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `temporal_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used)]

mod create;
mod delete;
mod join;
mod projection;
mod query;
mod retrieve;
mod update;

use andromeda_test_client::header::jsonld_link_header as try_jsonld_link_header;
use andromeda_test_fixtures::context::{COMPOUND_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use uuid::Uuid;

fn jsonld_link_header(context_url: &str) -> reqwest::header::HeaderValue {
    try_jsonld_link_header(context_url).expect("valid JSON-LD Link header")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

/// Parse Content-Range header: `date-time <start>-<end>/<size>`
fn parse_content_range(headers: &HeaderMap) -> Option<(String, String, String, String)> {
    let val = headers.get("content-range")?.to_str().ok()?;
    let re = regex::Regex::new(r"([a-zA-Z\-]+) (.*-.*-.*)-(.*-.*-.*)/(.*)").ok()?;
    let caps = re.captures(val)?;
    Some((caps[1].to_string(), caps[2].to_string(), caps[3].to_string(), caps[4].to_string()))
}

/// Collect timestamps from normalized temporal entity body.
fn collect_timestamps_normalized(body: &Value) -> Vec<String> {
    let mut timestamps = Vec::new();
    for attr in ["speed", "fuelLevel"] {
        if let Some(arr) = body.get(attr).and_then(|v| v.as_array()) {
            for instance in arr {
                if let Some(ts) = instance.get("observedAt").and_then(|v| v.as_str()) {
                    timestamps.push(ts.to_string());
                }
            }
        }
    }
    timestamps
}

/// Collect timestamps from simplified temporal entity body (temporalValues).
fn collect_timestamps_temporal_values(body: &Value) -> Vec<String> {
    let mut timestamps = Vec::new();
    for attr in ["speed", "fuelLevel"] {
        if let Some(values) = body.get(attr).and_then(|v| v.get("values")).and_then(|v| v.as_array()) {
            for entry in values {
                if let Some(ts) = entry.get(1).and_then(|v| v.as_str()) {
                    timestamps.push(ts.to_string());
                }
            }
        }
    }
    timestamps
}

/// Get most recent timestamp from single entity body.
fn get_most_recent_timestamp(body: &Value, representation: &str) -> Option<String> {
    let timestamps = if representation == "temporalValues" {
        collect_timestamps_temporal_values(body)
    } else {
        collect_timestamps_normalized(body)
    };
    timestamps.into_iter().max()
}

/// Collect timestamps from entity list (for query responses).
fn collect_timestamps_vehicles(body: &[Value], representation: &str) -> Vec<String> {
    let mut timestamps = Vec::new();
    for entity in body {
        for attr in ["speed", "fuelLevel"] {
            if representation == "temporalValues" {
                if let Some(values) = entity.get(attr).and_then(|v| v.get("values")).and_then(|v| v.as_array()) {
                    for entry in values {
                        if let Some(ts) = entry.get(1).and_then(|v| v.as_str()) {
                            timestamps.push(ts.to_string());
                        }
                    }
                }
            } else if let Some(arr) = entity.get(attr).and_then(|v| v.as_array()) {
                for instance in arr {
                    if let Some(ts) = instance.get("observedAt").and_then(|v| v.as_str()) {
                        timestamps.push(ts.to_string());
                    }
                }
            }
        }
    }
    timestamps
}

fn get_most_recent_timestamp_vehicles(body: &[Value], representation: &str) -> Option<String> {
    collect_timestamps_vehicles(body, representation).into_iter().max()
}

// ---------------------------------------------------------------------------
// Temporal entity factories
// ---------------------------------------------------------------------------

/// Vehicle with temporal speed and fuelLevel attributes (August 2020 data).
fn vehicle_temporal_august(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z" },
            { "type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z" },
            { "type": "Property", "value": 40, "observedAt": "2020-08-01T14:07:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with temporal speed and fuelLevel attributes (September 2020 data).
fn vehicle_temporal_september(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 91, "observedAt": "2020-09-01T13:03:00Z" },
            { "type": "Property", "value": 92, "observedAt": "2020-09-01T13:05:00Z" },
            { "type": "Property", "value": 93, "observedAt": "2020-09-01T13:07:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 45, "observedAt": "2020-09-01T13:03:00Z" },
            { "type": "Property", "value": 35, "observedAt": "2020-09-01T14:05:00Z" },
            { "type": "Property", "value": 30, "observedAt": "2020-09-01T15:07:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with temporal attributes and location GeoProperty.
fn vehicle_temporal_with_location(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 50, "observedAt": "2020-10-01T10:00:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 80, "observedAt": "2020-10-01T10:00:00Z" }
        ],
        // The `observedAt` is what puts the GeoProperty on the observed-time
        // axis a temporal query selects along (clause 5.7.3 defaults
        // `timeproperty` to `observedAt`). Without it the instance declares no
        // observation instant, so no `timerel` window can name it and the
        // attribute would be absent from every temporal representation.
        "location": {
            "type": "GeoProperty",
            "value": {
                "type": "Point",
                "coordinates": [-8.503, 41.202]
            },
            "observedAt": "2020-10-01T10:00:00Z"
        },
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with multiple instances of speed and fuelLevel for lastN testing.
fn vehicle_temporal_multiple_instances(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 100, "observedAt": "2020-08-01T12:01:00Z" },
            { "type": "Property", "value": 101, "observedAt": "2020-08-01T12:02:00Z" },
            { "type": "Property", "value": 102, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 103, "observedAt": "2020-08-01T12:04:00Z" },
            { "type": "Property", "value": 104, "observedAt": "2020-08-01T12:05:00Z" },
            { "type": "Property", "value": 105, "observedAt": "2020-08-01T12:06:00Z" },
            { "type": "Property", "value": 106, "observedAt": "2020-08-01T12:07:00Z" },
            { "type": "Property", "value": 107, "observedAt": "2020-08-01T12:08:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 70, "observedAt": "2020-08-01T12:01:00Z" },
            { "type": "Property", "value": 68, "observedAt": "2020-08-01T12:02:00Z" },
            { "type": "Property", "value": 66, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 64, "observedAt": "2020-08-01T12:04:00Z" },
            { "type": "Property", "value": 62, "observedAt": "2020-08-01T12:05:00Z" },
            { "type": "Property", "value": 60, "observedAt": "2020-08-01T12:06:00Z" },
            { "type": "Property", "value": 58, "observedAt": "2020-08-01T12:07:00Z" },
            { "type": "Property", "value": 56, "observedAt": "2020-08-01T12:08:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with 20 speed instances for pagination testing.
fn vehicle_temporal_twenty_instances(id: &str) -> Value {
    let speed_instances: Vec<Value> = (1..=20)
        .map(|i| {
            json!({
                "type": "Property",
                "value": 100 + i,
                "observedAt": format!("2020-01-01T{:02}:00:00Z", i)
            })
        })
        .collect();
    let fuel_instances: Vec<Value> = (1..=20)
        .map(|i| {
            json!({
                "type": "Property",
                "value": 80 - i,
                "observedAt": format!("2020-01-01T{:02}:00:00Z", i)
            })
        })
        .collect();

    json!({
        "id": id,
        "type": "Vehicle",
        "speed": speed_instances,
        "fuelLevel": fuel_instances,
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Bus with temporal representation.
fn bus_temporal_august(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Bus",
        "speed": [
            { "type": "Property", "value": 55, "observedAt": "2020-08-01T08:00:00Z" },
            { "type": "Property", "value": 60, "observedAt": "2020-08-01T09:00:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 90, "observedAt": "2020-08-01T08:00:00Z" },
            { "type": "Property", "value": 85, "observedAt": "2020-08-01T09:00:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with LanguageProperty for name.
fn vehicle_temporal_language_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "name": [
            {
                "type": "LanguageProperty",
                "languageMap": { "en": "Fast Car", "de": "Schnelles Auto" },
                "observedAt": "2020-08-01T12:00:00Z"
            },
            {
                "type": "LanguageProperty",
                "languageMap": { "en": "Super Fast Car", "de": "Super Schnelles Auto" },
                "observedAt": "2020-08-01T13:00:00Z"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with JsonProperty.
fn vehicle_temporal_json_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "config": [
            {
                "type": "JsonProperty",
                "json": { "maxSpeed": 200, "eco": true },
                "observedAt": "2020-08-01T12:00:00Z"
            },
            {
                "type": "JsonProperty",
                "json": { "maxSpeed": 180, "eco": false },
                "observedAt": "2020-08-01T13:00:00Z"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with VocabProperty.
fn vehicle_temporal_vocab_property(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "category": [
            {
                "type": "VocabProperty",
                "vocab": "sedan",
                "observedAt": "2020-08-01T12:00:00Z"
            },
            {
                "type": "VocabProperty",
                "vocab": "sports",
                "observedAt": "2020-08-01T13:00:00Z"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with different attribute types for deletedAt testing.
fn vehicle_temporal_different_types(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "fuelLevel": [
            { "type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "isParkedIn": [
            {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Parking:P1",
                "observedAt": "2020-08-01T12:03:00Z"
            }
        ],
        "name": [
            {
                "type": "LanguageProperty",
                "languageMap": { "en": "My Vehicle" },
                "observedAt": "2020-08-01T12:03:00Z"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with scope.
fn vehicle_temporal_with_scope(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "scope": [
            "/Madrid/Center"
        ],
        "speed": [
            { "type": "Property", "value": 80, "observedAt": "2020-08-01T12:00:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Vehicle with multi-attribute instances including datasetId.
fn vehicle_temporal_multi_attributes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 100, "observedAt": "2020-08-01T12:05:00Z" },
            { "type": "Property", "value": 110, "observedAt": "2020-08-01T12:06:00Z", "datasetId": "urn:ngsi-ld:Dataset:Speed" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 50, "observedAt": "2020-08-01T12:05:00Z", "datasetId": "urn:ngsi-ld:Dataset:Common" },
            { "type": "Property", "value": 45, "observedAt": "2020-08-01T12:06:00Z", "datasetId": "urn:ngsi-ld:Dataset:fuel" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

async fn create_temporal_entity(c: &Client, entity: &Value) -> StatusCode {
    let resp = c
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("request failed");
    resp.status()
}

async fn get_temporal_entity(c: &Client, id: &str, query: &str, accept: &str, context: Option<&str>) -> (StatusCode, Value, HeaderMap) {
    let url = if query.is_empty() {
        format!("{BASE_URL}/temporal/entities/{id}")
    } else {
        format!("{BASE_URL}/temporal/entities/{id}?{query}")
    };

    let mut req = c.get(&url).header("Accept", accept);
    if let Some(ctx) = context {
        req = req.header("Link", jsonld_link_header(ctx));
    }

    let resp = req.send().await.expect("request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body, headers)
}

async fn query_temporal_entities(c: &Client, query: &str, context: Option<&str>) -> (StatusCode, Value, HeaderMap) {
    let url = format!("{BASE_URL}/temporal/entities?{query}");

    let mut req = c.get(&url).header("Accept", "application/ld+json");
    if let Some(ctx) = context {
        req = req.header("Link", jsonld_link_header(ctx));
    }

    let resp = req.send().await.expect("request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body, headers)
}

async fn post_temporal_query(c: &Client, query: &Value, context: Option<&str>) -> (StatusCode, Value, HeaderMap) {
    let url = format!("{BASE_URL}/temporal/entityOperations/query");

    let mut req = c.post(&url).header("Content-Type", "application/json").json(query);
    if let Some(ctx) = context {
        req = req.header("Link", jsonld_link_header(ctx));
    }

    let resp = req.send().await.expect("request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body, headers)
}

async fn delete_entity_attr(c: &Client, id: &str, attr: &str, context: Option<&str>) -> StatusCode {
    let mut req = c.delete(format!("{BASE_URL}/entities/{id}/attrs/{attr}"));
    if let Some(ctx) = context {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("request failed");
    resp.status()
}

async fn create_temporal_entity_raw(c: &Client, bytes: &[u8], content_type: &str) -> (StatusCode, Value) {
    let resp = c
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", content_type)
        .body(bytes.to_vec())
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn create_temporal_entity_json(c: &Client, entity: &Value, content_type: &str, context: Option<&str>) -> StatusCode {
    let mut req = c
        .post(format!("{BASE_URL}/temporal/entities"))
        .header("Content-Type", content_type)
        .json(entity);
    if let Some(ctx) = context {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("request failed");
    resp.status()
}

async fn delete_temporal_entity(c: &Client, id: &str) -> (StatusCode, Value) {
    let resp = c.delete(format!("{BASE_URL}/temporal/entities/{id}")).send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

/// Recursively strip `instanceId` and `@context` from a JSON value for comparison.
fn strip_instance_ids(val: &mut Value) {
    match val {
        Value::Object(map) => {
            map.remove("instanceId");
            map.remove("@context");
            for v in map.values_mut() {
                strip_instance_ids(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                strip_instance_ids(v);
            }
        }
        _ => {}
    }
}
