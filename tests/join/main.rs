//! Join traversal, `containedBy` and projection integration tests
//! (NGSI-LD 4.5.23, 4.21, 6.3.15, 6.4.3.2).
//!
//! The cases are grouped by the shape of the response they assert on: the
//! flat collection, the inline nesting, the `pick`/`omit` narrowing, and the
//! GeoJSON rendering. A fifth group covers the clause 4.9 `q` traversal axis,
//! which reaches the same linked entities to *select* roots rather than to
//! render them. This module owns only the helpers those groups share.
//!
//! Naming convention: `join_<scenario>_<expected_outcome>`, and
//! `q_linked_<scenario>` for the traversal-axis cases.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod flat;
mod geojson;
mod inline;
mod projection;
mod q_traversal;

use andromeda_test_client::{header::jsonld_link_header, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// Create an entity, asserting the broker accepted it.
async fn create(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

/// `GET /entities` with the suite context, asserting a 200.
async fn query(client: &TestClient, query_str: &str) -> Value {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("query_entities failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

/// `GET /entities/{id}` with the suite context, asserting a 200.
async fn get_one(client: &TestClient, id: &str, query_str: &str) -> Value {
    let response = client
        .get_entity(id, query_str, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("get_entity failed");
    assert_eq!(response.status, StatusCode::OK);
    response.json_value().expect("response body must be valid JSON")
}

/// `POST /entityOperations/query` with the suite context, asserting a 200.
async fn post_query(client: &TestClient, body: &Value) -> Value {
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/query")
        .header("Link", jsonld_link_header(TEST_SUITE_CONTEXT).expect("valid JSON-LD Link header"))
        .json(body)
        .send()
        .await
        .expect("post query failed");
    assert_eq!(response.status, StatusCode::OK, "post query: {:?}", response.body);
    response.json_value().expect("response body must be valid JSON")
}

/// The `id` member of every entity in an array body, in response order.
fn ids_from_array(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("response must be array")
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect()
}

/// The one array member carrying `id`.
fn member_with_id<'a>(body: &'a Value, id: &str) -> &'a Value {
    let member = body
        .as_array()
        .expect("response must be array")
        .iter()
        .find(|entity| entity["id"].as_str() == Some(id));
    assert!(member.is_some(), "no array member with id {id}: {body}");
    member.expect("the array member was just asserted to be present")
}

fn minimal_building(id: &str) -> Value {
    json!({"id": id, "type": "Building", "@context": [TEST_SUITE_CONTEXT]})
}

fn linked_building(id: &str, target: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "linkedTo": {"type": "Relationship", "object": target},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A Building carrying the two Properties the nested-projection cases narrow.
fn described_building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "category": {"type": "Property", "value": "commercial"},
        "name": {"type": "Property", "value": "Depot"},
        "@context": [TEST_SUITE_CONTEXT]
    })
}
