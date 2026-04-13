#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Attribute details tests (NGSI-LD 5.5.11).
//! Uses `?details=true` to retrieve full `Attribute` objects with type and count information.

use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

use crate::{COMPOUND_CONTEXT_URL, clean_database, client, create_entity, delete_entity, list_attributes, load_expectation, load_fixture, unique_id};

fn sort_attributes(val: &Value) -> Vec<Value> {
    let mut arr: Vec<Value> = val.as_array().unwrap().clone();
    arr.sort_by(|a, b| a["attributeName"].as_str().unwrap().cmp(b["attributeName"].as_str().unwrap()));
    arr
}

// ---------------------------------------------------------------------------
// 5.5.11 -- Without Link header context
// ---------------------------------------------------------------------------

/// List attribute details without Link header -- attribute names returned as expanded IRIs (5.5.11).
#[tokio::test]
#[serial]
async fn list_attribute_details_without_context_returns_expanded_uris() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attr-details-no-ctx");

    let entity = load_fixture("entities/building-simple-attributes.json", &id);
    let (s, _) = create_entity(&c, &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_attributes(&c, "details=true", None).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = load_expectation("types/expectations/attribute-026-01-01.json");
    pretty_assertions::assert_eq!(sort_attributes(&body), sort_attributes(&expected));

    delete_entity(&c, &id).await;
}

// ---------------------------------------------------------------------------
// 5.5.11 -- With Link header context
// ---------------------------------------------------------------------------

/// List attribute details with compound context Link header -- attribute names compacted (5.5.11).
#[tokio::test]
#[serial]
async fn list_attribute_details_with_context_returns_compacted_names() {
    clean_database().await;
    let c = client();
    let id = unique_id("urn:ngsi-ld:Building:attr-details-with-ctx");

    let entity = load_fixture("entities/building-simple-attributes.json", &id);
    let (s, _) = create_entity(&c, &entity, Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(s, StatusCode::CREATED);

    let (status, body) = list_attributes(&c, "details=true", Some(COMPOUND_CONTEXT_URL)).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let expected = load_expectation("types/expectations/attribute-026-01-02.json");
    pretty_assertions::assert_eq!(sort_attributes(&body), sort_attributes(&expected));

    delete_entity(&c, &id).await;
}
