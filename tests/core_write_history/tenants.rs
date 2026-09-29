//! The setting holds for every tenant the broker serves.

use crate::{
    current_state::current,
    evolution::{assert_holds_only, evolution, seed},
    observation::{entity, fragment, reading},
};
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

/// The instances the test seeds: `v` = 1 at minute 0 and 2 at minute 1.
const SEEDED: [(i64, u32); 2] = [(1, 0), (2, 1)];

#[tokio::test]
async fn a_named_tenant_leaves_core_writes_out_of_history_too() {
    let scope = TenantScopedEnvironment::from_suite("core_write_history")
        .await
        .expect("private tenant environment");
    let entity_type = scope.scope().entity_type("Sensor");
    let seeded = scope.id("Sensor:tenant-seeded");
    let core_only = scope.id("Sensor:tenant-core-only");
    seed(scope.client(), &seeded, &entity_type, &SEEDED).await;

    let response = scope
        .client()
        .update_entity_attrs(&seeded, &fragment(json!({"v": reading(5, 2)})))
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::NO_CONTENT);
    let response = scope
        .client()
        .post_entity(&entity(&core_only, &entity_type, json!({"v": reading(7, 0)})))
        .send()
        .await
        .expect("the broker answers");
    assert_eq!(response.status, StatusCode::CREATED);

    assert_eq!(current(scope.client(), &seeded).await.expect("the entity exists")["v"]["value"], 5);
    assert_holds_only(scope.client(), &seeded, &SEEDED).await;
    assert_eq!(evolution(scope.client(), &core_only).await, None);

    scope.finish().await.expect("cleanup failed");
}
