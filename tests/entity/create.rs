//! Entity creation tests.
//!
//! Tests entity creation operations covering:
//! - Valid entity creation (minimal, with properties, relationships, GeoProperty)
//! - Invalid payloads (malformed JSON, empty, missing context)
//! - Duplicate entity ID handling
//! - Content-Type and Link header combinations
//! - LanguageProperty, JsonProperty, VocabProperty
//! - Scope property
//! - observationSpace/operationSpace GeoProperty

use crate::{
    TEST_SUITE_CONTEXT,
    building_json_property_array,
    building_json_property_object,
    building_language_property,
    building_location,
    building_minimal,
    building_non_core_geoproperty,
    building_observation_space,
    building_operation_space,
    building_relationship_of_property,
    building_simple_attributes,
    building_vocab_property_array,
    building_vocab_property_string,
    building_with_many_scopes,
    building_with_one_scope,
    clean_database,
    client,
    create_entity,
    create_entity_full,
    create_entity_json,
    create_entity_raw,
    get_entity,
    unique_id,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// 001_01: Create entity with valid payload
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_minimal_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_01_01");
    let entity = building_minimal(&id);

    let (status, location, _) = create_entity_full(&c, &entity, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::CREATED, "create should return 201");
    assert!(location.is_some(), "should return Location header");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert_eq!(body["type"], "Building");
}

#[tokio::test]
#[serial]
async fn entity_create_with_properties_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_01_02");
    let entity = building_simple_attributes(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert_eq!(body["name"]["type"], "Property");
    assert_eq!(body["name"]["value"], "Eiffel Tower");
    assert_eq!(body["subCategory"]["type"], "Property");
    assert_eq!(body["subCategory"]["value"], "tourism");
    assert_eq!(body["airQualityLevel"]["type"], "Property");
    assert_eq!(body["airQualityLevel"]["value"], 4);
    assert_eq!(body["airQualityLevel"]["unitCode"], "C62");
    assert_eq!(body["almostFull"]["type"], "Property");
    assert_eq!(body["almostFull"]["value"], false);
}

#[tokio::test]
#[serial]
async fn entity_create_with_relationship_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_01_03");
    let entity = building_relationship_of_property(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["airQualityLevel"]["type"], "Property");
    assert_eq!(body["airQualityLevel"]["value"], 4);
    assert_eq!(body["airQualityLevel"]["hasWeatherObserved"]["type"], "Relationship");
    assert_eq!(
        body["airQualityLevel"]["hasWeatherObserved"]["object"],
        "urn:ngsi-ld:Sensor:43a1af27-4aa7-44fe-bee9-fe76d56978a4"
    );
}

#[tokio::test]
#[serial]
async fn entity_create_with_location_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_01_04");
    let entity = building_location(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["location"]["type"], "GeoProperty");
    assert_eq!(body["location"]["value"]["type"], "Point");
    assert_eq!(body["name"]["value"], "Eiffel Tower");
    assert_eq!(body["subCategory"]["value"], "tourism");
}

#[tokio::test]
#[serial]
async fn entity_create_non_core_geoproperty_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_01_05");
    let entity = building_non_core_geoproperty(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["customLocation"]["type"], "GeoProperty");
    assert_eq!(body["customLocation"]["value"]["type"], "Point");
}

// ---------------------------------------------------------------------------
// 001_02: Invalid entity creation requests
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_invalid_json_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let (status, body) = create_entity_raw(&c, b"{\"id\":\"urn:ngsi-ld:Building:invalid\",,\"type\":\"Building\"}", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "malformed JSON should return 400");
    assert!(body["type"].as_str().is_some(), "should contain error type");
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn entity_create_empty_json_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let (status, body) = create_entity_raw(&c, b"[]", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty array should return 400");
    assert!(body["type"].as_str().is_some(), "should contain error type");
}

#[tokio::test]
#[serial]
async fn entity_create_invalid_type_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_02_04");
    let entity = json!({
        "id": &id,
        "type": "type",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _, _) = create_entity_full(&c, &entity, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid type should return 400");
}

// ---------------------------------------------------------------------------
// 001_03: Duplicate entity ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_duplicate_id_returns_409() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_03_01");
    let entity = building_minimal(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = create_entity_full(&c, &entity, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::CONFLICT, "duplicate should return 409");
    assert!(
        body["type"].as_str().unwrap_or("").contains("AlreadyExists"),
        "error type should be AlreadyExists"
    );
}

// ---------------------------------------------------------------------------
// 001_04: Link header with JSON content type
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_json_with_link_header_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_04_01");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        },
        "subCategory": {
            "type": "Property",
            "value": "tourism"
        },
        "almostFull": {
            "type": "Property",
            "value": false
        }
    });

    let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert_eq!(body["name"]["value"], "Eiffel Tower");
    assert_eq!(body["subCategory"]["value"], "tourism");
    assert_eq!(body["almostFull"]["value"], false);
}

#[tokio::test]
#[serial]
async fn entity_create_json_with_link_header_no_context_retrieval() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_04_02");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        }
    });

    let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    // Retrieve without context - attributes should appear as expanded URIs
    let (status, body) = get_entity(&c, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
}

