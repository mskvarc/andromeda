//! `DELETE /entities` with `datasetId` / `deleteAll` — the purge dataset-scope
//! extension.
//!
//! Runs against a broker started with `ANDROMEDA__EXTENSIONS__PURGE_DATASET_SCOPE=enabled`.

use crate::seeding::seed_two_dataset_instances;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::Value;

const DATASET: &str = "urn:ngsi-ld:Dataset:one";

/// The entity read back, or `None` when it no longer exists.
async fn read_entity(client: &TestClient, id: &str) -> Option<Value> {
    let response = client.get_entity(id, "", "application/ld+json", Some(CORE_CONTEXT)).await.expect("read entity");
    if response.status == StatusCode::NOT_FOUND {
        return None;
    }
    assert_eq!(response.status, StatusCode::OK, "unexpected read status");
    Some(response.json_value().expect("entity body is JSON"))
}

/// The `datasetId` values the `speed` attribute of a normalized entity carries;
/// the default instance contributes the sentinel `@none`.
fn speed_dataset_ids(entity: &Value) -> Vec<String> {
    let member = &entity["speed"];
    let instances = match member {
        Value::Array(instances) => instances.clone(),
        Value::Object(_) => vec![member.clone()],
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Vec::new(),
    };
    instances
        .iter()
        .map(|instance| instance["datasetId"].as_str().unwrap_or("@none").to_owned())
        .collect()
}

/// `drop` with a `datasetId` removes only that instance, leaving the default.
#[tokio::test]
async fn a_dataset_scoped_drop_removes_only_the_named_instance() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:dataset-drop");
    seed_two_dataset_instances(client, &id, DATASET).await;

    let query = format!(
        "type=Vehicle&id={}&drop=speed&datasetId={}",
        urlencoding::encode(&id),
        urlencoding::encode(DATASET)
    );
    let response = client.delete_entities_by_query(&query, Some(CORE_CONTEXT)).await.expect("purge");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let entity = read_entity(client, &id).await.expect("entity still exists");
    let datasets = speed_dataset_ids(&entity);
    assert!(datasets.contains(&"@none".to_string()), "the default instance survives: {datasets:?}");
    assert!(!datasets.contains(&DATASET.to_string()), "the named instance is gone: {datasets:?}");
    scope.finish().await.expect("cleanup");
}

/// `deleteAll` removes every instance — the whole attribute.
#[tokio::test]
async fn delete_all_removes_the_whole_attribute() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:dataset-deleteall");
    seed_two_dataset_instances(client, &id, DATASET).await;

    let query = format!("type=Vehicle&id={}&drop=speed&deleteAll=true", urlencoding::encode(&id));
    let response = client.delete_entities_by_query(&query, Some(CORE_CONTEXT)).await.expect("purge");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let entity = read_entity(client, &id).await.expect("entity still exists");
    assert!(entity["speed"].is_null(), "deleteAll removes every speed instance: {entity}");
    scope.finish().await.expect("cleanup");
}

/// A bare `drop` — no `datasetId`, no `deleteAll` — removes the whole attribute,
/// the standard clause 5.6.21 default preserved by the extension.
#[tokio::test]
async fn a_bare_drop_removes_the_whole_attribute() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:dataset-baredrop");
    seed_two_dataset_instances(client, &id, DATASET).await;

    let query = format!("type=Vehicle&id={}&drop=speed", urlencoding::encode(&id));
    let response = client.delete_entities_by_query(&query, Some(CORE_CONTEXT)).await.expect("purge");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let entity = read_entity(client, &id).await.expect("entity still exists");
    assert!(entity["speed"].is_null(), "a bare drop removes the whole attribute: {entity}");
    scope.finish().await.expect("cleanup");
}

/// `keep` with a `datasetId` removes the named instance of every attribute
/// except the kept one, sparing the kept attribute entirely.
#[tokio::test]
async fn a_dataset_scoped_keep_spares_the_kept_attribute() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:dataset-keep");
    seed_two_dataset_instances(client, &id, DATASET).await;

    // Keep speed: brandName is the non-kept attribute, but it carries no named
    // instance, so nothing of it is removed; speed is spared entirely.
    let query = format!(
        "type=Vehicle&id={}&keep=speed&datasetId={}",
        urlencoding::encode(&id),
        urlencoding::encode(DATASET)
    );
    let response = client.delete_entities_by_query(&query, Some(CORE_CONTEXT)).await.expect("purge");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let entity = read_entity(client, &id).await.expect("entity still exists");
    let datasets = speed_dataset_ids(&entity);
    assert!(
        datasets.contains(&DATASET.to_string()),
        "the kept attribute's named instance survives: {datasets:?}"
    );
    assert!(
        datasets.contains(&"@none".to_string()),
        "the kept attribute's default instance survives: {datasets:?}"
    );
    scope.finish().await.expect("cleanup");
}
