//! Temporal entity attribute instance delete tests.
//!
//! Tests DELETE /temporal/entities/{entityId}/attrs/{attrId}/{instanceId} operations:
//! - Delete specific attribute instance
//! - Error cases: invalid entity/attribute/instance IDs
//! - Not found cases: entity/attribute/instance not found

use crate::{
    COMPOUND_CONTEXT,
    client,
    create_temporal_entity,
    delete_temporal_attr_instance,
    extract_instance_id,
    get_temporal_entity,
    request_temporal_attrs,
    vehicle_temporal_representation,
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

async fn setup_entity_with_instance_id(scope: &TestEnvironment, local: &str) -> (reqwest::Client, String, String) {
    let c = client();
    let id = scope.id(local);

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let instance_id = extract_instance_id(&body, "speed");

    (c, id, instance_id)
}

#[tokio::test]
async fn temporal_attrs_delete_instance_success() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let (c, id, instance_id) = setup_entity_with_instance_id(&scope, "Vehicle:delete_inst_017_01_01").await;

    let (status, bytes, _) = delete_temporal_attr_instance(&c, &id, "speed", &instance_id).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete instance should return 204");
    assert!(bytes.is_empty(), "response body should be empty");

    let (status, _, _) = delete_temporal_attr_instance(&c, &id, "speed", &instance_id).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "repeating the same instance deletion must observe the first deletion"
    );

    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    let speed = body["speed"].as_array().expect("speed should be array");
    assert_eq!(speed.len(), 1, "should have 1 speed instance after deletion");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_attrs_delete_instance_invalid_entity_id_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let (_, _, instance_id) = setup_entity_with_instance_id(&scope, "Vehicle:delete_inst_017_02_01").await;
    let c = client();

    let (status, _, body) = delete_temporal_attr_instance(&c, "invalidId", "speed", &instance_id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_attrs_delete_instance_empty_entity_id_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let (_, _, instance_id) = setup_entity_with_instance_id(&scope, "Vehicle:delete_inst_017_02_02").await;
    let c = client();

    let (status, _body) = request_temporal_attrs(&c, Method::DELETE, &format!("/temporal/entities//attrs/speed/{instance_id}"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty entity ID should return 400");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_attrs_delete_instance_invalid_instance_id_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let (c, id, _) = setup_entity_with_instance_id(&scope, "Vehicle:delete_inst_017_02_03").await;

    let (status, _, body) = delete_temporal_attr_instance(&c, &id, "speed", "invalidId").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid instance ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_attrs_delete_instance_invalid_attribute_name_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let (c, id, instance_id) = setup_entity_with_instance_id(&scope, "Vehicle:delete_inst_017_02_04").await;

    let (status, _, body) = delete_temporal_attr_instance(&c, &id, "invalid(Name", &instance_id).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid attr name should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_attrs_delete_instance_empty_attribute_name_returns_400() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let (c, id, instance_id) = setup_entity_with_instance_id(&scope, "Vehicle:delete_inst_017_02_05").await;

    let (status, _) = request_temporal_attrs(&c, Method::DELETE, &format!("/temporal/entities/{id}/attrs//{instance_id}"), None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty attr name should return 400");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_attrs_delete_instance_entity_not_found_returns_404() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:delete_inst_017_03_01");

    let (status, _, body) = delete_temporal_attr_instance(&c, &id, "speed", "urn:ngsi-ld:Instance:nonexistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_attrs_delete_instance_attribute_not_found_returns_404() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let (c, id, instance_id) = setup_entity_with_instance_id(&scope, "Vehicle:delete_inst_017_03_02").await;

    let (status, _, body) = delete_temporal_attr_instance(&c, &id, "nonExistentAttr", &instance_id).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent attr should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn temporal_attrs_delete_instance_instance_not_found_returns_404() {
    let scope = TestEnvironment::from_suite("temporal_entity_attributes")
        .await
        .expect("isolated test environment");
    let (c, id, _) = setup_entity_with_instance_id(&scope, "Vehicle:delete_inst_017_03_03").await;

    let (status, _, body) = delete_temporal_attr_instance(&c, &id, "speed", "urn:ngsi-ld:01234567890123456789").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent instance should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
    scope.finish().await.expect("cleanup failed");
}
