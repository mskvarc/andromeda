//! Entity attribute update tests.
//!
//! Tests PATCH /entities/{entityId}/attrs operations:
//! - Update existing attributes
//! - Update with datasetId
//! - Error cases (invalid ID, non-existent entity)
//! - Scope handling
//! - Type modification
//! - Deletion via NGSI-LD null sentinel

use crate::{
    COMPOUND_CONTEXT,
    TEST_SUITE_CONTEXT,
    building_different_attributes_instances_and_types,
    building_different_attributes_types,
    building_different_default_attributes_instances_and_types,
    clean_database,
    client,
    create_entity,
    create_entity_json,
    get_entity,
    request_attrs_method,
    unique_id,
    update_entity_attrs,
    update_entity_attrs_json,
    update_entity_attrs_raw,
    vehicle_speed_two_datasetid,
    vehicle_two_datasetid_attributes,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Update attribute
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_update_no_datasetid_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_01_01");

    let entity = vehicle_two_datasetid_attributes(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100,
            "datasetId": "urn:ngsi-ld:Property:gpsBxyz123-speed"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("speed").is_some());
}

#[tokio::test]
#[serial]
async fn attribute_update_with_datasetid_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_01_02");

    let entity = vehicle_speed_two_datasetid(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100,
            "datasetId": "urn:ngsi-ld:Property:speedometer"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update with datasetId should return 204");
}

#[tokio::test]
#[serial]
async fn attribute_update_with_new_attribute_returns_204_or_207() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_01_03");

    let entity = vehicle_two_datasetid_attributes(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100,
            "datasetId": "urn:ngsi-ld:Property:newspeedometer"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert!(
        status == StatusCode::NO_CONTENT || status == StatusCode::MULTI_STATUS,
        "update with new attribute should return 204 or 207, got {status}"
    );
}

#[tokio::test]
#[serial]
async fn attribute_update_change_type_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_01_04");

    let entity = vehicle_two_datasetid_attributes(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // Change brandName from Property to Relationship
    let fragment = json!({
        "brandName": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:Brand:Mercedes"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update changing type should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["brandName"]["type"], "Relationship");
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_update_empty_entity_id_returns_error() {
    clean_database().await;
    let c = client();

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_attrs_method(&c, "PATCH", "/entities//attrs", Some(&fragment)).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::NOT_FOUND || status == StatusCode::METHOD_NOT_ALLOWED,
        "empty entity ID should return error status, got {status}"
    );
}

#[tokio::test]
#[serial]
async fn attribute_update_invalid_entity_id_returns_400() {
    clean_database().await;
    let c = client();

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_attrs_method(&c, "PATCH", "/entities/not-a-valid-uri/attrs", Some(&fragment)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid URI should return 400");
}

#[tokio::test]
#[serial]
async fn attribute_update_nonexistent_entity_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_03_01");

    let fragment = json!({
        "speed": {
            "type": "Property",
            "value": 100
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
}

#[tokio::test]
#[serial]
async fn attribute_update_invalid_json_returns_400() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_04_01");

    let entity = vehicle_two_datasetid_attributes(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_entity_attrs_raw(&c, &id, b"{invalid json", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid JSON should return 400");
}

// ---------------------------------------------------------------------------
// Scope handling
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_update_scope_existing_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_05_01");

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

    let (status, _) = update_entity_attrs_json(&c, &id, &fragment, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "update existing scope should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["scope"], "/CompanyA/OrganizationB/UnitC");
}

#[tokio::test]
#[serial]
async fn attribute_update_scope_not_existing_returns_207_or_204() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_05_02");

    let entity = json!({
        "id": &id,
        "type": "Building"
    });
    let status = create_entity_json(&c, &entity, TEST_SUITE_CONTEXT).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "scope": "/CompanyA/OrganizationB/UnitC"
    });

    let (status, _) = update_entity_attrs_json(&c, &id, &fragment, TEST_SUITE_CONTEXT).await;
    assert!(
        status == StatusCode::MULTI_STATUS || status == StatusCode::NO_CONTENT,
        "update non-existing scope should return 207 or 204, got {status}"
    );
}

// ---------------------------------------------------------------------------
// Type modification
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_update_append_single_type_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_06_01");

    let entity = vehicle_two_datasetid_attributes(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Building",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append type should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    if let Some(types) = body["type"].as_array() {
        let type_strs: Vec<&str> = types.iter().filter_map(|t| t.as_str()).collect();
        assert!(type_strs.contains(&"Vehicle"));
        assert!(type_strs.contains(&"Building"));
    }
}

#[tokio::test]
#[serial]
async fn attribute_update_append_multiple_types_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_06_02");

    let entity = vehicle_two_datasetid_attributes(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": ["Vehicle", "Car"],
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append multiple types should return 204");
}

#[tokio::test]
#[serial]
async fn attribute_update_append_duplicate_type_ignored() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_06_03");

    let entity = vehicle_two_datasetid_attributes(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "type": "Vehicle",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append duplicate type should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "Vehicle");
}

#[tokio::test]
#[serial]
async fn attribute_update_append_type_to_multi_type_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Vehicle:update_011_06_04");

    let entity = vehicle_two_datasetid_attributes(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    // First add "Car" type
    let car_fragment = json!({
        "type": ["Vehicle", "Car"],
        "@context": TEST_SUITE_CONTEXT
    });
    let (status, _) = update_entity_attrs(&c, &id, &car_fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // Then add "Building" type
    let building_fragment = json!({
        "type": "Building",
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &building_fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "append type to multi-type should return 204");
}

// ---------------------------------------------------------------------------
// Delete via NGSI-LD null
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_update_delete_property_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_07_01");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "name": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("name").is_none(), "name should be deleted");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_relationship_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_07_02");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "locatedAt": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete relationship via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("locatedAt").is_none(), "locatedAt should be deleted");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_geo_property_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_07_03");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "location": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete GeoProperty via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("location").is_none(), "location should be deleted");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_language_property_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_07_04");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "street": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete LanguageProperty via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("street").is_none(), "street should be deleted");
}

