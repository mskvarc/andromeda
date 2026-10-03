//! A written sub-attribute instance lands only on the held instance with its
//! `datasetId`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.12: "In the case of a member representing
//! a reified Property or Relationship including a datasetId, such member is only
//! updated if the datasetId is the same, otherwise the member of the Fragment is
//! added as a new instance to the target. If no datasetId is present, the
//! default Attribute instance is targeted and merged if present and otherwise
//! added." The merge goes "up to an arbitrary depth", and a sub-attribute is a
//! Property or Relationship of its attribute (clause 4.5.2.2). Clause 4.5.5.1:
//! "only instances with the same datasetId are affected, leaving instances with
//! another datasetId or an instance without a datasetId untouched." Clause 5.5.8
//! states the same for the Partial Attribute Update (clause 5.6.4).
//!
//! The broker landed a sub-attribute as one value: a single written instance
//! merged into a single held one whatever their `datasetId`s, taking the written
//! `datasetId`, so `accuracy` B merged onto a held `accuracy` A left one instance
//! B and A was lost; any other shape replaced every held instance, and a
//! deletion naming one `datasetId` removed them all.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::{collections::BTreeMap, slice};

const DATASET_A: &str = "urn:ngsi-ld:Dataset:A";
const DATASET_B: &str = "urn:ngsi-ld:Dataset:B";

/// The key [`accuracy_instances`] gives the default instance, which clause
/// 4.5.5.1 lets a request name as `"@none"`.
const DEFAULT_INSTANCE: &str = "@none";

/// Create one `Vehicle` whose default `speed` instance carries `accuracy` as
/// `held`, and assert clause 6.4.3.1 answered with 201.
async fn create_vehicle(client: &TestClient, id: &str, held: &Value) {
    let entity = json!({
        "id": id,
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 10, "accuracy": held},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "create {id}: {:?}", response.body);
}

