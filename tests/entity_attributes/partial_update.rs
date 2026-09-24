//! Entity attribute partial update tests.
//!
//! Tests PATCH /entities/{entityId}/attrs/{attrId} operations:
//! - Partial update attribute value
//! - Partial update with datasetId
//! - Error cases (invalid ID, non-existent entity, mismatched type)
//! - `LanguageProperty`, `JsonProperty`, `VocabProperty`
//! - Deletion via NGSI-LD null sentinel

use crate::{
    COMPOUND_CONTEXT,
    TEST_SUITE_CONTEXT,
    building_different_attributes_instances_and_types,
    building_different_attributes_types,
    building_different_default_attributes_instances_and_types,
    building_json_property_object_sub_property,
    building_language_property_sub_property,
    building_vocab_property_sub_property,
    client,
    create_entity,
    get_entity,
    partial_update_attr,
    partial_update_attr_raw,
    partial_update_attr_with_datasetid,
    request_attrs_method,
    vehicle_speed_two_datasetid,
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;

// ---------------------------------------------------------------------------
// Partial update attribute
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attribute_partial_update_no_datasetid_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:partial_012_01_01");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Update the isParked relationship's sub-attribute
    let fragment = json!({
        "providedBy": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Person:JohnDoe"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "isParked", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "partial update should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["isParked"]["providedBy"]["object"], "urn:ngsi-ld:Person:JohnDoe");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_with_datasetid_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:partial_012_01_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "value": 65,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr_with_datasetid(&c, &id, "speed", "urn:ngsi-ld:Property:speedometer", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "partial update with datasetId should return 204");
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attribute_partial_update_empty_entity_id_returns_error() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();

    let fragment = json!({
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_attrs_method(&c, Method::PATCH, "/entities//attrs/speed", Some(&fragment)).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
        "empty entity ID should return error status, got {status}"
    );
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_invalid_entity_id_returns_400() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();

    let fragment = json!({
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_attrs_method(&c, Method::PATCH, "/entities/not-a-valid-uri/attrs/speed", Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid URI should return 400");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_mismatched_type_returns_400() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:partial_012_02_03");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Try to update Property with Relationship type
    let fragment = json!({
        "type": "Relationship",
        "object": "urn:ngsi-ld:Something:else",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr_with_datasetid(&c, &id, "speed", "urn:ngsi-ld:Property:speedometer", &fragment, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "mismatched attribute type should return 400");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_empty_fragment_returns_400() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:partial_012_02_04");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = partial_update_attr_raw(&c, &id, "speed", b"{}", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "empty fragment should return 400");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_nonexistent_entity_returns_404() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:partial_012_03_01");

    let fragment = json!({
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "speed", &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_no_default_instance_returns_404() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:partial_012_03_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // vehicle_speed_two_datasetid has no default speed instance (all have datasetId)
    let fragment = json!({
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "speed", &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "no default instance should return 404");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_nonexistent_datasetid_returns_404() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:partial_012_03_03");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr_with_datasetid(&c, &id, "speed", "urn:ngsi-ld:Property:unknownDatasetId", &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent datasetId should return 404");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_nonexistent_attribute_returns_404() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Vehicle:partial_012_03_04");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "value": 100,
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "nonExistentAttr", &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent attribute should return 404");
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// LanguageProperty
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attribute_partial_update_language_property_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_04_01");

    let entity = building_language_property_sub_property(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "languageMap": {
            "en": "Grand Place",
            "es": "Gran Lugar"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "street", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "partial update LanguageProperty should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["street"]["languageMap"]["es"], "Gran Lugar");
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Delete via NGSI-LD null in partial update
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attribute_partial_update_delete_property_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_05_01");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "name", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete Property via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("name").is_none(), "name should be deleted");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_relationship_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_05_02");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Relationship",
        "object": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "locatedAt", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete Relationship via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("locatedAt").is_none(), "locatedAt should be deleted");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_geo_property_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_05_03");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "GeoProperty",
        "value": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "location", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete GeoProperty via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("location").is_none(), "location should be deleted");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_language_property_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_05_04");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "LanguageProperty",
        "languageMap": {
            "@none": "urn:ngsi-ld:null"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "street", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete LanguageProperty via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("street").is_none(), "street should be deleted");
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Delete specific instance via NGSI-LD null in partial update
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attribute_partial_update_delete_property_instance_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_06_01");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr_with_datasetid(&c, &id, "name", "urn:ngsi-ld:Dataset:01", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete Property instance via null should return 204");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_relationship_instance_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_06_02");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Relationship",
        "object": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr_with_datasetid(&c, &id, "locatedAt", "urn:ngsi-ld:Dataset:01", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete Relationship instance via null should return 204");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_geo_property_instance_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_06_03");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "GeoProperty",
        "value": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr_with_datasetid(&c, &id, "location", "urn:ngsi-ld:Dataset:01", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete GeoProperty instance via null should return 204");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_language_property_instance_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_06_04");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "LanguageProperty",
        "languageMap": {
            "@none": "urn:ngsi-ld:null"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr_with_datasetid(&c, &id, "street", "urn:ngsi-ld:Dataset:01", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete LanguageProperty instance via null should return 204");
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// Delete default instance via NGSI-LD null in partial update
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attribute_partial_update_delete_default_property_instance_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_07_01");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Property",
        "value": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "name", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default Property instance via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("name").is_some(), "name with datasetId should remain");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_default_relationship_instance_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_07_02");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Relationship",
        "object": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "locatedAt", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default Relationship via null should return 204");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_default_geo_property_instance_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_07_03");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "GeoProperty",
        "value": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "location", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default GeoProperty via null should return 204");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn attribute_partial_update_delete_default_language_property_instance_with_null_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_07_04");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "LanguageProperty",
        "languageMap": {
            "@none": "urn:ngsi-ld:null"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "street", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default LanguageProperty via null should return 204");
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// JsonProperty
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attribute_partial_update_json_property_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_08_01");

    let entity = building_json_property_object_sub_property(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "json": {
            "id": "another-id",
            "data": [4, 5, 6]
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "jsonProperty", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "partial update JsonProperty should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["jsonProperty"]["json"]["id"], "another-id");
    scope.finish().await.expect("cleanup failed");
}

// ---------------------------------------------------------------------------
// VocabProperty
// ---------------------------------------------------------------------------

#[tokio::test]
async fn attribute_partial_update_vocab_property_success() {
    let scope = TestEnvironment::from_suite("entity_attributes").await.expect("isolated test environment");
    let c = client();
    let id = scope.id("Building:partial_012_09_01");

    let entity = building_vocab_property_sub_property(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "vocab": "commercial",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = partial_update_attr(&c, &id, "vocabProperty", &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "partial update VocabProperty should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["vocabProperty"]["type"], "VocabProperty");
    scope.finish().await.expect("cleanup failed");
}
