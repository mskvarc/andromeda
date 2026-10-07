//! Temporal entity update tests.
//!
//! Tests POST /temporal/entities (update/merge) operations:
//! - Update temporal entity with additional instances
//! - Add new attributes to existing temporal entity
//! - Merge multiple attribute instances
//! - Update with different attribute types

use crate::{TEST_SUITE_CONTEXT, client, create_temporal_entity, get_temporal_entity};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn temporal_update_add_instances_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_01_01");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let speed = body["speed"].as_array().expect("speed should be array");
    assert_eq!(speed.len(), 3, "should have 3 speed instances after update");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_update_add_new_attribute_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_01_02");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    assert!(body["speed"].is_array(), "speed should exist");
    assert!(body["fuelLevel"].is_array(), "fuelLevel should exist");
    let fuel = body["fuelLevel"].as_array().expect("fuelLevel should be array");
    assert_eq!(fuel.len(), 2, "should have 2 fuelLevel instances");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_update_multiple_attributes_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_01_03");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let speed = body["speed"].as_array().expect("speed should be array");
    let fuel = body["fuelLevel"].as_array().expect("fuelLevel should be array");
    assert_eq!(speed.len(), 2, "should have 2 speed instances");
    assert_eq!(fuel.len(), 2, "should have 2 fuelLevel instances");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_update_with_relationship_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_02_01");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["isParkedIn"].is_array());
    assert_eq!(body["isParkedIn"][0]["type"], "Relationship");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_update_with_geoproperty_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_02_02");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["location"].is_array());
    assert_eq!(body["location"][0]["type"], "GeoProperty");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_update_with_language_property_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_02_03");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let name = body["name"].as_array().expect("name should be array");
    assert_eq!(name.len(), 2, "should have 2 name instances");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_update_with_datasetid_success() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_03_01");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let speed = body["speed"].as_array().expect("speed should be array");
    assert!(speed.len() >= 2, "should have at least 2 speed instances (default + datasetId)");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_update_nonexistent_entity_creates() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_04_01");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_update_same_instance_idempotent() {
    let scope = TestEnvironment::from_suite("temporal_entity").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:update_008_05_01");

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

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["speed"].is_array());
    scope.finish().await.expect("cleanup failed");
}
