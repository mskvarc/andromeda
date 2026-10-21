//! Entity-level Core writes leave the Temporal Evolution as it was.
//!
//! Each test seeds an Entity through the Temporal API, applies one Core
//! operation through the not-recorded broker, and checks that current state
//! took the write while the Temporal Evolution still holds exactly the seeded
//! instances and no deletion.

use crate::{
    current_state::current,
    evolution::{assert_holds_only, evolution, observations, seed},
    observation::{core_context, entity, fragment, reading},
};
use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
use andromeda_test_client::entity::results_count;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// The instances every test seeds: `v` = 1 at minute 0 and 2 at minute 1.
const SEEDED: [(i64, u32); 2] = [(1, 0), (2, 1)];

#[tokio::test]
async fn creating_an_entity_through_the_core_api_gives_it_no_temporal_evolution() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:core-created");

    let response = scope
        .client()
        .post_entity(&entity(&id, &entity_type, json!({"v": reading(7, 0)})))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::CREATED);

    assert_eq!(current(scope.client(), &id).await.expect("the entity exists")["v"]["value"], 7);
    assert_eq!(evolution(scope.client(), &id).await, None, "clause 5.7.3: no evolution is available");
    let listed = scope
        .client()
        .query_temporal_entities(&format!("type={entity_type}&timerel=after&timeAt=2000-01-01T00:00:00Z"), Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(listed.status, StatusCode::OK);
    assert_eq!(listed.json_value().expect("a JSON array"), json!([]));

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn merging_an_entity_changes_current_state_and_records_nothing() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = scope.id("Sensor:merged");
    seed(scope.client(), &id, &scope.entity_type("Sensor"), &SEEDED).await;

    let response = scope
        .client()
        .merge_entity(&id, &fragment(json!({"v": reading(5, 2), "w": reading(3, 2)})), None)
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let stored = current(scope.client(), &id).await.expect("the entity exists");
    assert_eq!((stored["v"]["value"].clone(), stored["w"]["value"].clone()), (json!(5), json!(3)));
    assert_holds_only(scope.client(), &id, &SEEDED).await;
    let recorded = evolution(scope.client(), &id).await.expect("the seeded evolution is kept");
    assert_eq!(observations(&recorded, "w"), Vec::new(), "a Core-only attribute has no history");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn replacing_an_entity_records_neither_its_instances_nor_the_attributes_it_drops() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Sensor");
    let id = scope.id("Sensor:replaced");
    seed(scope.client(), &id, &entity_type, &SEEDED).await;

    let replacement = entity(&id, &entity_type, json!({"u": reading(9, 5)}));
    let response = scope
        .client()
        .replace_entity(&id, &replacement, "application/ld+json", None)
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    let stored = current(scope.client(), &id).await.expect("the entity exists");
    assert_eq!(stored.get("v"), None, "clause 5.6.18: the replacement drops v");
    assert_eq!(stored["u"]["value"], 9);
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn deleting_an_entity_records_no_deletion() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = scope.id("Sensor:deleted");
    seed(scope.client(), &id, &scope.entity_type("Sensor"), &SEEDED).await;

    let response = scope.client().delete_entity(&id).await.expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    assert_eq!(current(scope.client(), &id).await, None);
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn purging_entities_by_query_records_no_deletion() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Purged");
    let id = scope.id("Sensor:purged");
    seed(scope.client(), &id, &entity_type, &SEEDED).await;

    let response = scope
        .client()
        .delete_entities_by_query(&format!("type={entity_type}"), Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    let remaining = scope
        .client()
        .query_entities(&format!("type={entity_type}&count=true"), "application/json", Some(core_context()))
        .await
        .expect("the broker answers");
    assert_eq!(results_count(&remaining.headers), Some(0));
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}

/// The expiry sweep removes an Entity past its `expiresAt` from current state,
/// and, like a Core deletion, leaves no deletion in its history.
#[tokio::test]
async fn an_expired_entity_leaves_its_temporal_evolution_as_it_was() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let id = scope.id("Sensor:expired");
    seed(scope.client(), &id, &scope.entity_type("Sensor"), &SEEDED).await;

    let expires_at = instant_text(chrono::Utc::now() + chrono::Duration::seconds(2));
    let response = scope
        .client()
        .merge_entity(&id, &fragment(json!({"expiresAt": expires_at})), None)
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);

    let deadline = Instant::now() + Duration::from_secs(20);
    while current(scope.client(), &id).await.is_some() {
        assert!(Instant::now() < deadline, "the entity did not expire");
        sleep(Duration::from_millis(250)).await;
    }
    assert_holds_only(scope.client(), &id, &SEEDED).await;

    scope.finish().await.expect("cleanup failed");
}
