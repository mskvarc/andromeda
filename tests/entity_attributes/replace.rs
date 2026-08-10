//! Entity attribute replace tests.
//!
//! Tests PUT /entities/{entityId}/attrs/{attrId} operations:
//! - Replace existing attribute
//! - Replace with type change
//! - Replace specific instance by datasetId
//! - Error cases (invalid ID, non-existent entity/attribute, scope)

use crate::{
    TEST_SUITE_CONTEXT,
    clean_database,
    client,
    create_entity,
    create_entity_json,
    get_entity,
    replace_entity_attr,
    replace_entity_attr_json,
    replace_entity_attr_with_datasetid,
    request_attrs_method,
    unique_id,
    vehicle_speed_multi_instances,
    vehicle_speed_two_datasetid,
};
use reqwest::{Method, StatusCode};
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Replace existing attribute
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_replace_existing_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:replace_055_01_01");

    let entity = vehicle_speed_multi_instances(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Relationship",
        "object": "urn:ngsi-ld:OffStreetParking:Downtown2"
    });

    let (status, _) = replace_entity_attr_json(&c, &id, "isParked", &fragment, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "replace attribute should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["isParked"]["type"], "Relationship");
    assert_eq!(body["isParked"]["object"], "urn:ngsi-ld:OffStreetParking:Downtown2");
}

#[tokio::test]
#[serial]
async fn attribute_replace_with_type_change_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:replace_055_01_02");

    let entity = vehicle_speed_multi_instances(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Change isParked from Relationship to Property
    let fragment = json!({
        "type": "Property",
        "value": true
    });

    let (status, _) = replace_entity_attr_json(&c, &id, "isParked", &fragment, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "replace with type change should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["isParked"]["type"], "Property");
}

// ---------------------------------------------------------------------------
// Multi-instance handling
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_replace_default_instance_returns_404_when_no_default() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:replace_055_02_01");

    // vehicle_speed_two_datasetid has no default instance (both have datasetId)
    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity_attr(&c, &id, "speed", &fragment, None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "replace default instance should return 404 when no default exists"
    );
}

#[tokio::test]
#[serial]
async fn attribute_replace_specific_instance_by_datasetid_success() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:replace_055_02_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity_attr_with_datasetid(&c, &id, "speed", "urn:ngsi-ld:Property:speedometer", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "replace specific instance by datasetId should return 204");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_replace_invalid_entity_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_attrs_method(&c, Method::PUT, "/entities/not-a-valid-uri/attrs/speed", Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");
}

#[tokio::test]
#[serial]
async fn attribute_replace_nonexistent_entity_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:replace_055_03_02");

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity_attr(&c, &id, "speed", &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
}

#[tokio::test]
#[serial]
async fn attribute_replace_invalid_attribute_name_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:replace_055_03_03");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_attrs_method(&c, Method::PUT, &format!("/entities/{id}/attrs/@invalid"), Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid attribute name should return 400");
}

#[tokio::test]
#[serial]
async fn attribute_replace_nonexistent_attribute_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:replace_055_03_04");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity_attr(&c, &id, "nonExistentAttr", &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent attribute should return 404");
}

#[tokio::test]
#[serial]
async fn attribute_replace_scope_returns_400() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:replace_055_03_05");

    let entity = json!({
        "id": &id,
        "type": "Building",
        "scope": "/Madrid/Gardens/ParqueNorte"
    });
    let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": "/Barcelona",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity_attr(&c, &id, "scope", &fragment, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "replace scope should return 400");
}
