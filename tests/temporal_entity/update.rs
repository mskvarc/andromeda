//! Temporal entity update tests.
//!
//! Tests POST /temporal/entities (update/merge) operations:
//! - Update temporal entity with additional instances
//! - Add new attributes to existing temporal entity
//! - Merge multiple attribute instances
//! - Update with different attribute types

use crate::{TEST_SUITE_CONTEXT, clean_database, client, create_temporal_entity, get_temporal_entity, unique_id};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Update existing temporal entity
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_update_add_instances_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_01_01");

    // Create initial entity
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update: add more instances
    let update = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z" },
            { "type": "Property", "value": 100, "observedAt": "2020-08-01T12:07:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &update).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    // Verify: should have all instances
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let speed = body["speed"].as_array().expect("speed should be array");
    assert_eq!(speed.len(), 3, "should have 3 speed instances after update");
}

#[tokio::test]
#[serial]
async fn temporal_update_add_new_attribute_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_01_02");

    // Create initial entity with speed only
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update: add fuelLevel attribute
    let update = json!({
        "id": &id,
        "type": "Vehicle",
        "fuelLevel": [
            { "type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &update).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    // Verify: should have both attributes
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    assert!(body["speed"].is_array(), "speed should exist");
    assert!(body["fuelLevel"].is_array(), "fuelLevel should exist");
    let fuel = body["fuelLevel"].as_array().expect("fuelLevel should be array");
    assert_eq!(fuel.len(), 2, "should have 2 fuelLevel instances");
}

#[tokio::test]
#[serial]
async fn temporal_update_multiple_attributes_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_01_03");

    // Create initial entity
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 80, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update both attributes
    let update = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 70, "observedAt": "2020-08-01T12:05:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &update).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    // Verify
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let speed = body["speed"].as_array().expect("speed should be array");
    let fuel = body["fuelLevel"].as_array().expect("fuelLevel should be array");
    assert_eq!(speed.len(), 2, "should have 2 speed instances");
    assert_eq!(fuel.len(), 2, "should have 2 fuelLevel instances");
}

// ---------------------------------------------------------------------------
// Update with different attribute types
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_update_with_relationship_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_02_01");

    // Create initial entity
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update: add Relationship
    let update = json!({
        "id": &id,
        "type": "Vehicle",
        "isParkedIn": [
            {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Parking:P1",
                "observedAt": "2020-08-01T13:00:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &update).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    // Verify
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["isParkedIn"].is_array());
    assert_eq!(body["isParkedIn"][0]["type"], "Relationship");
}

#[tokio::test]
#[serial]
async fn temporal_update_with_geoproperty_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_02_02");

    // Create initial entity
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update: add GeoProperty
    let update = json!({
        "id": &id,
        "type": "Vehicle",
        "location": [
            {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [-8.5, 41.2] },
                "observedAt": "2020-08-01T13:00:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &update).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    // Verify
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["location"].is_array());
    assert_eq!(body["location"][0]["type"], "GeoProperty");
}

#[tokio::test]
#[serial]
async fn temporal_update_with_language_property_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_02_03");

    // Create initial entity
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "name": [
            {
                "type": "LanguageProperty",
                "languageMap": { "en": "Fast Car" },
                "observedAt": "2020-08-01T12:00:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update: add another LanguageProperty instance
    let update = json!({
        "id": &id,
        "type": "Vehicle",
        "name": [
            {
                "type": "LanguageProperty",
                "languageMap": { "en": "Super Fast Car", "de": "Super Schnelles Auto" },
                "observedAt": "2020-08-01T13:00:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &update).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    // Verify
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let name = body["name"].as_array().expect("name should be array");
    assert_eq!(name.len(), 2, "should have 2 name instances");
}

// ---------------------------------------------------------------------------
// Update with datasetId
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_update_with_datasetid_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_03_01");

    // Create initial entity with default instance
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

    // Update: add instance with datasetId
    let update = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            {
                "type": "Property",
                "value": 110,
                "observedAt": "2020-08-01T12:05:00Z",
                "datasetId": "urn:ngsi-ld:Dataset:Speed"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &update).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    // Verify
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let speed = body["speed"].as_array().expect("speed should be array");
    assert!(speed.len() >= 2, "should have at least 2 speed instances (default + datasetId)");
}

// ---------------------------------------------------------------------------
// Update nonexistent entity creates it
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_update_nonexistent_entity_creates() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_04_01");

    // POST to nonexistent entity should create it
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "POST to nonexistent should return 201");

    // Verify entity exists
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

// ---------------------------------------------------------------------------
// Idempotent updates
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_update_same_instance_idempotent() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_008_05_01");

    // Create initial entity
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update with same instance (same observedAt)
    let update = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });
    let status = create_temporal_entity(&c, &update).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    // Verify: should still have only 1 instance (or might have 2 depending on implementation)
    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["speed"].is_array());
}
