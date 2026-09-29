//! The default broker records Core writes: the contrast that shows the
//! setting, not the operation, is what leaves them out elsewhere.

use crate::{
    broker_roles::{Broker, client_for},
    evolution::{deletions, evolution, expected, observations, seed},
    observation::{core_context, entity, fragment, reading},
};
use andromeda_test_client::entity_attribute::DatasetSelection;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;

/// The instances every test seeds: `v` = 1 at minute 0 and 2 at minute 1.
const SEEDED: [(i64, u32); 2] = [(1, 0), (2, 1)];

/// Clause 4.3.1: the recording broker updates the Temporal Evolution
/// "whenever the 'current state' is modified via the Core API" -- on creation,
/// on update, on a restatement, and on deletion.
#[tokio::test]
async fn the_default_records_every_core_write() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let recording = client_for(scope.client(), Broker::Recorded);
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:recorded");

    let response = recording
        .post_entity(&entity(&id, &entity_type, json!({"v": reading(1, 0)})))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::CREATED);
    let response = recording
        .update_entity_attrs(&id, &fragment(json!({"v": reading(2, 1)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let response = recording
        .raw(Method::POST, "entityOperations/upsert?options=update")
        .header("content-type", "application/ld+json")
        .json(&json!([entity(&id, &entity_type, json!({"v": reading(2, 1)}))]))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let recorded = evolution(&recording, &id).await.expect("the Core writes made an evolution");
    assert_eq!(
        observations(&recorded, "v"),
        expected(&[(1, 0), (2, 1), (2, 1)]),
        "the restatement is recorded again"
    );

    let response = recording
        .delete_entity_attr(&id, "v", None, DatasetSelection::Default, Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let recorded = evolution(&recording, &id).await.expect("the evolution is kept");
    assert_eq!(deletions(&recorded, "v"), 1, "clause 4.5.8: the deletion is recorded: {recorded}");

    scope.finish().await.expect("cleanup failed");
}

/// Whether a write is recorded is decided by the broker that commits it: over
/// one database, the same update through each broker lands in history only
/// through the one that records Core writes.
#[tokio::test]
async fn the_committing_broker_decides_whether_a_core_write_is_recorded() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let recording = client_for(scope.client(), Broker::Recorded);
    let id = scope.id("Sensor:shared");
    seed(scope.client(), &id, &scope.entity_type("Sensor"), &SEEDED).await;

    let response = scope
        .client()
        .update_entity_attrs(&id, &fragment(json!({"v": reading(5, 2)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let response = recording
        .update_entity_attrs(&id, &fragment(json!({"v": reading(6, 3)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let recorded = evolution(scope.client(), &id).await.expect("the evolution exists");
    assert_eq!(observations(&recorded, "v"), expected(&[(1, 0), (2, 1), (6, 3)]));

    scope.finish().await.expect("cleanup failed");
}
