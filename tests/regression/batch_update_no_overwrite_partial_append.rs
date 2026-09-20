//! Batch update with `noOverwrite` distinguishes a partial append (204) from an
//! all-skipped update (207) per entity.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.6.9.4 runs each entity through the local
//! Append Attributes of clause 5.6.3 with overwrite disabled, and clause 5.6.9.5
//! adds an entity to the success set S when it "was successfully updated
//! (Attributes appended)". A fragment that appends at least one attribute -- even
//! beside attributes it skips for `noOverwrite` -- is therefore a success, and an
//! all-success batch is 204; a fragment that appends nothing (every named
//! attribute already present) leaves the entity out of S, so it is reported in
//! the error set E and the batch is 207. This pins both the per-entity split and
//! the exact `BadRequestData` the skipped entity carries.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::{context::CORE_CONTEXT, problem_detail::BAD_REQUEST_DATA};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The exact detail an entity whose `noOverwrite` append added nothing carries in
/// the batch error set (clause 5.6.9.4/5.6.9.5).
const NO_OVERWRITE_DETAIL: &str = "no attributes were appended: every named attribute already exists and noOverwrite was set";

/// A `Building` carrying a `name` and a `subCategory`, both under the core
/// context.
fn building(id: &str, name: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": name},
        "subCategory": {"type": "Property", "value": "tourism"},
        "@context": [CORE_CONTEXT]
    })
}

/// Create one entity, asserting it was accepted.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);
}

/// POST an array of update fragments with `options=noOverwrite`, returning the
/// response status and its decoded body.
async fn batch_update_no_overwrite(client: &TestClient, updates: &Value) -> (StatusCode, Value) {
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/update?options=noOverwrite")
        .header("content-type", "application/ld+json")
        .json(updates)
        .send()
        .await
        .expect("batch update request");
    let status = response.status;
    let body = response.json_value().unwrap_or(Value::Null);
    (status, body)
}

/// Clause 5.6.9.4/5.6.9.5: a fragment mixing a present attribute with a new one
/// appends the new one, so the entity enters S and an all-S batch is 204; the
/// present attribute keeps its original value.
#[tokio::test]
async fn regression_batch_update_no_overwrite_partial_append_returns_204() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:nooverwrite-partial");
    create(client, &building(&id, "Eiffel Tower")).await;

    let fragment = json!({
        "id": &id,
        "type": "Building",
        "name": {"type": "Property", "value": "Tour Eiffel"},
        "locatedAt": {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"},
        "@context": [CORE_CONTEXT]
    });
    let (status, _) = batch_update_no_overwrite(client, &json!([fragment])).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "a fragment that appends at least one attribute is a 204");

    let read = client.get_entity(&id, "", "application/ld+json", None).await.expect("read-back");
    assert_eq!(read.status, StatusCode::OK);
    let body = read.json_value().expect("entity body is JSON");
    assert_eq!(
        body["name"]["value"], "Eiffel Tower",
        "noOverwrite preserved the present attribute's original value"
    );
    assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris", "the new attribute was appended");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.9.4/5.6.9.5: the success/error split is per entity -- one entity
/// whose fragment appends a new attribute is in `success`, another whose fragment
/// names only present attributes is in `errors` with a `BadRequestData`, and the
/// batch is 207.
#[tokio::test]
async fn regression_batch_update_no_overwrite_per_entity_success_error_split_returns_207() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let appended = scope.id("Building:nooverwrite-split-appended");
    let skipped = scope.id("Building:nooverwrite-split-skipped");
    create(client, &building(&appended, "Eiffel Tower")).await;
    create(client, &building(&skipped, "Louvre")).await;

    // The first fragment appends a new attribute (-> S); the second names only
    // attributes that already exist (-> E).
    let appends = json!({
        "id": &appended,
        "type": "Building",
        "name": {"type": "Property", "value": "Tour Eiffel"},
        "locatedAt": {"type": "Relationship", "object": "urn:ngsi-ld:City:Paris"},
        "@context": [CORE_CONTEXT]
    });
    let all_present = building(&skipped, "Musee du Louvre");
    let (status, body) = batch_update_no_overwrite(client, &json!([appends, all_present])).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "a per-entity success/error split is 207: {body}");

    let success = body["success"].as_array().expect("the result carries a success array");
    assert!(
        success.iter().any(|entry| entry == &json!(appended)),
        "the appended entity is in the success set: {body}"
    );

    let errors = body["errors"].as_array().expect("the result carries an errors array");
    let skipped_error = errors
        .iter()
        .find(|entry| entry["entityId"] == json!(skipped))
        .unwrap_or_else(|| panic!("the skipped entity is in the error set: {body}"));
    assert_eq!(
        skipped_error["error"]["type"].as_str(),
        Some(BAD_REQUEST_DATA.as_str()),
        "the skipped entity is a BadRequestData: {body}"
    );
    assert_eq!(skipped_error["error"]["status"], json!(400), "the BadRequestData carries a 400 status: {body}");
    assert_eq!(
        skipped_error["error"]["detail"],
        json!(NO_OVERWRITE_DETAIL),
        "the error explains that nothing was appended: {body}"
    );

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.9.4: a single entity whose `noOverwrite` fragment names only
/// present attributes appends nothing, so it is reported in `errors` and the
/// batch is 207 -- the boundary partner of the 204 partial-append case.
#[tokio::test]
async fn regression_batch_update_no_overwrite_all_skipped_returns_207() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Building:nooverwrite-all-skipped");
    create(client, &building(&id, "Eiffel Tower")).await;

    let all_present = building(&id, "Tour Eiffel");
    let (status, body) = batch_update_no_overwrite(client, &json!([all_present])).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "an all-skipped noOverwrite update is 207: {body}");

    let errors = body["errors"].as_array().expect("the result carries an errors array");
    let error = errors
        .iter()
        .find(|entry| entry["entityId"] == json!(id))
        .unwrap_or_else(|| panic!("the skipped entity is in the error set: {body}"));
    assert_eq!(
        error["error"]["type"].as_str(),
        Some(BAD_REQUEST_DATA.as_str()),
        "the skipped entity is a BadRequestData: {body}"
    );
    assert_eq!(error["error"]["status"], json!(400), "the BadRequestData carries a 400 status: {body}");
    assert_eq!(
        error["error"]["detail"],
        json!(NO_OVERWRITE_DETAIL),
        "the error explains that nothing was appended: {body}"
    );

    scope.finish().await.expect("environment cleanup");
}
