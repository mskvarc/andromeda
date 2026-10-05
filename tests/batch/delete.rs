#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use crate::helpers::*;
use andromeda_test_fixtures::problem_detail::BAD_REQUEST_DATA;
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::{Client, Method, StatusCode};
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

/// Create one simple Building per id.
async fn create_buildings(c: &Client, ids: &[&str]) {
    for id in ids {
        let (status, _, _) = create_entity(c, &building_simple_attributes(id)).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }
}

/// Clause 6.3.5: an `application/ld+json` body names its context in an
/// `@context` member, which an array of Entity IDs cannot carry, so the
/// request is `BadRequestData` and nothing is deleted.
#[tokio::test]
async fn batch_delete_ld_json_id_array_returns_400_and_deletes_nothing() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:006_05_01");
    create_buildings(&c, &[&id]).await;

    let body = serde_json::to_vec(&json!([&id])).expect("an id array");
    let (status, problem) = request_raw(&c, Method::POST, "/entityOperations/delete", &body, Some("application/ld+json"), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(problem["type"], BAD_REQUEST_DATA.as_str());

    let (status, _, _) = get_entity(&c, &id, "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.5: an `application/json` id array is read against the one
/// context `Link` the request carries.
#[tokio::test]
async fn batch_delete_json_id_array_with_context_link_returns_204() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();
    let id1 = scope.id("Building:006_06_01a");
    let id2 = scope.id("Building:006_06_01b");
    create_buildings(&c, &[&id1, &id2]).await;

    let body = serde_json::to_vec(&json!([&id1, &id2])).expect("an id array");
    let (status, _) = request_raw(&c, Method::POST, "/entityOperations/delete", &body, Some("application/json"), true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    for id in [&id1, &id2] {
        let (status, _, _) = get_entity(&c, id, "", false).await;
        pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    }
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.5: an `application/ld+json` body beside a context `Link` is
/// `BadRequestData`.
#[tokio::test]
async fn batch_delete_ld_json_beside_a_context_link_returns_400() {
    let scope = TestEnvironment::from_suite("batch").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:006_07_01");
    create_buildings(&c, &[&id]).await;

    let body = serde_json::to_vec(&json!([&id])).expect("an id array");
    let (status, problem) = request_raw(&c, Method::POST, "/entityOperations/delete", &body, Some("application/ld+json"), true).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(problem["type"], BAD_REQUEST_DATA.as_str());

    let (status, _, _) = get_entity(&c, &id, "", false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    scope.finish().await.expect("cleanup failed");
}
