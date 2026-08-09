//! Temporal entity attribute add tests.
//!
//! Tests POST /temporal/entities/{entityId}/attrs operations:
//! - Add attribute to existing temporal entity
//! - Error cases: empty/invalid entity ID, entity not found, invalid/empty content

use crate::{
    COMPOUND_CONTEXT,
    add_attribute_fragment,
    add_temporal_attrs,
    add_temporal_attrs_raw,
    clean_database,
    client,
    create_temporal_entity,
    get_temporal_entity,
    request_temporal_attrs,
    unique_id,
    vehicle_temporal_representation,
};
use reqwest::{Method, StatusCode};
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Add attribute to temporal entity
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_add_attribute_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_01_01");

    // Setup: create temporal entity
    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Add attribute
    let fragment = add_attribute_fragment();
    let (status, body) = add_temporal_attrs(&c, &id, &fragment).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "add attrs should return 204");
    assert!(body.is_empty(), "response body should be empty");

    // Verify
    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    assert!(body["speed"].is_array(), "speed should exist");
    assert!(body["fuelLevel"].is_array(), "fuelLevel should exist");
    assert!(body["added_attribute"].is_array(), "added_attribute should exist");

    let added = body["added_attribute"].as_array().expect("added_attribute should be array");
    assert_eq!(added.len(), 3, "should have 3 added_attribute instances");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_add_multiple_attributes_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_01_02");

    // Setup: create temporal entity
    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Add multiple attributes at once
    let fragment = json!({
        "newAttr1": [
            { "type": "Property", "value": 10, "observedAt": "2020-09-01T12:00:00Z" }
        ],
        "newAttr2": [
            { "type": "Property", "value": 20, "observedAt": "2020-09-01T12:00:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    });
    let (status, body) = add_temporal_attrs(&c, &id, &fragment).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "add multiple attrs should return 204");
    assert!(body.is_empty(), "response body should be empty");

    // Verify both attributes added
    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    assert!(body["newAttr1"].is_array(), "newAttr1 should exist");
    assert!(body["newAttr2"].is_array(), "newAttr2 should exist");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_add_relationship_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_01_03");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Add Relationship attribute
    let fragment = json!({
        "isParkedIn": [
            {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Parking:P1",
                "observedAt": "2020-09-01T14:00:00Z"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    });
    let (status, _) = add_temporal_attrs(&c, &id, &fragment).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["isParkedIn"].is_array());
    assert_eq!(body["isParkedIn"][0]["type"], "Relationship");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_add_geoproperty_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_01_04");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Add GeoProperty attribute
    let fragment = json!({
        "location": [
            {
                "type": "GeoProperty",
                "value": { "type": "Point", "coordinates": [-8.5, 41.2] },
                "observedAt": "2020-09-01T14:00:00Z"
            }
        ],
        "@context": [COMPOUND_CONTEXT]
    });
    let (status, _) = add_temporal_attrs(&c, &id, &fragment).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["location"].is_array());
    assert_eq!(body["location"][0]["type"], "GeoProperty");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_add_instances_to_existing_attribute_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_01_05");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Add more instances to existing speed attribute
    let fragment = json!({
        "speed": [
            { "type": "Property", "value": 90, "observedAt": "2020-09-01T12:10:00Z" },
            { "type": "Property", "value": 95, "observedAt": "2020-09-01T12:15:00Z" }
        ],
        "@context": [COMPOUND_CONTEXT]
    });
    let (status, _) = add_temporal_attrs(&c, &id, &fragment).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let speed = body["speed"].as_array().expect("speed should be array");
    assert_eq!(speed.len(), 4, "should have 4 speed instances (2 original + 2 added)");
}

// ---------------------------------------------------------------------------
// Invalid entity ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_add_empty_entity_id_returns_400() {
    clean_database().await;
    let c = client();

    let fragment = add_attribute_fragment();
    let (status, body) = request_temporal_attrs(&c, Method::POST, "/temporal/entities//attrs", Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty entity ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_add_invalid_entity_id_returns_400() {
    clean_database().await;
    let c = client();

    let fragment = add_attribute_fragment();
    let (status, body) = add_temporal_attrs_raw(&c, "invalidId", serde_json::to_vec(&fragment).unwrap().as_slice(), "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

// ---------------------------------------------------------------------------
// Entity not found
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_add_entity_not_found_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_03_01");

    let fragment = add_attribute_fragment();
    let (status, body) = add_temporal_attrs_raw(&c, &id, serde_json::to_vec(&fragment).unwrap().as_slice(), "application/ld+json").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

// ---------------------------------------------------------------------------
// Invalid/empty content
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_add_invalid_json_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_04_01");

    // Setup: create entity
    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = add_temporal_attrs_raw(&c, &id, b"{invalid json", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid JSON should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_add_empty_json_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_04_02");

    // Setup: create entity
    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = add_temporal_attrs_raw(&c, &id, b"", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty JSON should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("InvalidRequest")),
        "error type should be InvalidRequest"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_add_empty_object_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:add_014_04_03");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = add_temporal_attrs_raw(&c, &id, b"{}", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty object should return 400");
    assert!(
        body["type"]
            .as_str()
            .is_some_and(|t| t.contains("InvalidRequest") || t.contains("BadRequestData")),
        "error type should indicate bad request"
    );
}
