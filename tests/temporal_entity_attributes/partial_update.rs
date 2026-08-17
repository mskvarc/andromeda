//! Temporal entity attribute instance partial update tests.
//!
//! Tests PATCH /temporal/entities/{entityId}/attrs/{attrId}/{instanceId} operations:
//! - Modify specific attribute instance
//! - Error cases: invalid entity/attribute/instance IDs
//! - Not found cases: entity/attribute/instance not found

use crate::{
    COMPOUND_CONTEXT,
    clean_database,
    client,
    create_temporal_entity,
    extract_instance_id,
    get_temporal_entity,
    get_temporal_entity_with_query,
    modify_instance_fragment,
    modify_temporal_attr_instance,
    request_temporal_attrs,
    unique_id,
    vehicle_temporal_representation,
};
use reqwest::{Method, StatusCode};
use serde_json::{Number, json};
use serial_test::serial;
use std::{str::FromStr, time::Duration};
use tokio::time::sleep;

// ---------------------------------------------------------------------------
// Setup helper
// ---------------------------------------------------------------------------

async fn setup_entity_with_instance_id(prefix: &str) -> (reqwest::Client, String, String) {
    let c = client();
    let id = unique_id(prefix);

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Get the entity to extract instanceId
    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let instance_id = extract_instance_id(&body, "speed");

    (c, id, instance_id)
}

// ---------------------------------------------------------------------------
// Modify attribute instance
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_success() {
    let _serialized_environment = clean_database().await;
    let (c, id, instance_id) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_01_01").await;

    let (status, before) = get_temporal_entity_with_query(&c, &id, "options=sysAttrs", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let before_instance = before["speed"]
        .as_array()
        .and_then(|instances| instances.iter().find(|instance| instance["instanceId"] == instance_id))
        .expect("instance must be visible before modification");
    let created_at = before_instance["createdAt"].as_str().expect("createdAt must be rendered").to_owned();
    let modified_at = before_instance["modifiedAt"].as_str().expect("modifiedAt must be rendered").to_owned();
    sleep(Duration::from_secs(1)).await;

    let fragment = modify_instance_fragment();
    let (status, bytes, _) = modify_temporal_attr_instance(&c, &id, "speed", &instance_id, &fragment).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "modify should return 204");
    assert!(bytes.is_empty(), "response body should be empty");

    // Verify
    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // speed should still exist
    let speed = body["speed"].as_array().expect("speed should be array");
    // Should have one instance with modified value 129
    let has_modified = speed.iter().any(|inst| inst["value"] == 129);
    assert!(has_modified, "should have instance with modified value 129");

    let (status, after) = get_temporal_entity_with_query(&c, &id, "options=sysAttrs", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let after_instance = after["speed"]
        .as_array()
        .and_then(|instances| instances.iter().find(|instance| instance["instanceId"] == instance_id))
        .expect("instance must be visible after modification");
    assert_eq!(after_instance["createdAt"], created_at, "createdAt must be stable across instance replacement");
    let advanced = after_instance["modifiedAt"].as_str().expect("modifiedAt must be rendered after update");
    assert!(advanced > modified_at.as_str(), "modifiedAt must advance across instance replacement");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_preserves_an_exact_decimal() {
    let _serialized_environment = clean_database().await;
    let (c, id, instance_id) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_exact_number").await;
    let exact = Number::from_str("0.170000000000000005").expect("exact JSON number");
    let fragment = json!({"type": "Property", "value": exact, "observedAt": "2020-09-01T12:03:00Z", "@context": [COMPOUND_CONTEXT]});

    let (status, bytes, _) = modify_temporal_attr_instance(&c, &id, "speed", &instance_id, &fragment).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(bytes.is_empty());
    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(serde_json::to_string(&body).expect("temporal entity JSON").contains("0.170000000000000005"));
}

// ---------------------------------------------------------------------------
// Invalid entity/attribute/instance IDs
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_invalid_entity_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let (_, _, instance_id) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_02_01").await;
    let c = client();

    let fragment = modify_instance_fragment();
    let (status, _, body) = modify_temporal_attr_instance(&c, "invalidId", "speed", &instance_id, &fragment).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_empty_entity_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let (_, _, instance_id) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_02_02").await;
    let c = client();

    let fragment = modify_instance_fragment();
    let (status, _) = request_temporal_attrs(&c, Method::PATCH, &format!("/temporal/entities//attrs/speed/{instance_id}"), Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty entity ID should return 400");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_invalid_instance_id_returns_400() {
    let _serialized_environment = clean_database().await;
    let (c, id, _) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_02_03").await;

    let fragment = modify_instance_fragment();
    let (status, _, body) = modify_temporal_attr_instance(&c, &id, "speed", "invalidId", &fragment).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid instance ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_empty_instance_id_returns_405() {
    let _serialized_environment = clean_database().await;
    let (c, id, _) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_02_04").await;

    let fragment = modify_instance_fragment();
    let (status, _) = request_temporal_attrs(&c, Method::PATCH, &format!("/temporal/entities/{id}/attrs/speed/"), Some(&fragment)).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "empty instance ID should return 405");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_invalid_attribute_name_returns_400() {
    let _serialized_environment = clean_database().await;
    let (c, id, instance_id) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_02_05").await;

    let fragment = modify_instance_fragment();
    let (status, _, body) = modify_temporal_attr_instance(&c, &id, "invalid(Id", &instance_id, &fragment).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid attr name should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_empty_attribute_name_returns_400() {
    let _serialized_environment = clean_database().await;
    let (c, id, instance_id) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_02_06").await;

    let fragment = modify_instance_fragment();
    let (status, _) = request_temporal_attrs(&c, Method::PATCH, &format!("/temporal/entities/{id}/attrs//{instance_id}"), Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty attr name should return 400");
}

// ---------------------------------------------------------------------------
// Entity/attribute/instance not found
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_entity_not_found_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:modify_016_03_01");

    let fragment = modify_instance_fragment();
    let (status, _, body) = modify_temporal_attr_instance(&c, &id, "speed", "urn:ngsi-ld:Instance:nonexistent", &fragment).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_attribute_not_found_returns_404() {
    let _serialized_environment = clean_database().await;
    let (c, id, instance_id) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_03_02").await;

    let fragment = modify_instance_fragment();
    let (status, _, body) = modify_temporal_attr_instance(&c, &id, "nonExistentAttr", &instance_id, &fragment).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent attr should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_modify_instance_instance_not_found_returns_404() {
    let _serialized_environment = clean_database().await;
    let (c, id, _) = setup_entity_with_instance_id("urn:ngsi-ld:Vehicle:modify_016_03_03").await;

    let fragment = modify_instance_fragment();
    let (status, _, body) = modify_temporal_attr_instance(&c, &id, "speed", "urn:ngsi-ld:01234567890123456789", &fragment).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent instance should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}
