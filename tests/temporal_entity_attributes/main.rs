//! Temporal entity attributes integration tests for the NGSI-LD context broker.
//!
//! Tests temporal entity attribute provision operations:
//! - Add attributes: POST /temporal/entities/{entityId}/attrs
//! - Delete attributes: DELETE /temporal/entities/{entityId}/attrs/{attrId}
//! - Delete attribute instance: DELETE /temporal/entities/{entityId}/attrs/{attrId}/{instanceId}
//! - Partial update (modify instance): PATCH /temporal/entities/{entityId}/attrs/{attrId}/{instanceId}
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//!
//! Naming convention: `temporal_attrs_<operation>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod add;
mod delete;
mod delete_instance;
mod partial_update;

use andromeda_test_client::header::jsonld_link_header as try_jsonld_link_header;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, StatusCode, header::HeaderValue};
use serde_json::{Value, json};
use uuid::Uuid;

fn jsonld_link_header(context_url: &str) -> HeaderValue {
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

/// Extract instanceId from first instance of an attribute.
fn extract_instance_id(entity: &Value, attr: &str) -> String {
    entity[attr][0]["instanceId"].as_str().expect("instanceId not found").to_string()
}

// ---------------------------------------------------------------------------
// Temporal entity factories
// ---------------------------------------------------------------------------

/// Vehicle with temporal speed and fuelLevel attributes (standard test entity).
fn vehicle_temporal_representation(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z" },
            { "type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 67, "observedAt": "2020-09-01T12:03:00Z" },
            { "type": "Property", "value": 53, "observedAt": "2020-09-01T13:05:00Z" },
            { "type": "Property", "value": 40, "observedAt": "2020-09-01T14:07:00Z", "datasetId": "urn:ngsi-ld:Vehicle:12345-fuel" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Fragment to add a new attribute (added_attribute) with temporal values.
fn add_attribute_fragment() -> Value {
    json!({
        "added_attribute": [
            { "type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z" },
            { "type": "Property", "value": 80, "observedAt": "2020-09-01T12:05:00Z" },
            { "type": "Property", "value": 100, "observedAt": "2020-09-01T12:07:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    })
}

/// Fragment to modify an attribute instance (changes value to 129).
fn modify_instance_fragment() -> Value {
    json!({
        "type": "Property",
        "value": 129,
        "observedAt": "2020-09-01T12:03:00Z",
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

async fn get_temporal_entity(c: &Client, id: &str, context: Option<&str>) -> (StatusCode, Value) {
    get_temporal_entity_with_query(c, id, "", context).await
}

async fn get_temporal_entity_with_query(c: &Client, id: &str, query: &str, context: Option<&str>) -> (StatusCode, Value) {
    let suffix = if query.is_empty() { String::new() } else { format!("?{query}") };
    let mut req = c
        .get(format!("{BASE_URL}/temporal/entities/{id}{suffix}"))
        .header("Accept", "application/ld+json");
    if let Some(ctx) = context {
        req = req.header("Link", jsonld_link_header(ctx));
    }
    let resp = req.send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn add_temporal_attrs(c: &Client, id: &str, fragment: &Value) -> (StatusCode, Vec<u8>) {
    let resp = c
        .post(format!("{BASE_URL}/temporal/entities/{id}/attrs"))
        .header("Content-Type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
    (status, body)
}

async fn add_temporal_attrs_raw(c: &Client, id: &str, bytes: &[u8], content_type: &str) -> (StatusCode, Value) {
    let resp = c
        .post(format!("{BASE_URL}/temporal/entities/{id}/attrs"))
        .header("Content-Type", content_type)
        .body(bytes.to_vec())
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn request_temporal_attrs(c: &Client, method: reqwest::Method, path: &str, body: Option<&Value>) -> (StatusCode, Value) {
    let mut req = c.request(method, format!("{BASE_URL}{path}")).header("Content-Type", "application/ld+json");
    if let Some(b) = body {
        req = req.json(b);
    }
    let resp = req.send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    (status, body)
}

async fn delete_temporal_attr(c: &Client, id: &str, attr: &str) -> (StatusCode, Vec<u8>, Value) {
    let resp = c
        .delete(format!("{BASE_URL}/temporal/entities/{id}/attrs/{attr}"))
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let bytes = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, bytes, body)
}

async fn delete_temporal_attr_with_params(c: &Client, id: &str, attr: &str, params: &str) -> (StatusCode, Vec<u8>, Value) {
    let url = if params.is_empty() {
        format!("{BASE_URL}/temporal/entities/{id}/attrs/{attr}")
    } else {
        format!("{BASE_URL}/temporal/entities/{id}/attrs/{attr}?{params}")
    };
    let resp = c.delete(&url).send().await.expect("request failed");
    let status = resp.status();
    let bytes = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, bytes, body)
}

async fn delete_temporal_attr_instance(c: &Client, id: &str, attr: &str, instance_id: &str) -> (StatusCode, Vec<u8>, Value) {
    let resp = c
        .delete(format!("{BASE_URL}/temporal/entities/{id}/attrs/{attr}/{instance_id}"))
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let bytes = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, bytes, body)
}

async fn modify_temporal_attr_instance(c: &Client, id: &str, attr: &str, instance_id: &str, fragment: &Value) -> (StatusCode, Vec<u8>, Value) {
    let resp = c
        .patch(format!("{BASE_URL}/temporal/entities/{id}/attrs/{attr}/{instance_id}"))
        .header("Content-Type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let bytes = resp.bytes().await.map(|b| b.to_vec()).unwrap_or_default();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, bytes, body)
}
