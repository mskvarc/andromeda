//! The identifier a Replace Entity stores (clause 5.6.18).
//!
//! Clause 5.6.18.4 replaces the target "with the same Entity ID with the new Entity content provided", and the target is the one the request URL names. A replacement that leaves its `id` out is therefore stored with the target's id, since a stored entity without one would make every query returning it fail, and a payload naming another entity is refused.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

const REPLACED_TYPE: &str = "https://example.org/regression/ReplacedIdentity";
const SPEED: &str = "https://example.org/regression/speed";

/// The core context the created entities are written against; every name here
/// is a full IRI.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

#[tokio::test]
async fn regression_replace_entity_without_an_id_stays_queryable() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("ReplacedIdentity:without-id");
    let created = client
        .post_entity(&json!({"id": id, "type": REPLACED_TYPE, SPEED: {"type": "Property", "value": 1}, "@context": CORE_CONTEXT}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let replacement = json!({"type": REPLACED_TYPE, SPEED: {"type": "Property", "value": 2}});
    let replaced = client
        .replace_entity(&id, &replacement, "application/json", None)
        .await
        .expect("replace entity failed");
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "{:?}", replaced.body);

    let query = format!("type={}&id={id}", REPLACED_TYPE.replace(':', "%3A").replace('/', "%2F"));
    let queried = client.query_entities(&query, "application/json", None).await.expect("query failed");
    assert_eq!(queried.status, StatusCode::OK, "a replaced entity must stay queryable: {:?}", queried.body);
    let entities: Value = queried.json_value().expect("a JSON body");
    assert_eq!(entities[0]["id"], Value::String(id.clone()));
    assert_eq!(entities[0][SPEED]["value"], 2);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn regression_replace_entity_naming_another_entity_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("ReplacedIdentity:target");
    let other = scope.id("ReplacedIdentity:other");
    let created = client
        .post_entity(&json!({"id": id, "type": REPLACED_TYPE, SPEED: {"type": "Property", "value": 1}, "@context": CORE_CONTEXT}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let replacement = json!({"id": other, "type": REPLACED_TYPE, SPEED: {"type": "Property", "value": 2}});
    let refused = client
        .replace_entity(&id, &replacement, "application/json", None)
        .await
        .expect("replace entity failed");
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{:?}", refused.body);
    let problem: Value = refused.json_value().expect("a ProblemDetails body");
    assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");

    let kept = client.get_entity(&id, "", "application/json", None).await.expect("retrieve failed");
    let entity: Value = kept.json_value().expect("a JSON body");
    assert_eq!(entity[SPEED]["value"], 1, "the refused replacement changed nothing");
    scope.finish().await.expect("cleanup failed");
}

/// A replacement holding nothing but the target's `id` expands to no node at
/// all (JSON-LD drops a top-level node carrying only its `@id`), and is a bad
/// request, not an internal error (clause 6.4.3.3).
#[tokio::test]
async fn regression_replace_entity_holding_only_an_id_is_a_bad_request() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("ReplacedIdentity:id-only");
    let created = client
        .post_entity(&json!({"id": id, "type": REPLACED_TYPE, SPEED: {"type": "Property", "value": 1}, "@context": CORE_CONTEXT}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let refused = client
        .replace_entity(&id, &json!({"id": id}), "application/json", None)
        .await
        .expect("replace entity failed");
    assert_eq!(refused.status, StatusCode::BAD_REQUEST, "{:?}", refused.body);
    let problem: Value = refused.json_value().expect("a ProblemDetails body");
    assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData");
    scope.finish().await.expect("cleanup failed");
}
