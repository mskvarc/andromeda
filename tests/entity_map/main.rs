//! EntityMap integration tests for the NGSI-LD context broker.
//!
//! Standalone test suite using `tokio::test` and `reqwest` directly.
//! No libtest-mimic and no fixture loading helpers are used.
//!
//! The cases are grouped by what they exercise: the lifecycle of an EntityMap
//! resource, its creation from a query, the temporal variant, the entity query
//! that creates one along the way, frozen-map pagination, and content
//! negotiation. This module owns only what those groups share.
//!
//! Naming convention: `entity_map_<area>_<scenario>_<expected_outcome>`.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport errors.
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod content_negotiation;
mod creation;
mod entity_query;
mod lifecycle;
mod pagination;
mod temporal;

use andromeda_test_fixtures::{context::TEST_SUITE_CONTEXT, problem_detail as error};
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, serialized_environment::SerializedEnvironment};
use reqwest::{Client, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use urlencoding::encode;
use uuid::Uuid;

const BAD_REQUEST_DATA_TYPE: &iri_rs::Iri<&str> = error::BAD_REQUEST_DATA;
const INVALID_REQUEST_TYPE: &iri_rs::Iri<&str> = error::INVALID_REQUEST;
const RESOURCE_NOT_FOUND_TYPE: &iri_rs::Iri<&str> = error::RESOURCE_NOT_FOUND;

fn client() -> Client {
    Client::new()
}

fn unique_id(prefix: &str) -> String {
    format!("{prefix}:{}", Uuid::new_v4())
}

fn jsonld_link_value() -> String {
    format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// The instant every fixture attribute declares as its `observedAt`.
///
/// Clause 4.11 makes `observedAt` the time property a temporal query selects by
/// default, and clause 5.2.21 admits only the four semantic axes, so an
/// attribute instance recorded without `observedAt` matches no `observedAt`
/// window and the broker never substitutes its write clock for one. The
/// fixtures therefore declare an observation time, which is what makes them
/// visible to the temporal cases of this suite.
const FIXTURE_OBSERVED_AT: &str = "2021-01-01T00:00:00Z";

/// A `speed` Property carrying the fixture observation time.
fn observed_speed(speed: i64) -> Value {
    json!({
        "type": "Property",
        "value": speed,
        "observedAt": FIXTURE_OBSERVED_AT
    })
}

fn vehicle_entity(id: &str) -> Value {
    vehicle_entity_with_speed(id, 50)
}

fn vehicle_entity_with_speed(id: &str, speed: i64) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": observed_speed(speed),
        "@context": TEST_SUITE_CONTEXT
    })
}

/// A Vehicle whose `linkedTo` Relationship names `target`, so a clause 4.5.23
/// join reaches `target` from it.
fn vehicle_linking_to(id: &str, target: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": observed_speed(50),
        "linkedTo": {
            "type": "Relationship",
            "object": target,
            "observedAt": FIXTURE_OBSERVED_AT
        },
        "@context": TEST_SUITE_CONTEXT
    })
}

async fn clean_database() -> SerializedEnvironment {
    SerializedEnvironment::from_environment_reset()
        .await
        .expect("serialized test database environment")
}

async fn create_entity(client: &Client, entity: &Value) -> (StatusCode, Option<String>) {
    let resp = client
        .post(format!("{BASE_URL}/entities"))
        .header("Content-Type", "application/ld+json")
        .json(entity)
        .send()
        .await
        .expect("create_entity request failed");
    let status = resp.status();
    let location = resp.headers().get("location").map(|v| v.to_str().expect("non-ascii location").to_owned());
    (status, location)
}

async fn delete_entity(client: &Client, id: &str) {
    let _ = client.delete(format!("{BASE_URL}/entities/{}", encode(id))).send().await;
}

async fn create_entity_map_from_get(client: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entityMaps{sep}{query}"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("create_entity_map_from_get request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn create_entity_map_from_post(client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .post(format!("{BASE_URL}/entityMaps"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_value())
        .json(body)
        .send()
        .await
        .expect("create_entity_map_from_post request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn create_temporal_entity_map_from_get(client: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/temporal/entityMaps{sep}{query}"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("create_temporal_entity_map_from_get request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn create_temporal_entity_map_from_post(client: &Client, body: &Value) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .post(format!("{BASE_URL}/temporal/entityMaps"))
        .header("Content-Type", "application/json")
        .header("Link", jsonld_link_value())
        .json(body)
        .send()
        .await
        .expect("create_temporal_entity_map_from_post request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn get_entity_map(client: &Client, id: &str, accept: &str) -> (StatusCode, HeaderMap, Value) {
    let resp = client
        .get(format!("{BASE_URL}/entityMaps/{}", encode(id)))
        .header("Accept", accept)
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("get_entity_map request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn update_entity_map(client: &Client, id: &str, patch: &Value) -> (StatusCode, Value) {
    let resp = client
        .patch(format!("{BASE_URL}/entityMaps/{}", encode(id)))
        .header("Content-Type", "application/json")
        .json(patch)
        .send()
        .await
        .expect("update_entity_map request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, body)
}

async fn delete_entity_map(client: &Client, id: &str) -> StatusCode {
    client
        .delete(format!("{BASE_URL}/entityMaps/{}", encode(id)))
        .send()
        .await
        .expect("delete_entity_map request failed")
        .status()
}

async fn query_entities(client: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("query_entities request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn query_temporal_entities(client: &Client, query: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/temporal/entities{sep}{query}"))
        .header("Link", jsonld_link_value())
        .send()
        .await
        .expect("query_temporal_entities request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn query_entities_with_entity_map(client: &Client, query: &str, entity_map_id: &str) -> (StatusCode, HeaderMap, Value) {
    let sep = if query.is_empty() { "" } else { "?" };
    let resp = client
        .get(format!("{BASE_URL}/entities{sep}{query}"))
        .header("Link", jsonld_link_value())
        .header("NGSILD-EntityMap", entity_map_id)
        .send()
        .await
        .expect("query_entities_with_entity_map request failed");
    let status = resp.status();
    let headers = resp.headers().clone();
    let body: Value = resp.json().await.unwrap_or_default();
    (status, headers, body)
}

async fn cleanup_entity_map(client: &Client, id: &str) {
    let _ = delete_entity_map(client, id).await;
}

async fn setup_single_vehicle(client: &Client, id: &str) {
    let (status, _) = create_entity(client, &vehicle_entity(id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
}

/// The identifier the `NGSILD-EntityMap` response header names.
fn entity_map_header(headers: &HeaderMap) -> String {
    headers
        .get("NGSILD-EntityMap")
        .expect("NGSILD-EntityMap header should be present")
        .to_str()
        .expect("header should be valid ASCII")
        .to_owned()
}

/// The identifiers one EntityMap resource records.
fn mapped_ids(map: &Value) -> Vec<String> {
    map["entityMap"].as_object().expect("entityMap should be object").keys().cloned().collect()
}
