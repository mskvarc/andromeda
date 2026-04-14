//! Entity attribute deletion tests.
//!
//! Tests DELETE /entities/{entityId}/attrs/{attrId} operations:
//! - Delete attribute default instance
//! - Delete attribute with specific datasetId
//! - Delete all attribute instances (deleteAll=true)
//! - Error cases (invalid ID, non-existent entity/attribute)
//! - Special attributes (scope, observationSpace, operationSpace, VocabProperty)

use crate::{
    COMPOUND_CONTEXT,
    TEST_SUITE_CONTEXT,
    building_observation_space,
    building_operation_space,
    building_vocab_property_string,
    clean_database,
    client,
    create_entity,
    create_entity_json,
    delete_entity_attr,
    delete_entity_attr_delete_all,
    delete_entity_attr_with_datasetid,
    get_entity,
    request_attrs_method,
    unique_id,
    vehicle_speed_only_multi_instances,
    vehicle_speed_two_datasetid,
};
use reqwest::StatusCode;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Delete attribute instances
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_delete_default_instance_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_013_01_01");

    // vehicle_speed_only_multi_instances has a default instance (no datasetId) plus two with datasetId
    let entity = vehicle_speed_only_multi_instances(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr(&c, &id, "speed", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default instance should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    // Other instances with datasetId should remain
    assert!(body.get("speed").is_some(), "speed instances with datasetId should remain");
}

#[tokio::test]
#[serial]
async fn attribute_delete_with_datasetid_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_013_01_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr_with_datasetid(&c, &id, "speed", "urn:ngsi-ld:Property:gpsBxyz123-speed", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete with datasetId should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    // Other instance should remain
    assert!(body.get("speed").is_some(), "speed instance with other datasetId should remain");
}

#[tokio::test]
#[serial]
async fn attribute_delete_all_instances_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_013_01_03");

    let entity = vehicle_speed_only_multi_instances(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr_delete_all(&c, &id, "speed", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete all instances should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_none(), "all speed instances should be deleted");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_delete_empty_entity_id_returns_error() {
    clean_database().await;
    let c = client();

    let (status, _) = request_attrs_method(&c, "DELETE", "/entities//attrs/speed", None).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
        "empty entity ID should return error status, got {status}"
    );
}

#[tokio::test]
#[serial]
async fn attribute_delete_invalid_entity_id_returns_400() {
    clean_database().await;
    let c = client();

    let (status, _) = request_attrs_method(&c, "DELETE", "/entities/not-a-valid-uri/attrs/speed", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid entity ID should return 400");
}

#[tokio::test]
#[serial]
async fn attribute_delete_missing_attribute_name_returns_405() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_013_02_03");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = request_attrs_method(&c, "DELETE", &format!("/entities/{id}/attrs"), None).await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED, "DELETE /attrs without attribute name should return 405");
}

#[tokio::test]
#[serial]
async fn attribute_delete_nonexistent_entity_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_013_03_01");

    let (status, _) = delete_entity_attr(&c, &id, "speed", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
}

#[tokio::test]
#[serial]
async fn attribute_delete_nonexistent_attribute_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_013_03_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr(&c, &id, "nonExistentAttr", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent attribute should return 404");
}

#[tokio::test]
#[serial]
async fn attribute_delete_nonexistent_datasetid_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:delete_013_03_03");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr_with_datasetid(&c, &id, "speed", "urn:ngsi-ld:Dataset:nonexistent", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent datasetId should return 404");
}

// ---------------------------------------------------------------------------
// Special attributes
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_delete_scope_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:delete_013_04");

    let entity = serde_json::json!({
        "id": &id,
        "type": "Building",
        "scope": "/Madrid/Gardens/ParqueNorte"
    });
    let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr(&c, &id, "scope", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete scope should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("scope").is_none(), "scope should be deleted");
}

#[tokio::test]
#[serial]
async fn attribute_delete_observation_space_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:delete_013_05");

    let entity = building_observation_space(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr(&c, &id, "observationSpace", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete observationSpace should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("observationSpace").is_none(), "observationSpace should be deleted");
}

#[tokio::test]
#[serial]
async fn attribute_delete_operation_space_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:delete_013_06");

    let entity = building_operation_space(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr(&c, &id, "operationSpace", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete operationSpace should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("operationSpace").is_none(), "operationSpace should be deleted");
}

#[tokio::test]
#[serial]
async fn attribute_delete_vocab_property_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:delete_013_07_01");

    let entity = building_vocab_property_string(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = delete_entity_attr(&c, &id, "vocabProperty", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete VocabProperty should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(COMPOUND_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("vocabProperty").is_none(), "vocabProperty should be deleted");
}
