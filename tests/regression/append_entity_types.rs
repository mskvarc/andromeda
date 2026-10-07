//! ETSI GS CIM 009 v1.9.1 clause 5.6.3.4: "If type is included in the Fragment and it includes Entity Type names that are not yet in the target Entity, add them to the list of Entity Type names of the target Entity." The rule stands beside the per-attribute rules, so it holds whether or not overwrite is allowed, and clause 5.6.9.4 runs Batch Entity Update with `noOverwrite` through the same Append Attributes.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// A `Vehicle` carrying a `speed` of 80, under the core context.
fn vehicle(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 80},
        "@context": [CORE_CONTEXT]
    })
}

/// Create one entity, asserting it was accepted.
async fn create(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);
}

/// The entity `id` as stored, answering 200.
async fn stored(client: &TestClient, id: &str) -> Value {
    let read = client.get_entity(id, "", "application/ld+json", None).await.expect("retrieve entity request");
    assert_eq!(read.status, StatusCode::OK, "retrieve {id}: {:?}", read.body);
    read.json_value().expect("an entity body")
}

/// The Entity Type names of `entity` in sorted order, whether rendered as one
/// string or as an array.
fn types(entity: &Value) -> Vec<&str> {
    let rendered = &entity["type"];
    let mut names = rendered.as_array().map_or_else(
        || vec![rendered.as_str().expect("one type name")],
        |names| names.iter().map(|name| name.as_str().expect("a type name")).collect(),
    );
    names.sort_unstable();
    names
}

/// POST `fragment` to the Append Attributes endpoint of `id` under `query`.
async fn append(client: &TestClient, id: &str, query: &str, fragment: &Value) -> StatusCode {
    let response = client
        .raw(Method::POST, &format!("/ngsi-ld/v1/entities/{}/attrs{query}", urlencoding::encode(id)))
        .header("content-type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("append attributes request");
    response.status
}

/// Clause 5.6.3.4 with overwrite allowed: the fragment's `Car` joins `Vehicle`,
/// and `Vehicle`, already held, is not repeated.
#[tokio::test]
async fn regression_r04_append_adds_the_fragment_types_the_entity_lacks() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:append-types");
    create(client, &vehicle(&id)).await;

    let fragment = json!({
        "type": ["Vehicle", "Car"],
        "brand": {"type": "Property", "value": "Mercedes"},
        "@context": [CORE_CONTEXT]
    });
    assert_eq!(append(client, &id, "", &fragment).await, StatusCode::NO_CONTENT);

    let entity = stored(client, &id).await;
    assert_eq!(types(&entity), vec!["Car", "Vehicle"], "{entity}");
    assert_eq!(entity["brand"]["value"], json!("Mercedes"), "{entity}");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.3.4 under `noOverwrite`: the held `speed` is left untouched, so
/// the append is partial (207), and the fragment's `Car` is still added.
#[tokio::test]
async fn regression_r04_append_with_no_overwrite_adds_the_types_even_when_the_attributes_are_kept() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:append-types-no-overwrite");
    create(client, &vehicle(&id)).await;

    let fragment = json!({
        "type": "Car",
        "speed": {"type": "Property", "value": 120},
        "@context": [CORE_CONTEXT]
    });
    assert_eq!(append(client, &id, "?options=noOverwrite", &fragment).await, StatusCode::MULTI_STATUS);

    let entity = stored(client, &id).await;
    assert_eq!(types(&entity), vec!["Car", "Vehicle"], "{entity}");
    assert_eq!(entity["speed"]["value"], json!(80), "noOverwrite kept the held speed: {entity}");

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.9.4 runs Batch Entity Update with `noOverwrite` as an Append Attributes, so the fragment's `Car` is added there too.
#[tokio::test]
async fn regression_r04_batch_update_with_no_overwrite_adds_the_fragment_types() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:batch-update-types");
    create(client, &vehicle(&id)).await;

    let fragment = json!({
        "id": &id,
        "type": "Car",
        "brand": {"type": "Property", "value": "Mercedes"},
        "@context": [CORE_CONTEXT]
    });
    let response = client
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/update?options=noOverwrite")
        .header("content-type", "application/ld+json")
        .json(&json!([fragment]))
        .send()
        .await
        .expect("batch update request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "batch update: {:?}", response.body);

    let entity = stored(client, &id).await;
    assert_eq!(types(&entity), vec!["Car", "Vehicle"], "{entity}");
    assert_eq!(entity["brand"]["value"], json!("Mercedes"), "{entity}");

    scope.finish().await.expect("environment cleanup");
}
