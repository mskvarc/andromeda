//! Temporal entity create tests.
//!
//! Tests POST /temporal/entities operations:
//! - Create temporal entity with valid payload
//! - Create without @context (Content-Type: application/json)
//! - Error cases: invalid JSON, empty JSON, missing ID, invalid URI

use crate::{
    TEST_SUITE_CONTEXT,
    clean_database,
    client,
    create_temporal_entity,
    create_temporal_entity_json,
    create_temporal_entity_raw,
    get_temporal_entity,
    strip_instance_ids,
    unique_id,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Valid create operations
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_create_entity_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_01_01");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 80, "observedAt": "2020-08-01T12:05:00Z" }
        ],
        "fuelLevel": [
            { "type": "Property", "value": 67, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 53, "observedAt": "2020-08-01T13:05:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "create should return 201");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let mut body = body;
    strip_instance_ids(&mut body);
    assert_eq!(body["id"], id);
    assert_eq!(body["type"], "Vehicle");
    assert!(body["speed"].is_array());
    assert!(body["fuelLevel"].is_array());
}

#[tokio::test]
#[serial]
async fn temporal_create_entity_with_plural_values_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_plural_values");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": {
            "type": "Property",
            "datasetId": "urn:ngsi-ld:Dataset:sensor",
            "values": [
                {"value": 55, "observedAt": "2020-08-01T12:00:00Z"},
                {"value": 61, "observedAt": "2020-08-01T13:00:00Z"}
            ]
        },
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["speed"].as_array().map(Vec::len), Some(2));
    assert!(
        body["speed"]
            .as_array()
            .is_some_and(|observations| { observations.iter().all(|observation| observation["datasetId"] == "urn:ngsi-ld:Dataset:sensor") })
    );
}

#[tokio::test]
#[serial]
async fn temporal_create_entity_without_context_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_01_02");

    // Entity without @context, sent as application/json with Link header
    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ]
    });

    let status = create_temporal_entity_json(&c, &entity, "application/json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::CREATED, "create without @context should return 201");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert!(body["speed"].is_array());
}

// ---------------------------------------------------------------------------
// Invalid JSON
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_create_invalid_json_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let (status, body) = create_temporal_entity_raw(&c, b"{invalid json", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid JSON should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_create_empty_json_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let (status, body) = create_temporal_entity_raw(&c, b"", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty JSON should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("InvalidRequest")),
        "error type should be InvalidRequest"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

// ---------------------------------------------------------------------------
// Missing/invalid entity ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_create_missing_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let entity = json!({
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing ID should return 400");
}

#[tokio::test]
#[serial]
async fn temporal_create_invalid_uri_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let entity = json!({
        "id": "invalidId",
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid URI should return 400");
}

// ---------------------------------------------------------------------------
// Duplicate entity
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_create_duplicate_returns_409() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_04_01");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "first create should return 201");

    // Second create of same entity should fail (unless it's an upsert)
    let status = create_temporal_entity(&c, &entity).await;
    // Note: NGSI-LD spec allows POST to update/merge if entity exists
    // 409 Conflict or 204 No Content depending on implementation
    assert!(
        status == StatusCode::CONFLICT || status == StatusCode::NO_CONTENT,
        "duplicate create should return 409 or 204, got {status}"
    );
}

// ---------------------------------------------------------------------------
// Missing type
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_create_missing_type_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_05_01");

    let entity = json!({
        "id": &id,
        "speed": [
            { "type": "Property", "value": 120, "observedAt": "2020-08-01T12:03:00Z" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "missing type should return 400");
}

// ---------------------------------------------------------------------------
// Various attribute types
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_create_with_relationship_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_06_01");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "isParkedIn": [
            {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Parking:P1",
                "observedAt": "2020-08-01T12:03:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "create with Relationship should return 201");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["isParkedIn"].is_array());
    assert_eq!(body["isParkedIn"][0]["type"], "Relationship");
}

#[tokio::test]
#[serial]
async fn temporal_create_with_geoproperty_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_06_02");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "location": [
            {
                "type": "GeoProperty",
                "value": {
                    "type": "Point",
                    "coordinates": [-8.5, 41.2]
                },
                "observedAt": "2020-08-01T12:03:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "create with GeoProperty should return 201");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["location"].is_array());
    assert_eq!(body["location"][0]["type"], "GeoProperty");
}

#[tokio::test]
#[serial]
async fn temporal_create_with_language_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_06_03");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "name": [
            {
                "type": "LanguageProperty",
                "languageMap": { "en": "Fast Car", "de": "Schnelles Auto" },
                "observedAt": "2020-08-01T12:03:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "create with LanguageProperty should return 201");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["name"].is_array());
    assert_eq!(body["name"][0]["type"], "LanguageProperty");
}

#[tokio::test]
#[serial]
async fn temporal_create_with_json_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_06_04");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "config": [
            {
                "type": "JsonProperty",
                "json": { "maxSpeed": 200, "eco": true },
                "observedAt": "2020-08-01T12:03:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "create with JsonProperty should return 201");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["config"].is_array());
    assert_eq!(body["config"][0]["type"], "JsonProperty");
}

#[tokio::test]
#[serial]
async fn temporal_create_with_vocab_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_06_05");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "category": [
            {
                "type": "VocabProperty",
                "vocab": "sedan",
                "observedAt": "2020-08-01T12:03:00Z"
            }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "create with VocabProperty should return 201");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["category"].is_array());
    assert_eq!(body["category"][0]["type"], "VocabProperty");
}

// ---------------------------------------------------------------------------
// Multiple instances with datasetId
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_create_with_multiple_dataset_instances_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:create_007_07_01");

    let entity = json!({
        "id": &id,
        "type": "Vehicle",
        "speed": [
            { "type": "Property", "value": 100, "observedAt": "2020-08-01T12:03:00Z" },
            { "type": "Property", "value": 110, "observedAt": "2020-08-01T12:04:00Z", "datasetId": "urn:ngsi-ld:Dataset:Speed" }
        ],
        "@context": [TEST_SUITE_CONTEXT]
    });

    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED, "create with multiple datasetId instances should return 201");

    let (status, body, _) = get_temporal_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["speed"].is_array());
}
