//! Entity type info retrieval tests (NGSI-LD 5.5.10, 6.3.5).

use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

use crate::{
    COMPOUND_CONTEXT_URL,
    RESOURCE_NOT_FOUND_TYPE,
    clean_database,
    client,
    create_entity,
    delete_entity,
    load_expectation,
    load_fixture,
    retrieve_entity_type,
    unique_id,
};

fn sort_attribute_details(details: &mut Vec<Value>) {
    details.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
}

// ---------------------------------------------------------------------------
// 5.5.10 -- Unknown type
// ---------------------------------------------------------------------------

/// Retrieve type info for a compacted name with no context -- 404 because the name cannot be resolved (5.5.10).
#[tokio::test]
#[serial]
async fn retrieve_entity_type_info_unknown_type_returns_404() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:type-info-404");
    let entity = load_fixture("entities/building-simple-attributes.json", &id);

    let (s, _) = create_entity(&c, &entity, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);

    // "Building" is a compacted name; without a context it cannot be expanded to a known IRI
    let (status, body) = retrieve_entity_type(&c, "Building", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);
    assert!(body["title"].as_str().is_some_and(|t| !t.is_empty()), "title should be a non-empty string");

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.5.10, 6.3.5 -- Compacted type name
// ---------------------------------------------------------------------------

/// Retrieve entity type info by compacted name with context Link header -- returns EntityTypeInfo (5.5.10, 6.3.5).
#[tokio::test]
#[serial]
async fn retrieve_entity_type_info_with_compacted_name_returns_type_info() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:Building:type-info-compact-a");
    let id2 = unique_id("urn:ngsi-ld:Building:type-info-compact-b");

    let e1 = load_fixture("entities/building-simple-attributes.json", &id1);
    let e2 = load_fixture("entities/building-simple-attributes.json", &id2);

    let (s, _) = create_entity(&c, &e1, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, &e2, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, body) = retrieve_entity_type(&c, "Building", Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(status, StatusCode::OK);

    let expected = load_expectation("types/expectations/entity-type-info-024-01.json");
    assert_eq!(body["type"], expected["type"]);
    assert_eq!(body["typeName"], expected["typeName"]);
    assert_eq!(body["entityCount"], expected["entityCount"]);

    let mut body_details: Vec<Value> = body["attributeDetails"].as_array().unwrap().clone();
    sort_attribute_details(&mut body_details);
    let mut expected_details: Vec<Value> = expected["attributeDetails"].as_array().unwrap().clone();
    sort_attribute_details(&mut expected_details);
    assert_eq!(body_details, expected_details);

    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
}

// ---------------------------------------------------------------------------
// 5.5.10, 6.3.5 -- Expanded (URL-encoded) type name
// ---------------------------------------------------------------------------

/// Retrieve entity type info by URL-encoded expanded IRI -- returns same EntityTypeInfo as compacted (5.5.10, 6.3.5).
#[tokio::test]
#[serial]
async fn retrieve_entity_type_info_with_expanded_name_returns_type_info() {
    clean_database().await;
    let c = client();
    let id1 = unique_id("urn:ngsi-ld:Building:type-info-expanded-a");
    let id2 = unique_id("urn:ngsi-ld:Building:type-info-expanded-b");

    let e1 = load_fixture("entities/building-simple-attributes.json", &id1);
    let e2 = load_fixture("entities/building-simple-attributes.json", &id2);

    let (s, _) = create_entity(&c, &e1, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = create_entity(&c, &e2, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);

    // "https://ngsi-ld-test-suite/context#Building" percent-encoded
    let expanded_type = "https%3A%2F%2Fngsi-ld-test-suite%2Fcontext%23Building";
    let (status, body) = retrieve_entity_type(&c, expanded_type, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(status, StatusCode::OK);

    let expected = load_expectation("types/expectations/entity-type-info-024-01.json");
    assert_eq!(body["type"], expected["type"]);
    assert_eq!(body["typeName"], expected["typeName"]);
    assert_eq!(body["entityCount"], expected["entityCount"]);

    let mut body_details: Vec<Value> = body["attributeDetails"].as_array().unwrap().clone();
    sort_attribute_details(&mut body_details);
    let mut expected_details: Vec<Value> = expected["attributeDetails"].as_array().unwrap().clone();
    sort_attribute_details(&mut expected_details);
    assert_eq!(body_details, expected_details);

    delete_entity(&c, &id1).await;
    delete_entity(&c, &id2).await;
}