/// Merge a `speed` fragment carrying `accuracy` as `written`, and assert
/// clause 6.5.3.4 answered with 204.
async fn merge_speed(client: &TestClient, id: &str, written: &Value) {
    let fragment = json!({
        "speed": {"type": "Property", "value": 20, "accuracy": written},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.merge_entity(id, &fragment, None).await.expect("merge entity request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "merge {id}: {:?}", response.body);
}

/// The stored entity, compacted with the test-suite context.
async fn stored_entity(client: &TestClient, id: &str) -> Value {
    let response = client
        .get_entity(id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id}: {:?}", response.body);
    response.json_value().expect("the retrieved entity is JSON")
}

/// The `accuracy` instances of the stored `speed`, keyed by `datasetId`
/// ([`DEFAULT_INSTANCE`] for the default one), so the assertions do not
/// depend on instance order. One instance renders as an object and several
/// as an array (clause 4.5.5.1).
fn accuracy_instances(entity: &Value) -> BTreeMap<&str, &Value> {
    let instances = match &entity["speed"]["accuracy"] {
        Value::Array(instances) => instances.as_slice(),
        Value::Null => &[],
        instance @ (Value::Bool(_) | Value::Number(_) | Value::String(_) | Value::Object(_)) => slice::from_ref(instance),
    };
    instances
        .iter()
        .map(|instance| (instance["datasetId"].as_str().unwrap_or(DEFAULT_INSTANCE), instance))
        .collect()
}

/// The `value` of every `accuracy` instance of the stored `speed`, keyed as
/// [`accuracy_instances`] keys them.
fn accuracy_values(entity: &Value) -> BTreeMap<&str, &Value> {
    accuracy_instances(entity)
        .into_iter()
        .map(|(dataset_id, instance)| (dataset_id, &instance["value"]))
        .collect()
}

/// The expected [`accuracy_values`].
fn expected<'a>(values: &'a [(&'a str, Value)]) -> BTreeMap<&'a str, &'a Value> {
    values.iter().map(|(dataset_id, value)| (*dataset_id, value)).collect()
}

/// Clauses 5.5.12 and 5.6.17.4: a written `datasetId` the held instances do
/// not carry is added beside them. Before the fix A was merged into B and
/// lost: `{B: 0.2}`.
#[tokio::test]
async fn regression_sub_attribute_dataset_merge_adds_another_dataset_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:sadm-other-dataset");
    create_vehicle(client, &id, &json!({"type": "Property", "value": 0.1, "datasetId": DATASET_A})).await;

    merge_speed(client, &id, &json!({"type": "Property", "value": 0.2, "datasetId": DATASET_B})).await;

    let entity = stored_entity(client, &id).await;
    assert_eq!(entity["speed"]["value"], json!(20));
    assert_eq!(accuracy_values(&entity), expected(&[(DATASET_A, json!(0.1)), (DATASET_B, json!(0.2))]));

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12: a written instance naming no `datasetId` targets the
/// default instance, "merged if present and otherwise added". Before the fix
/// it merged into A: `{A: 0.2}`.
#[tokio::test]
async fn regression_sub_attribute_dataset_merge_adds_a_default_instance_beside_a_dataset_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:sadm-default");
    create_vehicle(client, &id, &json!({"type": "Property", "value": 0.1, "datasetId": DATASET_A})).await;

    merge_speed(client, &id, &json!({"type": "Property", "value": 0.2})).await;

    let entity = stored_entity(client, &id).await;
    assert_eq!(accuracy_values(&entity), expected(&[(DATASET_A, json!(0.1)), (DEFAULT_INSTANCE, json!(0.2))]));

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.5.12 and 4.5.5.1: the instance with the written `datasetId` is
/// merged, keeping the `observedAt` the fragment omits, and the other is left
/// untouched. Before the fix A was rebuilt from the fragment alone and B was
/// lost.
#[tokio::test]
async fn regression_sub_attribute_dataset_merge_merges_only_the_matching_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:sadm-matching");
    create_vehicle(
        client,
        &id,
        &json!([
            {"type": "Property", "value": 0.1, "observedAt": "2020-01-01T00:00:00Z", "datasetId": DATASET_A},
            {"type": "Property", "value": 0.5, "datasetId": DATASET_B}
        ]),
    )
    .await;

    merge_speed(client, &id, &json!({"type": "Property", "value": 0.2, "datasetId": DATASET_A})).await;

    let entity = stored_entity(client, &id).await;
    assert_eq!(accuracy_values(&entity), expected(&[(DATASET_A, json!(0.2)), (DATASET_B, json!(0.5))]));
    assert_eq!(
        accuracy_instances(&entity)[DATASET_A]["observedAt"],
        json!("2020-01-01T00:00:00Z"),
        "merged, not replaced"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.12 hands the deletion of "a specific Attribute instance with a
/// datasetId" to clause 5.6.5, which removes that instance alone (clause
/// 5.6.5.4). Before the fix the whole `accuracy` was deleted.
#[tokio::test]
async fn regression_sub_attribute_dataset_merge_deletes_only_the_named_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:sadm-deletion");
    create_vehicle(
        client,
        &id,
        &json!([
            {"type": "Property", "value": 0.1},
            {"type": "Property", "value": 0.5, "datasetId": DATASET_A}
        ]),
    )
    .await;

    merge_speed(client, &id, &json!({"type": "Property", "value": "urn:ngsi-ld:null", "datasetId": DATASET_A})).await;

    let entity = stored_entity(client, &id).await;
    assert_eq!(accuracy_values(&entity), expected(&[(DEFAULT_INSTANCE, json!(0.1))]));

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.4 and 5.5.8: a partially updated attribute's sub-attribute
/// "is only replaced if the datasetId is the same, otherwise the member of the
/// Fragment is added as a new instance". Before the fix A was dropped: `{B:
/// 0.2}`.
#[tokio::test]
async fn regression_sub_attribute_dataset_partial_update_adds_another_dataset_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    let id = scope.id("Vehicle:sadm-partial-update");
    create_vehicle(client, &id, &json!({"type": "Property", "value": 0.1, "datasetId": DATASET_A})).await;

    let fragment = json!({
        "value": 20,
        "accuracy": {"type": "Property", "value": 0.2, "datasetId": DATASET_B},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client
        .partial_update_attr(&id, "speed", &fragment)
        .await
        .expect("partial attribute update request");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "partial update {id}: {:?}", response.body);

    let entity = stored_entity(client, &id).await;
    assert_eq!(entity["speed"]["value"], json!(20));
    assert_eq!(accuracy_values(&entity), expected(&[(DATASET_A, json!(0.1)), (DATASET_B, json!(0.2))]));

    scope.finish().await.expect("cleanup failed");
}
