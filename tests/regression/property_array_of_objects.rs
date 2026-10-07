//! A Property whose value is an array holding several JSON objects.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.2.5 puts no restriction on a Property
//! `value`: Table 5.2.5-1 gives it the data type "JSON value", so an array of
//! objects, and an array mixing objects with scalars, are admissible. `{"a":1}`,
//! `[{"a":1}]`, `[{"a":1},{"b":2}]` and `[1,{"a":1}]` are all accepted: JSON-LD
//! expansion turns each object into a node object rather than a value node, so a
//! history decoder demanding `@value` from every element of a multi-element array
//! would wrongly refuse them with `400 BadRequestData` "Property array element
//! carries no value".
//!
//! Every write path that records the value is exercised, and each asserts the
//! current representation and the Temporal Evolution give back the array with
//! its object keys intact.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// The window every temporal read of this module asks for.
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=modifiedAt";

/// An array of zone objects, as in a Country's `timezones`.
fn zones() -> Value {
    json!([
        {"zoneName": "Europe/Andorra", "gmtOffset": 3600},
        {"zoneName": "Asia/Dubai", "gmtOffset": 14400}
    ])
}

/// An array mixing a scalar with an object.
fn mixed() -> Value {
    json!([1, {"a": 1}])
}

/// One Property carrying `value`.
fn property(value: &Value) -> Value {
    json!({"type": "Property", "value": value})
}

/// The current representation of one Entity (clause 5.7.1).
async fn current(client: &TestClient, id: &str) -> Value {
    let response = client
        .get_entity(id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "current retrieval of {id}: {:?}", response.body);
    response.json_value().expect("an entity is JSON")
}

/// The Temporal Evolution of one attribute of one Entity (clause 5.7.3).
async fn evolution(client: &TestClient, id: &str, name: &str) -> Value {
    let response = client
        .get_temporal_entity(id, &format!("{WINDOW}&attrs={name}"), Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "temporal retrieval of {id}: {:?}", response.body);
    response.json_value().expect("a temporal answer is JSON")
}

/// Assert the current value and every recorded instance of `name` are `expected`.
async fn assert_round_trips(client: &TestClient, id: &str, name: &str, expected: &Value, instances: usize) {
    let current = current(client, id).await;
    assert_eq!(&current[name]["value"], expected, "the current representation keeps the array: {current}");
    let evolution = evolution(client, id, name).await;
    let recorded = evolution[name].as_array().map_or_else(|| vec![evolution[name].clone()], Clone::clone);
    assert_eq!(recorded.len(), instances, "one instance per write: {evolution}");
    let last = recorded.last().expect("at least one instance");
    assert_eq!(&last["value"], expected, "the Temporal Evolution keeps the array: {evolution}");
}

