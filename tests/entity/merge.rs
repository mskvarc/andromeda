//! Entity merge tests.
//!
//! Tests entity merge (PATCH) operations:
//! - Merge empty fragment
//! - Merge simple properties
//! - Merge relationship
//! - Merge partial properties (preserve sub-attributes)
//! - Error cases (invalid ID, missing ID, not found)
//! - Delete attributes via NGSI-LD null
//! - Delete specific attribute instances by datasetId

use crate::{
    COMPOUND_CONTEXT,
    TEST_SUITE_CONTEXT,
    building_different_attributes_instances_and_types,
    building_different_attributes_types,
    building_different_default_attributes_instances_and_types,
    building_merge_data,
    building_minimal,
    building_only_airqualitylevel_value,
    building_relationship_for_merge,
    building_simple_attributes_second_for_merge,
    clean_database,
    client,
    create_entity,
    get_entity,
    merge_entity,
    request_method_on_collection,
    unique_id,
};
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

// ---------------------------------------------------------------------------
// 056_01: Merge entity scenarios
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_merge_empty_fragment_unchanged() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_01_01");

    let entity = building_merge_data(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = building_minimal(&id);
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "merge should return 204");

    // Verify entity unchanged
    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["value"], "Eiffel Tower");
    assert_eq!(body["almostFull"]["value"], false);
}

#[tokio::test]
#[serial]
async fn entity_merge_simple_properties_updated() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_01_02");

    let entity = building_merge_data(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = building_simple_attributes_second_for_merge(&id);
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"]["value"], "Pisa Tower");
    assert_eq!(body["almostFull"]["value"], true);
    assert_eq!(body["airQualityLevel"]["value"], 6);
}

#[tokio::test]
#[serial]
async fn entity_merge_new_relationship_added() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_01_03");

    let entity = building_merge_data(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = building_relationship_for_merge(&id);
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["locatedAt"]["type"], "Relationship");
    assert_eq!(body["locatedAt"]["object"], "urn:ngsi-ld:City:Paris");
    // Original properties should still be present
    assert_eq!(body["name"]["value"], "Eiffel Tower");
}

#[tokio::test]
#[serial]
async fn entity_merge_partial_properties_preserved() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_01_04");

    let entity = building_merge_data(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = building_only_airqualitylevel_value(&id);
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["airQualityLevel"]["value"], 1);
    let observed = body["airQualityLevel"]["observedAt"].as_str().unwrap();
    assert!(observed.starts_with("2024-01-01T00:00:00"), "observedAt should match: {observed}");
    // unitCode should be preserved from original
    assert_eq!(body["airQualityLevel"]["unitCode"], "C62");
    // Other properties unchanged
    assert_eq!(body["name"]["value"], "Eiffel Tower");
}

// ---------------------------------------------------------------------------
// 056_02: Merge entity error cases
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_merge_invalid_id_returns_400() {
    clean_database().await;
    let c = client();

    let fragment = json!({
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = merge_entity(&c, "invalidUri", &fragment, None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "invalid ID should return 400");
}

#[tokio::test]
#[serial]
async fn entity_merge_missing_id_returns_error() {
    clean_database().await;
    let c = client();

    let fragment = json!({
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = request_method_on_collection(&c, "PATCH", Some(&fragment)).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::METHOD_NOT_ALLOWED,
        "missing ID should return 400 or 405, got {status}",
    );
}

#[tokio::test]
#[serial]
async fn entity_merge_not_found_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_02_03");

    let fragment = json!({
        "id": &id,
        "type": "Building",
        "name": {
            "type": "Property",
            "value": "Pisa Tower"
        },
        "@context": TEST_SUITE_CONTEXT
    });

    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "non-existent entity should return 404");
}

// ---------------------------------------------------------------------------
// 056_03: Delete attributes via NGSI-LD null
// ---------------------------------------------------------------------------

fn null_property() -> serde_json::Value {
    json!({
        "name": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    })
}

fn null_relationship() -> serde_json::Value {
    json!({
        "locatedAt": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    })
}

fn null_geoproperty() -> serde_json::Value {
    json!({
        "location": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    })
}

fn null_languageproperty() -> serde_json::Value {
    json!({
        "street": "urn:ngsi-ld:null",
        "@context": COMPOUND_CONTEXT
    })
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_property_via_null() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_03_01");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_property();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("name").is_none(), "name should be deleted");
    // Other attributes should remain
    assert!(body.get("locatedAt").is_some(), "locatedAt should remain");
    assert!(body.get("location").is_some(), "location should remain");
    assert!(body.get("street").is_some(), "street should remain");
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_relationship_via_null() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_03_02");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_relationship();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("locatedAt").is_none(), "locatedAt should be deleted");
    assert!(body.get("name").is_some(), "name should remain");
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_geoproperty_via_null() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_03_03");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_geoproperty();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("location").is_none(), "location should be deleted");
    assert!(body.get("name").is_some(), "name should remain");
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_languageproperty_via_null() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_03_04");

    let entity = building_different_attributes_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_languageproperty();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("street").is_none(), "street should be deleted");
    assert!(body.get("name").is_some(), "name should remain");
}

