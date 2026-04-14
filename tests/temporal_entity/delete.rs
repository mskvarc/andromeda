//! Temporal entity delete tests.
//!
//! Tests DELETE /temporal/entities/{entityId} operations:
//! - Delete existing temporal entity
//! - Error cases: empty ID, invalid ID, unknown ID

use crate::{BASE_URL, TEST_SUITE_CONTEXT, clean_database, client, create_temporal_entity, delete_temporal_entity, get_temporal_entity, unique_id};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Valid delete operations
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_delete_entity_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_009_01_01");

    // Setup: create the temporal entity
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete
    let (status, _) = delete_temporal_entity(&c, &id).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete should return 204");

    // Verify: entity no longer exists
    let (status, _, _) = get_temporal_entity(&c, &id, "", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "deleted entity should return 404");
}

// ---------------------------------------------------------------------------
// Empty entity ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_delete_empty_id_returns_405() {
    clean_database().await;
    let c = client();

    let resp = c.delete(format!("{BASE_URL}/temporal/entities/")).send().await.expect("request failed");
    assert_eq!(resp.status(), StatusCode::METHOD_NOT_ALLOWED, "empty ID should return 405");
}

// ---------------------------------------------------------------------------
// Invalid entity ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_delete_invalid_id_returns_400() {
    clean_database().await;
    let c = client();

    let (status, body) = delete_temporal_entity(&c, "invalidId").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

// ---------------------------------------------------------------------------
// Unknown entity ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_delete_unknown_id_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_009_03_01");

    let (status, body) = delete_temporal_entity(&c, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "unknown ID should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

// ---------------------------------------------------------------------------
// Delete and recreate
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_delete_and_recreate_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_009_04_01");

    // Create
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 100, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete
    let (status, _) = delete_temporal_entity(&c, &id).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Recreate with different data
    let entity2 = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 200, "observedAt": "2020-09-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity2).await;
    assert_eq!(status, StatusCode::CREATED, "recreate after delete should return 201");

    // Verify new data
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let speed = body["speed"].as_array().expect("speed should be array");
    assert_eq!(speed[0]["value"], 200);
}