/// Create one Entity carrying `name` with `value` through clause 5.6.1.
async fn create(client: &TestClient, id: &str, name: &str, value: &Value) {
    let response = client
        .post_entity(&json!({"id": id, "type": "ArrayOfObjects", name: property(value), "@context": CORE_CONTEXT}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {id}: {:?}", response.body);
}

/// Clause 5.6.1: Create Entity accepts an array of several objects.
#[tokio::test]
async fn regression_property_array_of_objects_create_entity_accepts_several_objects() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("ArrayOfObjects:create");

    create(scope.client(), &id, "timezones", &zones()).await;
    assert_round_trips(scope.client(), &id, "timezones", &zones(), 1).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.1: an array mixing a scalar with an object is accepted too.
#[tokio::test]
async fn regression_property_array_of_objects_create_entity_accepts_a_scalar_beside_an_object() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("ArrayOfObjects:mixed");

    create(scope.client(), &id, "mixed", &mixed()).await;
    assert_round_trips(scope.client(), &id, "mixed", &mixed(), 1).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.8: Batch Upsert creates and then updates the Entity with the
/// array, and neither write fails the batch.
#[tokio::test]
async fn regression_property_array_of_objects_batch_upsert_creates_and_updates() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("ArrayOfObjects:upsert");
    let entity = |value: &Value| json!([{"id": id, "type": "ArrayOfObjects", "timezones": property(value), "@context": CORE_CONTEXT}]);

    let created = scope.client().batch_upsert(&entity(&mixed())).await.expect("batch upsert request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let updated = scope.client().batch_upsert(&entity(&zones())).await.expect("batch upsert request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);

    assert_round_trips(scope.client(), &id, "timezones", &zones(), 2).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.7: Batch Create accepts the array for every Entity of the batch.
#[tokio::test]
async fn regression_property_array_of_objects_batch_create_accepts_every_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let first = scope.id("ArrayOfObjects:batch-1");
    let second = scope.id("ArrayOfObjects:batch-2");
    let body = json!([
        {"id": first, "type": "ArrayOfObjects", "timezones": property(&zones()), "@context": CORE_CONTEXT},
        {"id": second, "type": "ArrayOfObjects", "timezones": property(&mixed()), "@context": CORE_CONTEXT}
    ]);

    let response = scope.client().batch_create(&body).await.expect("batch create request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);

    assert_round_trips(scope.client(), &first, "timezones", &zones(), 1).await;
    assert_round_trips(scope.client(), &second, "timezones", &mixed(), 1).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.17 (Merge Entity) replaces the scalar value with the array.
#[tokio::test]
async fn regression_property_array_of_objects_merge_entity_writes_the_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("ArrayOfObjects:merge");
    create(scope.client(), &id, "timezones", &json!("none")).await;

    let response = scope
        .client()
        .merge_entity(&id, &json!({"timezones": property(&zones()), "@context": CORE_CONTEXT}), None)
        .await
        .expect("merge entity request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{:?}", response.body);

    assert_round_trips(scope.client(), &id, "timezones", &zones(), 2).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.2 (Update Attributes) and 5.6.3 (Append Attributes) write the
/// array onto an existing Entity.
#[tokio::test]
async fn regression_property_array_of_objects_update_and_append_attributes_write_the_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("ArrayOfObjects:attrs");
    create(scope.client(), &id, "timezones", &json!("none")).await;

    let updated = scope
        .client()
        .update_entity_attrs(&id, &json!({"timezones": property(&zones()), "@context": CORE_CONTEXT}))
        .await
        .expect("update attributes request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);
    let appended = scope
        .client()
        .append_entity_attrs(&id, &json!({"mixed": property(&mixed()), "@context": CORE_CONTEXT}))
        .await
        .expect("append attributes request");
    assert_eq!(appended.status, StatusCode::NO_CONTENT, "{:?}", appended.body);

    assert_round_trips(scope.client(), &id, "timezones", &zones(), 2).await;
    assert_round_trips(scope.client(), &id, "mixed", &mixed(), 1).await;

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.11: a Temporal Evolution whose instances carry the array is
/// created, and each instance keeps it.
#[tokio::test]
async fn regression_property_array_of_objects_temporal_create_keeps_every_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let id = scope.id("ArrayOfObjects:temporal");
    let body = json!({
        "id": id,
        "type": "ArrayOfObjects",
        "timezones": [
            {"type": "Property", "value": mixed(), "observedAt": "2024-01-01T00:00:00Z"},
            {"type": "Property", "value": zones(), "observedAt": "2024-01-02T00:00:00Z"}
        ],
        "@context": CORE_CONTEXT
    });
    let response = scope.client().create_temporal_entity(&body).await.expect("create temporal entity request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);

    let evolution = scope
        .client()
        .get_temporal_entity(&id, "timerel=after&timeAt=2000-01-01T00:00:00Z", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(evolution.status, StatusCode::OK, "{:?}", evolution.body);
    let evolution = evolution.json_value().expect("a temporal answer is JSON");
    let values: Vec<&Value> = evolution["timezones"]
        .as_array()
        .expect("two instances")
        .iter()
        .map(|instance| &instance["value"])
        .collect();
    assert_eq!(values, vec![&mixed(), &zones()], "{evolution}");

    scope.finish().await.expect("cleanup failed");
}
