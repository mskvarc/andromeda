#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use reqwest::{Method, StatusCode};

use crate::helpers::*;
use andromeda_test_harness::environment::TestEnvironment;
use serde_json::json;

#[tokio::test]
async fn batch_delete_entities_returns_204() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let id1 = scope.id("Building:006_01_01a");
    let id2 = scope.id("Building:006_01_01b");

    for id in [&id1, &id2] {
        let entity = building_simple_attributes(id);
        let (status, _, _) = create_entity(&c, &entity).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let ids = json!([&id1, &id2]);
    let (status, _) = batch_delete(&c, &ids).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "batch delete should return 204");

    let (status, _, _) = get_entity(&c, &id1, "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, _) = get_entity(&c, &id2, "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_delete_mixed_existing_nonexisting_returns_207() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let existing_id = scope.id("Building:006_02_01a");
    let nonexistent_id = scope.id("Building:006_02_01b");

    let entity = building_simple_attributes(&existing_id);
    let (status, _, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let ids = json!([&existing_id, &nonexistent_id]);
    let (status, body) = batch_delete(&c, &ids).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "mixed should return 207");

    let success = body["success"].as_array().expect("should have success array");
    let errors = body["errors"].as_array().expect("should have errors array");
    pretty_assertions::assert_eq!(success.len(), 1);
    pretty_assertions::assert_eq!(errors.len(), 1);

    let (status, _, _) = get_entity(&c, &existing_id, "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn batch_delete_invalid_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(
        &c,
        Method::POST,
        "/entityOperations/delete",
        b"[\"urn:ngsi-ld:Building:x\",,]",
        Some("application/json"),
        false,
    )
    .await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_delete_empty_json_returns_400() {
    let c = client();
    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/delete", b"[]", Some("application/json"), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn batch_delete_duplicate_ids_returns_207() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();

    let id = scope.id("Building:006_04_01");
    let entity = building_simple_attributes(&id);
    let (status, _, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let ids = json!([&id, &id]);
    let (status, body) = batch_delete(&c, &ids).await;
    pretty_assertions::assert_eq!(status, StatusCode::MULTI_STATUS, "duplicate IDs should return 207");

    let success = body["success"].as_array().expect("should have success array");
    let errors = body["errors"].as_array().expect("should have errors array");
    pretty_assertions::assert_eq!(success.len(), 1, "first delete should succeed");
    pretty_assertions::assert_eq!(errors.len(), 1, "second delete should fail");
    scope.finish().await.expect("cleanup failed");
}
