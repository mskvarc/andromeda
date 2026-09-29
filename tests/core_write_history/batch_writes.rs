//! Batch operations (clause 5.6.7 to 5.6.10 and 5.6.20) leave the Temporal
//! Evolution as it was.

use crate::{
    current_state::current,
    evolution::{assert_holds_only, evolution, seed},
    observation::{entity, reading},
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

/// The instances every test seeds: `v` = 1 at minute 0 and 2 at minute 1.
const SEEDED: [(i64, u32); 2] = [(1, 0), (2, 1)];

/// Two Entities: one seeded through the Temporal API, one that only a batch
/// writes. Returns their identifiers and shared type.
async fn pair(scope: &TestEnvironment, label: &str) -> (String, String, String) {
    let entity_type = scope.entity_type("Sensor");
    let seeded = scope.id(&format!("Sensor:{label}-seeded"));
    let fresh = scope.id(&format!("Sensor:{label}-fresh"));
    seed(scope.client(), &seeded, &entity_type, &SEEDED).await;
    (seeded, fresh, entity_type)
}

/// Neither Entity has any history the batch wrote.
async fn assert_batch_recorded_nothing(scope: &TestEnvironment, seeded: &str, fresh: &str) {
    assert_holds_only(scope.client(), seeded, &SEEDED).await;
    assert_eq!(evolution(scope.client(), fresh).await, None);
}

/// The value of `v` current state holds for `id`.
async fn current_value(scope: &TestEnvironment, id: &str) -> Value {
    current(scope.client(), id).await.expect("the entity exists")["v"]["value"].clone() // clone: the caller owns the reported value.
}

#[tokio::test]
async fn batch_create_gives_its_entities_no_temporal_evolution() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let entity_type = scope.entity_type("Sensor");
    let first = scope.id("Sensor:batch-created-1");
    let second = scope.id("Sensor:batch-created-2");

    let response = scope
        .client()
        .batch_create(&json!([
            entity(&first, &entity_type, json!({"v": reading(1, 0)})),
            entity(&second, &entity_type, json!({"v": reading(2, 0)}))
        ]))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::CREATED, "{}", String::from_utf8_lossy(&response.body));

    assert_eq!(current_value(&scope, &second).await, json!(2));
    assert_eq!(evolution(scope.client(), &first).await, None);
    assert_eq!(evolution(scope.client(), &second).await, None);

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_upsert_records_nothing_in_either_mode() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let (seeded, fresh, entity_type) = pair(&scope, "upserted").await;

    for (path, value) in [("entityOperations/upsert", 5), ("entityOperations/upsert?options=update", 6)] {
        let response = scope
            .client()
            .raw(Method::POST, path)
            .header("content-type", "application/ld+json")
            .json(&json!([
                entity(&seeded, &entity_type, json!({"v": reading(value, 2)})),
                entity(&fresh, &entity_type, json!({"v": reading(value, 2)}))
            ]))
            .send()
            .await
            .expect("the broker answers");
        assert!(
            response.status.is_success(),
            "{path}: {} {}",
            response.status,
            String::from_utf8_lossy(&response.body)
        );
        assert_eq!(current_value(&scope, &seeded).await, json!(value), "{path}");
    }
    assert_batch_recorded_nothing(&scope, &seeded, &fresh).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_update_records_nothing() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let (seeded, fresh, entity_type) = pair(&scope, "updated").await;

    let response = scope
        .client()
        .batch_update(&json!([entity(&seeded, &entity_type, json!({"v": reading(5, 2)}))]))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    assert_eq!(current_value(&scope, &seeded).await, json!(5));
    assert_batch_recorded_nothing(&scope, &seeded, &fresh).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_merge_records_nothing() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let (seeded, fresh, entity_type) = pair(&scope, "merged").await;

    let response = scope
        .client()
        .batch_merge(&json!([entity(&seeded, &entity_type, json!({"v": reading(5, 2)}))]))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    assert_eq!(current_value(&scope, &seeded).await, json!(5));
    assert_batch_recorded_nothing(&scope, &seeded, &fresh).await;

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_delete_records_no_deletion() {
    let scope = TestEnvironment::from_suite("core_write_history").await.expect("isolated test environment");
    let (seeded, fresh, _) = pair(&scope, "deleted").await;

    let response = scope.client().batch_delete(&json!([seeded])).await.expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT, "{}", String::from_utf8_lossy(&response.body));

    assert_eq!(current(scope.client(), &seeded).await, None);
    assert_batch_recorded_nothing(&scope, &seeded, &fresh).await;

    scope.finish().await.expect("cleanup failed");
}
