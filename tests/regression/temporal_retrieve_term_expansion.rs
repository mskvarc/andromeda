//! Whether a temporal retrieval compacts its terms against the request
//! `@context` alone, or truncates undeclared terms to bare fragments.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.7.3 retrieves the Temporal Evolution of an
//! Entity and, like every read, renders it against the `@context` the request
//! carries -- the core context alone when the client supplies none (clause
//! 5.5.7). Compaction turns `@id`/`@type` into `id`/`type` because the core
//! context declares those aliases, but a domain term the resolved context does
//! not declare has no short form and stays a full IRI. Clause 5.7.6 renders
//! type and attribute discovery by the same rule, so the two agree: no context
//! means full IRIs, a context that declares the term means the short term.
//!
//! The temporal read path runs no second pass over the compacted document that
//! collapses leftover full IRIs to their `#fragment` or last path segment. That
//! would produce `speed` and `Vehicle` for an entity created under the
//! test-suite context but retrieved without one, where clause 5.7.3 requires
//! the full IRIs, and a bare fragment the client could not expand back into the
//! IRI it named.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The test-suite context, which declares `Vehicle`, `speed` and `fuelLevel`
/// as terms for the `https://ngsi-ld-test-suite/context#` vocabulary.
const TEST_SUITE_CONTEXT: &str = "https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/raw/develop/resources/jsonld-contexts/ngsi-ld-test-suite.jsonld";
/// The expanded IRI the `Vehicle` term names.
const VEHICLE_TYPE_IRI: &str = "https://ngsi-ld-test-suite/context#Vehicle";
/// The expanded IRI the `speed` term names.
const SPEED_IRI: &str = "https://ngsi-ld-test-suite/context#speed";
/// The expanded IRI the `fuelLevel` term names.
const FUEL_IRI: &str = "https://ngsi-ld-test-suite/context#fuelLevel";

/// One Vehicle Temporal Evolution written under the test-suite context, so its
/// type and attributes expand to the test-suite vocabulary.
fn vehicle(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": [
            {"type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z"},
            {"type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z"}
        ],
        "fuelLevel": [
            {"type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z"}
        ],
        "@context": TEST_SUITE_CONTEXT
    })
}

/// Create the Vehicle and assert clause 6.20.3.1 answered 201.
async fn create(client: &TestClient, body: &Value) {
    let created = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create temporal entity: {:?}", created.body);
}

/// Clause 5.7.3 with clause 5.5.7: a retrieval that carries no `@context` is
/// rendered against the core context alone, so the test-suite type and
/// attributes -- which the core context does not declare -- stay full IRIs
/// rather than collapsing to `Vehicle` / `speed`.
#[tokio::test]
async fn regression_temporal_retrieve_no_context_keeps_full_iris() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-no-context-expansion");
    create(client, &vehicle(&id)).await;

    let response = client.get_temporal_entity(&id, "", None).await.expect("temporal retrieval request");
    assert_eq!(response.status, StatusCode::OK, "retrieval: {:?}", response.body);
    let body = response.json_value().expect("a temporal retrieval answers with JSON");

    assert_eq!(body["id"], json!(id), "the id is carried verbatim: {body}");
    assert_eq!(body["type"], json!(VEHICLE_TYPE_IRI), "an undeclared type stays a full IRI: {body}");
    assert!(body.get(SPEED_IRI).is_some(), "the speed sequence is keyed by its full IRI: {body}");
    assert!(body.get(FUEL_IRI).is_some(), "the fuelLevel sequence is keyed by its full IRI: {body}");
    assert!(body.get("speed").is_none(), "no term was truncated to a bare fragment: {body}");
    assert!(body.get("fuelLevel").is_none(), "no term was truncated to a bare fragment: {body}");

    client.delete_temporal_entity(&id).await.expect("delete temporal entity");
    scope.finish().await.expect("cleanup failed");
}

/// The same entity, retrieved under the context that declares its terms, comes
/// back compacted: `type` is `Vehicle` and the sequences are keyed by `speed`
/// and `fuelLevel` (clause 5.7.3).
#[tokio::test]
async fn regression_temporal_retrieve_with_context_compacts_terms() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:temporal-with-context-compaction");
    create(client, &vehicle(&id)).await;

    let response = client
        .get_temporal_entity(&id, "", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("temporal retrieval request");
    assert_eq!(response.status, StatusCode::OK, "retrieval: {:?}", response.body);
    let body = response.json_value().expect("a temporal retrieval answers with JSON");

    assert_eq!(body["id"], json!(id), "the id is carried verbatim: {body}");
    assert_eq!(body["type"], json!("Vehicle"), "the declared type compacts to its term: {body}");
    assert!(body.get("speed").is_some(), "the speed sequence compacts to its term: {body}");
    assert!(body.get("fuelLevel").is_some(), "the fuelLevel sequence compacts to its term: {body}");
    assert!(body.get(SPEED_IRI).is_none(), "the full IRI does not survive compaction: {body}");

    client.delete_temporal_entity(&id).await.expect("delete temporal entity");
    scope.finish().await.expect("cleanup failed");
}
