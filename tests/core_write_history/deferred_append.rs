//! Core writes stay out of history under the deferred append mode too.
//!
//! A deferred broker queues the Temporal API's history and a drain lands it
//! after the write answers, so absence cannot be read straight after a Core
//! write. Each test writes a Temporal API marker after the Core write and
//! waits for the marker: once it has landed, anything the Core write had
//! queued before it would have landed as well.

use crate::{
    broker_roles::{Broker, client_for},
    current_state::current,
    evolution::{add, evolution, expected, observations, observations_when, seed},
    observation::{core_context, entity, fragment, reading},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;

/// The instances every test seeds: `v` = 1 at minute 0 and 2 at minute 1.
const SEEDED: [(i64, u32); 2] = [(1, 0), (2, 1)];

/// Add a `marker` instance to `id` and wait for the drain to land it.
async fn land_marker(deferred: &TestClient, id: &str) {
    add(deferred, id, json!({"marker": reading(1, 59)})).await;
    let marker = observations_when(deferred, id, "marker", |observed| !observed.is_empty()).await;
    assert_eq!(marker, expected(&[(1, 59)]), "the deferred drain landed the marker");
}

#[tokio::test]
async fn deferred_temporal_api_writes_land_and_core_writes_do_not() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let deferred = client_for(scope.client(), Broker::Deferred);
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:deferred");
    seed(&deferred, &id, &entity_type, &SEEDED).await;
    let seeded = observations_when(&deferred, &id, "v", |observed| observed.len() == SEEDED.len()).await;
    assert_eq!(seeded, expected(&SEEDED));

    let response = deferred
        .raw(Method::POST, "entityOperations/upsert?options=update")
        .header("content-type", "application/ld+json")
        .json(&json!([entity(&id, &entity_type, json!({"v": reading(2, 1)}))]))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));
    let response = deferred
        .update_entity_attrs(&id, &fragment(json!({"v": reading(5, 2)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));
    land_marker(&deferred, &id).await;

    let recorded = evolution(&deferred, &id).await.expect("the evolution exists");
    assert_eq!(observations(&recorded, "v"), expected(&SEEDED), "{recorded}");
    assert_eq!(current(&deferred, &id).await.expect("the entity exists")["v"]["value"], 5);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn a_deferred_core_created_entity_gains_no_temporal_evolution() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let deferred = client_for(scope.client(), Broker::Deferred);
    let entity_type = scope.entity_type("Sensor");
    let core_only = scope.id("Sensor:deferred-core-only");
    let witness = scope.id("Sensor:deferred-witness");
    seed(&deferred, &witness, &entity_type, &SEEDED).await;

    let response = deferred
        .post_entity(&entity(&core_only, &entity_type, json!({"v": reading(7, 0)})))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::CREATED);
    land_marker(&deferred, &witness).await;

    assert_eq!(evolution(&deferred, &core_only).await, None);
    let matching = deferred
        .query_entities(&format!("type={entity_type}&q=v==7"), "application/json", Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(
        matching.json_value().expect("a JSON array")[0]["id"],
        json!(core_only),
        "the projection is written inline"
    );

    scope.finish().await.expect("cleanup failed");
}
