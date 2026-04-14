#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Entity type list tests (NGSI-LD 5.5.9, 6.3.5).

use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

use crate::{
    COMPOUND_CONTEXT_URL,
    building_simple_attributes,
    clean_database,
    client,
    create_entity,
    delete_entity,
    expected_entity_type_list_compacted,
    expected_entity_type_list_expanded,
    list_entity_types,
    unique_id,
    vehicle_simple_attributes,
};

fn sorted_strings(val: &Value) -> Vec<String> {
    let mut v: Vec<String> = val.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------
// 5.5.9 -- Empty database
// ---------------------------------------------------------------------------

/// List entity types on empty database -- returns EntityTypeList with empty typeList (5.5.9).
#[tokio::test]
#[serial]
async fn list_entity_types_empty_database_returns_empty_list() {
    clean_database().await;
    let c = client();

    let (status, body) = list_entity_types(&c, "", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["type"], "EntityTypeList");
    let types = body["typeList"].as_array().expect("typeList should be array");
    assert!(types.is_empty(), "expected empty typeList on clean database, got: {types:?}");
}

// ---------------------------------------------------------------------------
// 5.5.9, 6.3.5 -- Without Link header context
// ---------------------------------------------------------------------------

/// List entity types without Link header -- type URIs returned as expanded (absolute) IRIs (5.5.9, 6.3.5).
#[tokio::test]
#[serial]
async fn list_entity_types_without_context_returns_expanded_uris() {
    clean_database().await;
    let c = client();
    let building_id = unique_id("urn:ngsi-ld:Building:types-no-ctx");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:types-no-ctx");

    let building = building_simple_attributes(&building_id);
    let vehicle = vehicle_simple_attributes(&vehicle_id);

    let (s, _) = create_entity(&c, &building, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, &vehicle, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_entity_types(&c, "", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_entity_type_list_expanded();
    pretty_assertions::assert_eq!(body["type"], "EntityTypeList");
    pretty_assertions::assert_eq!(sorted_strings(&body["typeList"]), sorted_strings(&expected["typeList"]));

    delete_entity(&c, &building_id).await;
    delete_entity(&c, &vehicle_id).await;
}

// ---------------------------------------------------------------------------
// 5.5.9, 6.3.5 -- With Link header context
// ---------------------------------------------------------------------------

/// List entity types with compound context Link header -- type URIs compacted to short names (5.5.9, 6.3.5).
#[tokio::test]
#[serial]
async fn list_entity_types_with_context_returns_compacted_names() {
    clean_database().await;
    let c = client();
    let building_id = unique_id("urn:ngsi-ld:Building:types-with-ctx");
    let vehicle_id = unique_id("urn:ngsi-ld:Vehicle:types-with-ctx");

    let building = building_simple_attributes(&building_id);
    let vehicle = vehicle_simple_attributes(&vehicle_id);

    let (s, _) = create_entity(&c, &building, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, &vehicle, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_entity_types(&c, "", Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_entity_type_list_compacted();
    pretty_assertions::assert_eq!(body["type"], "EntityTypeList");
    pretty_assertions::assert_eq!(sorted_strings(&body["typeList"]), sorted_strings(&expected["typeList"]));

    delete_entity(&c, &building_id).await;
    delete_entity(&c, &vehicle_id).await;
}