// ---------------------------------------------------------------------------
// 056_04: Delete specific attribute instance via NGSI-LD null
// ---------------------------------------------------------------------------

fn null_property_instance() -> serde_json::Value {
    json!({
        "name": {
            "type": "Property",
            "value": "urn:ngsi-ld:null",
            "datasetId": "urn:ngsi-ld:Dataset:01"
        },
        "@context": COMPOUND_CONTEXT
    })
}

fn null_relationship_instance() -> serde_json::Value {
    json!({
        "locatedAt": {
            "type": "Relationship",
            "object": "urn:ngsi-ld:null",
            "datasetId": "urn:ngsi-ld:Dataset:01"
        },
        "@context": COMPOUND_CONTEXT
    })
}

fn null_geoproperty_instance() -> serde_json::Value {
    json!({
        "location": {
            "type": "GeoProperty",
            "value": "urn:ngsi-ld:null",
            "datasetId": "urn:ngsi-ld:Dataset:01"
        },
        "@context": COMPOUND_CONTEXT
    })
}

fn null_languageproperty_instance() -> serde_json::Value {
    json!({
        "street": {
            "type": "LanguageProperty",
            "languageMap": "urn:ngsi-ld:null",
            "datasetId": "urn:ngsi-ld:Dataset:01"
        },
        "@context": COMPOUND_CONTEXT
    })
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_property_instance_by_datasetid() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_04_01");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_property_instance();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    // Dataset:01 instance should be gone, Dataset:02 should remain
    let name = &body["name"];
    if name.is_array() {
        let names = name.as_array().unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(names[0]["datasetId"], "urn:ngsi-ld:Dataset:02");
    } else {
        assert_eq!(name["datasetId"], "urn:ngsi-ld:Dataset:02");
    }
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_relationship_instance_by_datasetid() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_04_02");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_relationship_instance();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let located = &body["locatedAt"];
    if located.is_array() {
        let items = located.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["datasetId"], "urn:ngsi-ld:Dataset:02");
    } else {
        assert_eq!(located["datasetId"], "urn:ngsi-ld:Dataset:02");
    }
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_geoproperty_instance_by_datasetid() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_04_03");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_geoproperty_instance();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let loc = &body["location"];
    if loc.is_array() {
        let items = loc.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["datasetId"], "urn:ngsi-ld:Dataset:02");
    } else {
        assert_eq!(loc["datasetId"], "urn:ngsi-ld:Dataset:02");
    }
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_languageproperty_instance_by_datasetid() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_04_04");

    let entity = building_different_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_languageproperty_instance();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    let street = &body["street"];
    if street.is_array() {
        let items = street.as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["datasetId"], "urn:ngsi-ld:Dataset:02");
    } else {
        assert_eq!(street["datasetId"], "urn:ngsi-ld:Dataset:02");
    }
}

// ---------------------------------------------------------------------------
// 056_05: Delete all attribute instances via NGSI-LD null (entity with defaults)
// Per NGSI-LD 5.6.1: null sentinel without datasetId removes ALL instances
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn entity_merge_delete_all_property_instances_via_null() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_05_01");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_property();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    // Per 5.6.1: null without datasetId removes ALL instances
    assert!(body.get("name").is_none(), "name should be fully deleted");
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_all_relationship_instances_via_null() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_05_02");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_relationship();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("locatedAt").is_none(), "locatedAt should be fully deleted");
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_all_geoproperty_instances_via_null() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_05_03");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_geoproperty();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("location").is_none(), "location should be fully deleted");
}

#[tokio::test]
#[serial]
async fn entity_merge_delete_all_languageproperty_instances_via_null() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:merge_056_05_04");

    let entity = building_different_default_attributes_instances_and_types(&id);
    let status = create_entity(&c, &entity).await;
    assert_eq!(status, StatusCode::CREATED);

    let fragment = null_languageproperty();
    let (status, _) = merge_entity(&c, &id, &fragment, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = get_entity(&c, &id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.get("street").is_none(), "street should be fully deleted");
}
