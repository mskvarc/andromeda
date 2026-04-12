//! Attribute list tests (NGSI-LD 5.5.11, 6.3.5).

use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

use crate::{COMPOUND_CONTEXT_URL, clean_database, client, create_entity, delete_entity, list_attributes, load_expectation, load_fixture, unique_id};

fn sorted_strings(val: &Value) -> Vec<String> {
    let mut v: Vec<String> = val.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect();
    v.sort();
    v
}

// ---------------------------------------------------------------------------
// 5.5.11 -- Empty database
// ---------------------------------------------------------------------------

/// List attributes on empty database -- returns AttributeList with empty attributeList (5.5.11).
#[tokio::test]
#[serial]
async fn list_attributes_empty_database_returns_empty_list() {
    clean_database().await;
    let c = client();

    let (status, body) = list_attributes(&c, "", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["type"], "AttributeList");
    let attrs = body["attributeList"].as_array().expect("attributeList should be array");
    assert!(attrs.is_empty(), "expected empty attributeList on clean database, got: {attrs:?}");
}

// ---------------------------------------------------------------------------
// 5.5.11, 6.3.5 -- Without Link header context
// ---------------------------------------------------------------------------

/// List attributes without Link header -- attribute URIs returned as expanded (absolute) IRIs (5.5.11, 6.3.5).
#[tokio::test]
#[serial]
async fn list_attributes_without_context_returns_expanded_uris() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attrs-no-ctx");

    let entity = load_fixture("entities/building-simple-attributes.json", &id);
    let (s, _) = create_entity(&c, &entity, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_attributes(&c, "", None).await;
    assert_eq!(status, StatusCode::OK);

    let expected = load_expectation("types/expectations/attribute-list-025-01-01.json");
    assert_eq!(body["type"], "AttributeList");
    assert_eq!(sorted_strings(&body["attributeList"]), sorted_strings(&expected["attributeList"]));

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.5.11, 6.3.5 -- With Link header context
// ---------------------------------------------------------------------------

/// List attributes with compound context Link header -- attribute names compacted to short names (5.5.11, 6.3.5).
#[tokio::test]
#[serial]
async fn list_attributes_with_context_returns_compacted_names() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attrs-with-ctx");

    let entity = load_fixture("entities/building-simple-attributes.json", &id);
    let (s, _) = create_entity(&c, &entity, Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_attributes(&c, "", Some(COMPOUND_CONTEXT_URL)).await;
    assert_eq!(status, StatusCode::OK);

    let expected = load_expectation("types/expectations/attribute-list-025-01-02.json");
    assert_eq!(body["type"], "AttributeList");
    assert_eq!(sorted_strings(&body["attributeList"]), sorted_strings(&expected["attributeList"]));

    delete_entity(&c, &id).await;
}
