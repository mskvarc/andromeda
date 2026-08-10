#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Attribute info retrieval tests (NGSI-LD 5.5.12, 6.3.5).

use crate::{
    COMPOUND_CONTEXT_URL,
    RESOURCE_NOT_FOUND_TYPE,
    building_simple_attributes,
    clean_database,
    client,
    create_entity,
    delete_entity,
    expected_attribute_info,
    retrieve_attribute,
    unique_id,
};
use reqwest::StatusCode;
use serial_test::serial;

// ---------------------------------------------------------------------------
// 5.5.12 -- Unknown attribute
// ---------------------------------------------------------------------------

/// Retrieve attribute info for a compacted name with no context -- 404 because the name cannot be resolved (5.5.12).
#[tokio::test]
#[serial]
async fn retrieve_attribute_info_unknown_attribute_returns_404() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attr-info-404");
    let entity = building_simple_attributes(&id);

    let (s, _) = create_entity(&c, &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    // "airQualityLevel" is a compacted name; without a context it cannot be expanded to a known IRI
    let (status, body) = retrieve_attribute(&c, "airQualityLevel", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()), "title should be a non-empty string");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.5.12, 6.3.5 -- Compacted attribute name
// ---------------------------------------------------------------------------

/// Retrieve attribute info by compacted name with context Link header -- returns Attribute (5.5.12, 6.3.5).
#[tokio::test]
#[serial]
async fn retrieve_attribute_info_with_compacted_name_returns_attribute_info() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attr-info-compact");

    let entity = building_simple_attributes(&id);
    let (s, _) = create_entity(&c, &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = retrieve_attribute(&c, "airQualityLevel", Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_attribute_info();
    pretty_assertions::assert_eq!(body["type"], expected["type"]);
    pretty_assertions::assert_eq!(body["attributeName"], expected["attributeName"]);
    pretty_assertions::assert_eq!(body["attributeCount"], expected["attributeCount"]);
    pretty_assertions::assert_eq!(body["attributeTypes"], expected["attributeTypes"]);
    pretty_assertions::assert_eq!(body["typeNames"], expected["typeNames"]);

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.5.12, 6.3.5 -- Expanded (URL-encoded) attribute name
// ---------------------------------------------------------------------------

/// Retrieve attribute info by URL-encoded expanded IRI -- returns same Attribute as compacted lookup (5.5.12, 6.3.5).
#[tokio::test]
#[serial]
async fn retrieve_attribute_info_with_expanded_name_returns_attribute_info() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attr-info-expanded");

    let entity = building_simple_attributes(&id);
    let (s, _) = create_entity(&c, &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    // "https://ngsi-ld-test-suite/context#airQualityLevel" percent-encoded
    let expanded_attr = "https%3A%2F%2Fngsi-ld-test-suite%2Fcontext%23airQualityLevel";
    let (status, body) = retrieve_attribute(&c, expanded_attr, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = expected_attribute_info();
    pretty_assertions::assert_eq!(body["type"], expected["type"]);
    pretty_assertions::assert_eq!(body["attributeName"], expected["attributeName"]);
    pretty_assertions::assert_eq!(body["attributeCount"], expected["attributeCount"]);
    pretty_assertions::assert_eq!(body["attributeTypes"], expected["attributeTypes"]);
    pretty_assertions::assert_eq!(body["typeNames"], expected["typeNames"]);

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.5.12 -- Attribute count accuracy
// ---------------------------------------------------------------------------

/// Retrieve attribute info after adding two entities with the same attribute -- attributeCount reflects both (5.5.12).
#[tokio::test]
#[serial]
async fn retrieve_attribute_info_count_reflects_all_entity_instances() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:Building:attr-count-a");
    let id2 = unique_id("urn:ngsi-ld:Building:attr-count-b");

    let e1 = building_simple_attributes(&id1);
    let e2 = building_simple_attributes(&id2);

    let (s, _) = create_entity(&c, &e1, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, &e2, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = retrieve_attribute(&c, "airQualityLevel", Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(
        body["attributeCount"],
        2,
        "attributeCount should be 2 after creating two entities with this attribute"
    );

    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
}
