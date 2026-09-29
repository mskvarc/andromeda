//! Attribute-level Core writes leave the Temporal Evolution as it was.

use crate::{
    current_state::current,
    evolution::{assert_holds_only, deletions, evolution, observations, seed},
    observation::{at, core_context, entity, fragment, reading},
};
use andromeda_test_client::entity_attribute::DatasetSelection;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use urlencoding::encode;

/// The instances every test seeds: `v` = 1 at minute 0 and 2 at minute 1.
const SEEDED: [(i64, u32); 2] = [(1, 0), (2, 1)];

/// A seeded Entity for the test `local` names.
async fn seeded(scope: &TestEnvironment, local: &str) -> String {
    let id = scope.id(local);
    seed(scope.client(), &id, &scope.entity_type("Sensor"), &SEEDED).await;
    id
}

#[tokio::test]
async fn appending_attributes_records_neither_the_new_nor_the_restated_ones() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = seeded(&scope, "Sensor:appended").await;

    let response = scope
        .client()
        .append_entity_attrs(&id, &fragment(json!({"v": reading(5, 2), "w": reading(3, 2)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    let stored = current(scope.client(), &id).await.expect("the entity exists");
    assert_eq!((stored["v"]["value"].clone(), stored["w"]["value"].clone()), (json!(5), json!(3)));
    assert_holds_only(scope.client(), &id, &SEEDED).await;
    let recorded = evolution(scope.client(), &id).await.expect("the evolution is kept");
    assert_eq!(observations(&recorded, "w"), Vec::new());

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn updating_attributes_records_nothing() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = seeded(&scope, "Sensor:updated").await;

    let response = scope
        .client()
        .update_entity_attrs(&id, &fragment(json!({"v": reading(5, 2)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    assert_eq!(current(scope.client(), &id).await.expect("the entity exists")["v"]["value"], 5);
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn partially_updating_an_attribute_records_nothing() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = seeded(&scope, "Sensor:partially-updated").await;

    let response = scope
        .client()
        .partial_update_attr(&id, "v", &fragment(json!({"value": 6, "observedAt": at(3)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    let stored = current(scope.client(), &id).await.expect("the entity exists");
    assert_eq!(stored["v"]["value"], 6);
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn replacing_an_attribute_records_nothing() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = seeded(&scope, "Sensor:attribute-replaced").await;

    let mut replacement = fragment(json!({}));
    replacement["type"] = json!("Property");
    replacement["value"] = json!(9);
    replacement["observedAt"] = json!(at(5));
    let response = scope
        .client()
        .raw(Method::PUT, &format!("entities/{}/attrs/v", encode(&id)))
        .header("content-type", "application/ld+json")
        .json(&replacement)
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    assert_eq!(current(scope.client(), &id).await.expect("the entity exists")["v"]["value"], 9);
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn deleting_an_attribute_records_no_deletion() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = seeded(&scope, "Sensor:attribute-deleted").await;

    let response = scope
        .client()
        .delete_entity_attr(&id, "v", None, DatasetSelection::Default, Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    assert_eq!(current(scope.client(), &id).await.expect("the entity exists").get("v"), None);
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

/// Deleting one `datasetId` instance removes it from current state and leaves
/// both instances' histories as the Temporal API wrote them.
#[tokio::test]
async fn deleting_one_dataset_instance_records_no_deletion() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = scope.id("Sensor:dataset-deleted");
    let dataset = "urn:ngsi-ld:Dataset:secondary";
    let mut secondary = reading(10, 0);
    secondary["datasetId"] = json!(dataset);
    let response = scope
        .client()
        .create_temporal_entity(&entity(&id, &scope.entity_type("Sensor"), json!({"v": [reading(1, 0), secondary]})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&response.body));

    let response = scope
        .client()
        .delete_entity_attr(&id, "v", Some(dataset), DatasetSelection::Dataset, Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    let stored = current(scope.client(), &id).await.expect("the entity exists");
    assert_eq!(stored["v"]["value"], 1, "the default instance stands alone: {stored}");
    let recorded = evolution(scope.client(), &id).await.expect("the evolution is kept");
    let mut values: Vec<i64> = observations(&recorded, "v").into_iter().filter_map(|(value, _)| value.as_i64()).collect();
    values.sort_unstable();
    assert_eq!(values, vec![1, 10]);
    assert_eq!(deletions(&recorded, "v"), 0, "{recorded}");

    scope.finish().await.expect("cleanup failed");
}