// ---------------------------------------------------------------------------
// Delete specific instance via NGSI-LD null
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_update_delete_property_instance_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_08_01");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "name": {
            "type": "Property",
            "value": "urn:ngsi-ld:null",
            "datasetId": "urn:ngsi-ld:Dataset:01"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete Property instance via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("name").is_some(), "name with other datasetId should remain");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_relationship_instance_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_08_02");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "locatedAt": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:null",
            "datasetId": "urn:ngsi-ld:Dataset:01"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete Relationship instance via null should return 204");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_geo_property_instance_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_08_03");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "location": {
            "type": "GeoProperty",
            "value": "urn:ngsi-ld:null",
            "datasetId": "urn:ngsi-ld:Dataset:01"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete GeoProperty instance via null should return 204");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_language_property_instance_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_08_04");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "@none": "urn:ngsi-ld:null"
            },
            "datasetId": "urn:ngsi-ld:Dataset:01"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete LanguageProperty instance via null should return 204");
}

// ---------------------------------------------------------------------------
// Delete default instance via NGSI-LD null
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn attribute_update_delete_default_property_instance_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_09_01");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "name": {
            "type": "Property",
            "value": "urn:ngsi-ld:null"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default Property via null should return 204");

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("name").is_some(), "name with datasetId should remain");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_default_relationship_instance_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_09_02");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "locatedAt": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:null"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default Relationship via null should return 204");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_default_geo_property_instance_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_09_03");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "location": {
            "type": "GeoProperty",
            "value": "urn:ngsi-ld:null"
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default GeoProperty via null should return 204");
}

#[tokio::test]
#[serial]
async fn attribute_update_delete_default_language_property_instance_with_null_success() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:update_011_09_04");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = json!({
        "street": {
            "type": "LanguageProperty",
            "languageMap": {
                "@none": "urn:ngsi-ld:null"
            }
        },
        "@context": COMPOUND_CONTEXT
    });

    let (status, _) = update_entity_attrs(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "delete default LanguageProperty via null should return 204");
}
