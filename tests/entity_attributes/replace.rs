//! Entity attribute replace tests.
//!
//! Tests PUT /entities/{entityId}/attrs/{attrId} operations:
//! - Replace existing attribute
//! - Replace with type change
//! - Replace specific instance by datasetId
//! - Error cases (invalid ID, non-existent entity/attribute, scope)

use crate::{
    TEST_SUITE_CONTEXT,
    client,
    create_entity,
    create_entity_json,
    get_entity,
    replace_entity_attr,
    replace_entity_attr_json,
    request_attrs_method,
    vehicle_speed_multi_instances,
    vehicle_speed_two_datasetid,
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Number, json};
use std::str::FromStr;

#[tokio::test]
async fn attribute_replace_existing_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:replace_055_01_01");

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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_preserves_an_exact_decimal() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:replace_exact_number");
    assert_eq!(
        create_entity_json(&c, &vehicle_speed_multi_instances(&id), TEST_SUITE_CONTEXT).await,
        StatusCode::CREATED
    );
    let exact = Number::from_str("0.150000000000000005").expect("exact JSON number");
    let fragment = json!({"type": "Property", "value": exact, "@context": TEST_SUITE_CONTEXT});

    let (status, _) = replace_entity_attr(&c, &id, "speed", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(serde_json::to_string(&body).expect("entity JSON").contains("0.150000000000000005"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_with_type_change_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:replace_055_01_02");

    let entity = vehicle_speed_multi_instances(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": true
    });

    let (status, _) = replace_entity_attr_json(&c, &id, "isParked", &fragment, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "replace with type change should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["isParked"]["type"], "Property");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_default_instance_returns_404_when_no_default() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:replace_055_02_01");

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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_specific_instance_by_datasetid_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:replace_055_02_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "datasetId": "urn:ngsi-ld:Property:speedometer",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity_attr(&c, &id, "speed", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "replace specific instance by datasetId should return 204");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_invalid_entity_id_returns_400() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_attrs_method(&c, Method::PUT, "/entities/not-a-valid-uri/attrs/speed", Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_nonexistent_entity_returns_404() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:replace_055_03_02");

    let fragment = json!({
        "type": "Property",
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = replace_entity_attr(&c, &id, "speed", &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_invalid_attribute_name_returns_400() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:replace_055_03_03");

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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_nonexistent_attribute_returns_404() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:replace_055_03_04");

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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_replace_scope_returns_400() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:replace_055_03_05");

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
    scope.finish().await.expect("cleanup failed");
}
