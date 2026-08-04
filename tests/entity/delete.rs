//! Entity deletion tests.
//!
//! Tests single entity deletion operations:
//! - Delete existing entity
//! - Missing ID (DELETE on collection)
//! - Invalid ID
//! - Non-existent entity

use crate::{building_simple_attributes, clean_database, client, create_entity, delete_entity, get_entity, request_method_on_collection, unique_id};
use reqwest::{Method, StatusCode};
use serial_test::serial;

// ---------------------------------------------------------------------------
// 002_01: Delete existing entity
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_existing_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:delete_002_01_01");
    let entity = building_simple_attributes(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity(&c, &id).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete should return 204");

    // Verify entity is gone
    let (status, _) = get_entity(&c, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "entity should no longer exist");
}

// ---------------------------------------------------------------------------
// 002_02: Invalid delete requests
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_missing_id_returns_400() {
    clean_database().await;
    let c = client();

    let (status, _) = request_method_on_collection(&c, Method::DELETE, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "DELETE on /entities without filters should return 400");
}

#[tokio::test]
#[serial]
async fn entity_delete_invalid_id_returns_400() {
    clean_database().await;
    let c = client();

    let (status, body) = delete_entity(&c, "thisisaninvaliduri").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid URI should return 400");
    assert!(body["type"].as_str().is_some(), "should contain error type");
}

// ---------------------------------------------------------------------------
// 002_03: Non-existent entity
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_delete_not_found_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:delete_002_03_01");

    let (status, body) = delete_entity(&c, &id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    assert!(
        body["type"].as_str().unwrap_or("").contains("ResourceNotFound"),
        "error type should be ResourceNotFound"
    );
}