// ---------------------------------------------------------------------------
// 001_05: Default context with JSON content type
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_default_context_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_05_01");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        }
    });

    let (status, _, _) = create_entity_full(&c, &entity, "application/json", None).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["id"], id);
    assert_eq!(body["type"], "Building");
}

#[tokio::test]
#[serial]
async fn entity_create_default_context_retrieval() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_05_02");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        }
    });

    let (status, _, _) = create_entity_full(&c, &entity, "application/json", None).await;
    assert_eq!(status, StatusCode::CREATED);

    // Retrieve without context - attributes use default context expansion
    let (status, body) = get_entity(&c, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK);
    // Attribute appears with expanded IRI since no context is used for compaction
    assert!(
        body.get("name").is_some() || body.get("https://uri.etsi.org/ngsi-ld/default-context/name").is_some(),
        "name attribute should be present"
    );
}

// ---------------------------------------------------------------------------
// 001_06: @context in body with application/json
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_json_with_context_in_body_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_06_01");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _, _) = create_entity_full(&c, &entity, "application/json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "@context with application/json should return 400");
}

// ---------------------------------------------------------------------------
// 001_07: @context from payload with application/ld+json
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_ld_json_with_context_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_07_01");
    let entity = building_simple_attributes(&id);

    let (status, location, _) = create_entity_full(&c, &entity, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some());

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["value"], "Eiffel Tower");
    assert_eq!(body["subCategory"]["value"], "tourism");
    assert_eq!(body["almostFull"]["value"], false);
}

#[tokio::test]
#[serial]
async fn entity_create_ld_json_with_context_retrieval() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_07_02");
    let entity = building_simple_attributes(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["value"], "Eiffel Tower");
    assert_eq!(body["airQualityLevel"]["value"], 4);
    assert_eq!(body["airQualityLevel"]["unitCode"], "C62");
}

// ---------------------------------------------------------------------------
// 001_08: Missing @context with application/ld+json
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_ld_json_without_context_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_08_01");
    let entity = json!({
        "id": &id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Eiffel Tower"
        }
    });

    let (status, _, _) = create_entity_full(&c, &entity, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "ld+json without @context should return 400");
}

// ---------------------------------------------------------------------------
// 001_09: Link header with application/ld+json
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_ld_json_with_link_header_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_09_01");
    let entity = building_simple_attributes(&id);

    let (status, _, _) = create_entity_full(&c, &entity, "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "Link header with ld+json should return 400");
}

// ---------------------------------------------------------------------------
// 001_10: LanguageProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_language_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_10_01");
    let entity = building_language_property(&id);

    let (status, location, _) = create_entity_full(&c, &entity, "application/ld+json", None).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(location.is_some());

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["type"], "LanguageProperty");
    assert_eq!(body["name"]["languageMap"]["en"], "Eiffel Tower");
    assert_eq!(body["name"]["languageMap"]["fr"], "Tour Eiffel");
    assert_eq!(body["name"]["languageMap"]["de"], "Eiffelturm");
}

// ---------------------------------------------------------------------------
// 001_11: Scope property
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_one_scope_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_11_01");
    let entity = building_with_one_scope(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scope"], "/Madrid/Gardens/ParqueNorte");
}

#[tokio::test]
#[serial]
async fn entity_create_many_scopes_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_11_02");
    let entity = building_with_many_scopes(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/json", None).await;
    assert_eq!(status, StatusCode::OK);
    let scopes = body["scope"].as_array().expect("scope should be an array");
    assert_eq!(scopes.len(), 2);
}

// ---------------------------------------------------------------------------
// 001_12: observationSpace GeoProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_observation_space_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_12");
    let entity = building_observation_space(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["observationSpace"]["type"], "GeoProperty");
}

// ---------------------------------------------------------------------------
// 001_13: operationSpace GeoProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_operation_space_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_13");
    let entity = building_operation_space(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["operationSpace"]["type"], "GeoProperty");
    assert_eq!(body["operationSpace"]["value"]["type"], "Polygon");
}

// ---------------------------------------------------------------------------
// 001_14: JsonProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_json_property_object_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_14_01");
    let entity = building_json_property_object(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["jsonProperty"]["type"], "JsonProperty");
    assert!(body["jsonProperty"]["json"].is_object());
}

#[tokio::test]
#[serial]
async fn entity_create_json_property_array_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_14_02");
    let entity = building_json_property_array(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["jsonProperty"]["type"], "JsonProperty");
    assert!(body["jsonProperty"]["json"].is_array());
    assert_eq!(body["jsonProperty"]["json"].as_array().unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// 001_15: VocabProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_create_vocab_property_string_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_15_01");
    let entity = building_vocab_property_string(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["vocabProperty"]["type"], "VocabProperty");
}

#[tokio::test]
#[serial]
async fn entity_create_vocab_property_array_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:create_001_15_02");
    let entity = building_vocab_property_array(&id);

    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["vocabProperty"]["type"], "VocabProperty");
}
