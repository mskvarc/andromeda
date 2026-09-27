//! Current projection delete: deletions remove exactly the grains they name.
//!
//! Deleting one dataset instance removes exactly its clause 4.5.5 grain,
//! deleting the Entity removes every remaining grain through the single
//! `entity_key` cascade, and concurrent projection writes cannot deadlock
//! against an asynchronous writer because none exists.

use crate::projection_rows::{entity_key, rows_for_attribute, rows_for_key};
use andromeda_test_client::entity_attribute::DatasetSelection;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use futures::future::join_all;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// `datasetId` of the instance each deletion test removes first.
const FIRST_DATASET: &str = "http://example.test/d1";

/// `datasetId` of the instance that must survive that deletion.
const SECOND_DATASET: &str = "urn:ngsi-ld:Dataset:d2";

/// A three-instance temperature attribute: the default instance plus the two
/// named datasets.
fn three_instance_building(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "type": "Building",
        "temperature": [
            {"type": "Property", "value": 1.0},
            {"type": "Property", "value": 2.0, "datasetId": FIRST_DATASET},
            {"type": "Property", "value": 3.0, "datasetId": SECOND_DATASET}
        ],
        "@context": [TEST_SUITE_CONTEXT]
    })
}

#[tokio::test]
async fn current_projection_dataset_delete_removes_only_that_grain() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:delete-one-dataset");

    let created = client.post_entity(&three_instance_building(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    assert_eq!(rows_for_attribute(scope.database_pool(), &id, "temperature").await.len(), 3);

    let deleted = client
        .delete_entity_attr(&id, "temperature", Some(FIRST_DATASET), DatasetSelection::Dataset, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("delete attribute instance failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete instance: {:?}", deleted.body);

    let remaining = rows_for_attribute(scope.database_pool(), &id, "temperature").await;
    let datasets: Vec<Option<&str>> = remaining.iter().map(|row| row.dataset_id.as_deref()).collect();
    assert_eq!(
        datasets,
        vec![None, Some(SECOND_DATASET)],
        "only the named grain may disappear, and the default instance is not a named grain"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_entity_delete_removes_every_remaining_row_through_one_cascade() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:delete-entity");

    let created = client.post_entity(&three_instance_building(&id)).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
    let key = entity_key(scope.database_pool(), &id)
        .await
        .expect("the created Entity must have a surrogate key");
    assert_eq!(rows_for_key(scope.database_pool(), key).await, 3);

    let deleted = client.delete_entity(&id).await.expect("delete entity failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete entity: {:?}", deleted.body);

    assert_eq!(entity_key(scope.database_pool(), &id).await, None, "the Entity row must be gone");
    assert_eq!(
        rows_for_key(scope.database_pool(), key).await,
        0,
        "every remaining grain must follow the Entity through the entity_key cascade"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_concurrent_writes_complete_without_deadlock() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:delete-concurrent");

    let entity = json!({
        "id": id,
        "type": "Building",
        "temperature": {"type": "Property", "value": 0.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    // Eight writers touching eight distinct grains of one Entity. With the
    // projection written inside the Entity transaction there is no second
    // writer to order against, so every request must succeed.
    let writes = (0..8).map(|index| {
        let fragment = json!({
            format!("attr{index}"): {"type": "Property", "value": index},
            "@context": [TEST_SUITE_CONTEXT]
        });
        let entity_id = id.clone(); // clone: each concurrent future owns the identifier it patches.
        async move { client.append_entity_attrs(&entity_id, &fragment).await }
    });
    for outcome in join_all(writes).await {
        let response = outcome.expect("append attributes failed");
        assert!(
            response.status == StatusCode::NO_CONTENT || response.status == StatusCode::CREATED,
            "concurrent projection write: {:?} {:?}",
            response.status,
            response.body
        );
    }

    let key = entity_key(scope.database_pool(), &id).await.expect("the Entity must still exist");
    assert_eq!(
        rows_for_key(scope.database_pool(), key).await,
        9,
        "the initial attribute plus eight concurrent appends must all be projected"
    );

    scope.finish().await.expect("cleanup failed");
}
