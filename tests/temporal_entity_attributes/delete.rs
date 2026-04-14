//! Temporal entity attribute delete tests.
//!
//! Tests DELETE /temporal/entities/{entityId}/attrs/{attrId} operations:
//! - Delete attribute from temporal entity
//! - Delete with datasetId parameter
//! - Delete with deleteAll parameter
//! - Error cases: empty/invalid entity ID, invalid attribute name, entity/attribute not found

use crate::{
    COMPOUND_CONTEXT,
    clean_database,
    client,
    create_temporal_entity,
    delete_temporal_attr,
    delete_temporal_attr_with_params,
    get_temporal_entity,
    request_temporal_attrs,
    unique_id,
    vehicle_temporal_representation,
};
use reqwest::{Method, StatusCode};
use serial_test::serial;

// ---------------------------------------------------------------------------
// Delete attribute from temporal entity
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_attribute_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_015_01_01");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, bytes, _) = delete_temporal_attr(&c, &id, "fuelLevel").await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete attr should return 204");
    assert!(bytes.is_empty(), "response body should be empty");

    // Verify: fuelLevel should be removed (default instance only)
    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // speed should still exist
    assert!(body["speed"].is_array(), "speed should still exist");
    // fuelLevel should either be removed or have only the datasetId instance
    if let Some(fuel) = body.get("fuelLevel") {
        // If present, should only be the datasetId instance
        let arr = fuel.as_array().expect("fuelLevel should be array if present");
        for instance in arr {
            assert!(instance.get("datasetId").is_some(), "only datasetId instances should remain");
        }
    }
}

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_with_datasetid_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_015_01_02");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete the fuelLevel instance with specific datasetId
    let (status, bytes, _) = delete_temporal_attr_with_params(&c, &id, "fuelLevel", "datasetId=urn:ngsi-ld:Vehicle:12345-fuel").await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete attr with datasetId should return 204");
    assert!(bytes.is_empty(), "response body should be empty");

    // Verify
    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // fuelLevel should still have the default instances (without datasetId)
    let fuel = body["fuelLevel"].as_array().expect("fuelLevel should be array");
    for instance in fuel {
        assert!(
            instance.get("datasetId").is_none() || instance["datasetId"].is_null(),
            "datasetId instance should be removed"
        );
    }
}

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_with_deleteall_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_015_01_03");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Delete all fuelLevel instances
    let (status, bytes, _) = delete_temporal_attr_with_params(&c, &id, "fuelLevel", "deleteAll=true").await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete attr with deleteAll should return 204");
    assert!(bytes.is_empty(), "response body should be empty");

    // Verify
    let (status, body) = get_temporal_entity(&c, &id, Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);

    // fuelLevel should be completely removed
    assert!(
        body.get("fuelLevel").is_none() || body["fuelLevel"].is_null(),
        "fuelLevel should be removed with deleteAll"
    );
    // speed should still exist
    assert!(body["speed"].is_array(), "speed should still exist");
}

// ---------------------------------------------------------------------------
// Invalid entity/attribute ID
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_empty_entity_id_returns_400() {
    clean_database().await;
    let c = client();

    let (status, body) = request_temporal_attrs(&c, Method::DELETE, "/temporal/entities//attrs/fuelLevel", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty entity ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_invalid_entity_id_returns_400() {
    clean_database().await;
    let c = client();

    let (status, _, body) = delete_temporal_attr(&c, "invalidId", "speed").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_invalid_attribute_name_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_015_02_03");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = delete_temporal_attr(&c, &id, "invalid(Name").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid attr name should return 400");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("BadRequestData")),
        "error type should be BadRequestData"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

// ---------------------------------------------------------------------------
// Entity/attribute not found
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_entity_not_found_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_015_03_01");

    let (status, _, body) = delete_temporal_attr(&c, &id, "fuelLevel").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_attribute_not_found_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_015_03_02");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = delete_temporal_attr(&c, &id, "nonExistentAttr").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent attr should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
    assert!(body["title"].as_str().is_some(), "should contain error title");
}

#[tokio::test]
#[serial]
async fn temporal_attrs_delete_nonexistent_datasetid_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_015_03_03");

    let entity = vehicle_temporal_representation(&id);
    let status = create_temporal_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Try to delete with non-existent datasetId
    let (status, _, body) = delete_temporal_attr_with_params(&c, &id, "fuelLevel", "datasetId=urn:ngsi-ld:NonExistent").await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent datasetId should return 404");
    assert!(
        body["type"].as_str().is_some_and(|t| t.contains("ResourceNotFound")),
        "error type should be ResourceNotFound"
    );
}
