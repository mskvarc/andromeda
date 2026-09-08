//! Entity attribute append tests.
//!
//! Tests POST /entities/{entityId}/attrs operations:
//! - Append new attribute to existing entity
//! - Append attribute with different datasetId
//! - Append with noOverwrite option
//! - Error cases (invalid ID, non-existent entity)
//! - Special attribute types (`LanguageProperty`, `JsonProperty`, `VocabProperty`)
//! - Scope handling
//! - observationSpace/operationSpace `GeoProperty`

use crate::{
    TEST_SUITE_CONTEXT,
    append_entity_attrs,
    append_entity_attrs_json,
    append_entity_attrs_raw,
    append_entity_attrs_with_options,
    building_minimal,
    clean_database,
    client,
    create_entity,
    create_entity_json,
    get_entity,
    request_attrs_method,
    unique_id,
    vehicle_minimal,
    vehicle_speed_two_datasetid,
};
use reqwest::{Method, StatusCode};
use serde_json::{Number, json};
use serial_test::serial;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Append new attribute
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_append_new_attribute_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_01_01");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "attribute_to_be_added": {
            "type": "Property",
            "value": 23
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = append_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append new attribute should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["attribute_to_be_added"]["type"], "Property");
    assert_eq!(body["attribute_to_be_added"]["value"], 23);
    assert!(body.get("speed").is_some(), "existing attribute should remain");
}

#[tokio::test]
#[serial]
async fn attribute_append_preserves_an_exact_decimal() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_exact_number");
    assert_eq!(create_entity(&c, &vehicle_minimal(&id)).await, StatusCode::CREATED);
    let exact = Number::from_str("0.130000000000000005").expect("exact JSON number");
    let fragment = json!({"reading": {"type": "Property", "value": exact}, "@context": TEST_SUITE_CONTEXT});

    let (status, _) = append_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(serde_json::to_string(&body).expect("entity JSON").contains("0.130000000000000005"));
}

#[tokio::test]
#[serial]
async fn attribute_append_with_different_datasetid_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_01_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 60,
            "datasetId": "urn:ngsi-ld:Property:newspeedometer"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = append_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append with different datasetId should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some());
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_append_empty_entity_id_returns_error() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_attrs_method(&c, Method::POST, "/entities//attrs", Some(&fragment)).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
        "empty entity ID should return error status, got {status}"
    );
}

#[tokio::test]
#[serial]
async fn attribute_append_invalid_entity_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, body) = request_attrs_method(&c, Method::POST, "/entities/not-a-valid-uri/attrs", Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid URI should return 400");
    assert!(body["type"].as_str().is_some(), "should contain error type");
}

#[tokio::test]
#[serial]
async fn attribute_append_nonexistent_entity_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_03_01");

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, body) = append_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    assert!(body["type"].as_str().is_some(), "should contain error type");
}

#[tokio::test]
#[serial]
async fn attribute_append_invalid_json_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_05_01");

    let entity = vehicle_minimal(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = append_entity_attrs_raw(&c, &id, b"{invalid json", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid JSON should return 400");
}

// ---------------------------------------------------------------------------
// noOverwrite option
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_append_no_overwrite_existing_returns_207() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_04_01");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Try to overwrite existing speed attribute with noOverwrite
    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100,
            "datasetId": "urn:ngsi-ld:Property:gpsBxyz123-speed"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, body) = append_entity_attrs_with_options(&c, &id, &fragment, "noOverwrite", None).await;
    assert_eq!(status, StatusCode::MULTI_STATUS, "noOverwrite on existing should return 207");
    assert!(body.get("notUpdated").is_some() || body.get("updated").is_some());
}

#[tokio::test]
#[serial]
async fn attribute_append_no_overwrite_new_instance_returns_204() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_04_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Append new instance with different datasetId using noOverwrite
    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 60,
            "datasetId": "urn:ngsi-ld:Property:newspeedometer"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = append_entity_attrs_with_options(&c, &id, &fragment, "noOverwrite", None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "noOverwrite with new instance should return 204");
}

// ---------------------------------------------------------------------------
// LanguageProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_append_language_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_06_01");

    let entity = vehicle_minimal(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "en": "Grand Place",
                "fr": "Grand Place"
            }
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = append_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append LanguageProperty should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["type"], "LanguageProperty");
    assert_eq!(body["street"]["languageMap"]["fr"], "Grand Place");
}

// ---------------------------------------------------------------------------
// Scope handling
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_append_scope_with_overwrite_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:append_010_07_01");

    let entity = json!({
        "id": &id,
        "type": "Building",
        "scope": "/Madrid/Gardens/ParqueNorte"
    });
    let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "scope": "/CompanyA/OrganizationB/UnitC"
    });

    let (status, _) = append_entity_attrs_json(&c, &id, &fragment, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append scope should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scope"], "/CompanyA/OrganizationB/UnitC");
}

#[tokio::test]
#[serial]
async fn attribute_append_scope_with_no_overwrite_returns_multi_status_or_204() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:append_010_07_02");

    let entity = json!({
        "id": &id,
        "type": "Building",
        "scope": "/Madrid/Gardens/ParqueNorte"
    });
    let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "scope": "/CompanyA/OrganizationB/UnitC",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = append_entity_attrs_with_options(&c, &id, &fragment, "noOverwrite", None).await;
    assert!(
        status == StatusCode::NO_CONTENT || status == StatusCode::MULTI_STATUS,
        "noOverwrite scope should return 204 or 207, got {status}"
    );
}

// ---------------------------------------------------------------------------
// observationSpace / operationSpace
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_append_observation_space_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:append_010_08");

    let entity = building_minimal(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "observationSpace": {
            "type": "GeoProperty",
            "value": {
                "type": "Polygon",
                "coordinates": [[[13.35, 52.51], [13.40, 52.51], [13.40, 52.56], [13.35, 52.56], [13.35, 52.51]]]
            }
        }
    });

    let (status, _) = append_entity_attrs_json(&c, &id, &fragment, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append observationSpace should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["observationSpace"]["type"], "GeoProperty");
}

#[tokio::test]
#[serial]
async fn attribute_append_operation_space_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:append_010_09");

    let entity = building_minimal(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "operationSpace": {
            "type": "GeoProperty",
            "value": {
                "type": "Polygon",
                "coordinates": [[[13.30, 52.50], [13.45, 52.50], [13.45, 52.58], [13.30, 52.58], [13.30, 52.50]]]
            }
        }
    });

    let (status, _) = append_entity_attrs_json(&c, &id, &fragment, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append operationSpace should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["operationSpace"]["type"], "GeoProperty");
}

// ---------------------------------------------------------------------------
// JsonProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_append_json_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_10_01");

    let entity = vehicle_minimal(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "jsonProperty": {
            "type": "JsonProperty",
            "json": {
                "key1": "value1",
                "key2": 42
            }
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = append_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append JsonProperty should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["jsonProperty"]["type"], "JsonProperty");
    assert!(body["jsonProperty"]["json"].is_object());
}

// ---------------------------------------------------------------------------
// VocabProperty
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_append_vocab_property_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:append_010_11_01");

    let entity = vehicle_minimal(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "vocabProperty": {
            "type": "VocabProperty",
            "vocab": "residential"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = append_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append VocabProperty should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["vocabProperty"]["type"], "VocabProperty");
}
