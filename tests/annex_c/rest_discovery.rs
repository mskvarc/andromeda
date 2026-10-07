//! Clauses C.5.7 to C.5.12: the type and attribute discovery examples.
//!
//! All six share one seeded tenant: two Vehicles, one `OffStreetParking` and one
//! Entity whose type and one Attribute are Fully Qualified Names no supplied
//! `@context` shortens. That last one is what makes the clauses' repeated NOTE
//! checkable -- names found in the `@context` are rendered short, the rest as
//! FQNs.
//!
//! Clause C.5.10 and C.5.11 print attribute lists that omit the parking
//! Attributes clause C.5.8 puts on the very same tenant. The corpus completes
//! them rather than reproducing the omission.

use crate::{
    clean_tenant,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    create_entity_batch,
    example_context::hosted_contexts,
    get_json,
};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};
use serde_json::Value;

/// Seed the discovery fixtures and answer the wrapper `@context` link.
async fn seed_discovery(http: &Client) -> String {
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    create_entity_batch(http, &corpus(include_str!("corpus/c-5-7-discovery-seed.json")), Some(&link)).await;
    link
}

/// Run one discovery request and return its answer.
async fn discover(http: &Client, link: &str, path: &str) -> Value {
    let (status, body) = get_json(http, path, "application/json", Some(link)).await;
    assert_eq!(status, StatusCode::OK, "discovery request {path} failed: {body}");
    body
}

/// The members of a list answer, sorted so the comparison does not depend on
/// an order clause 5.7.4 does not mandate.
fn sorted_strings(value: &Value) -> Vec<String> {
    let mut names: Vec<String> = value
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();
    names.sort();
    names
}

#[tokio::test]
async fn annex_c_5_7_retrieve_available_entity_types() {
    let environment = clean_tenant().await;
    let link = seed_discovery(&environment.client()).await;

    let actual = discover(&environment.client(), &link, "/types").await;
    assert_eq!(actual["type"], Value::String("EntityTypeList".to_owned()));
    assert!(
        actual["id"].as_str().is_some_and(|id| id.starts_with("urn:ngsi-ld:EntityTypeList:")),
        "clause C.5.7.3 names the list with a broker-minted EntityTypeList URI, got {}",
        actual["id"]
    );
    assert_eq!(
        sorted_strings(&actual["typeList"]),
        vec![
            "OffStreetParking".to_owned(),
            "Vehicle".to_owned(),
            "http://example.org/parking/ParkingSpot".to_owned()
        ],
        "clause C.5.7.3 renders the types the supplied @context knows as short names and the rest as FQNs"
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_8_retrieve_details_of_available_entity_types() {
    let environment = clean_tenant().await;
    let link = seed_discovery(&environment.client()).await;

    let actual = discover(&environment.client(), &link, "/types?details=true").await;
    let expected = corpus(include_str!("corpus/c-5-8-entity-type-details.json"));
    assert_matches_example(
        "C.5.8",
        &sorted_attribute_names(expected),
        &sorted_attribute_names(actual),
        ContextMember::Ignored,
        EntityOrder::Unordered,
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_9_retrieve_available_entity_type_information() {
    let environment = clean_tenant().await;
    let link = seed_discovery(&environment.client()).await;

    let actual = discover(&environment.client(), &link, "/types/Vehicle").await;
    let expected = corpus(include_str!("corpus/c-5-9-entity-type-information.json"));
    assert_matches_example(
        "C.5.9",
        &sorted_details(expected),
        &sorted_details(actual),
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_10_retrieve_available_attributes() {
    let environment = clean_tenant().await;
    let link = seed_discovery(&environment.client()).await;

    let actual = discover(&environment.client(), &link, "/attributes").await;
    assert_eq!(actual["type"], Value::String("AttributeList".to_owned()));
    assert!(
        actual["id"].as_str().is_some_and(|id| id.starts_with("urn:ngsi-ld:AttributeList:")),
        "clause C.5.10.3 names the list with a broker-minted AttributeList URI, got {}",
        actual["id"]
    );
    assert_eq!(
        sorted_strings(&actual["attributeList"]),
        vec![
            "availableSpotNumber".to_owned(),
            "brandName".to_owned(),
            "http://example.org/parking/status".to_owned(),
            "isNextToBuilding".to_owned(),
            "isParked".to_owned(),
            "location".to_owned(),
            "speed".to_owned(),
            "totalSpotNumber".to_owned()
        ],
        "clause C.5.10.3 renders the attributes the supplied @context knows as short names and the rest as FQNs"
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_11_retrieve_details_of_available_attributes() {
    let environment = clean_tenant().await;
    let link = seed_discovery(&environment.client()).await;

    let actual = discover(&environment.client(), &link, "/attributes?details=true").await;
    let expected = corpus(include_str!("corpus/c-5-11-attribute-details.json"));
    assert_matches_example(
        "C.5.11",
        &sorted_type_names(expected),
        &sorted_type_names(actual),
        ContextMember::Ignored,
        EntityOrder::Unordered,
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_5_12_retrieve_available_attribute_information() {
    let environment = clean_tenant().await;
    let link = seed_discovery(&environment.client()).await;

    let actual = discover(&environment.client(), &link, "/attributes/brandName").await;
    let expected = corpus(include_str!("corpus/c-5-12-attribute-information.json"));
    assert_matches_example("C.5.12", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

/// Order every `attributeNames` list alphabetically.
///
/// Clause 5.7.4 does not state an order for the list, and the annex prints one
/// list in a different order from the rest.
fn sorted_attribute_names(mut answer: Value) -> Value {
    if let Some(entries) = answer.as_array_mut() {
        for entry in entries.iter_mut() {
            if let Some(names) = entry.get_mut("attributeNames").and_then(Value::as_array_mut) {
                names.sort_by_key(|name| name.as_str().unwrap_or_default().to_owned());
            }
        }
    }
    answer
}

/// Order an `EntityTypeInfo` answer's `attributeDetails` by `id`.
fn sorted_details(mut answer: Value) -> Value {
    if let Some(details) = answer.get_mut("attributeDetails").and_then(Value::as_array_mut) {
        details.sort_by_key(|detail| detail.get("id").and_then(Value::as_str).unwrap_or_default().to_owned());
    }
    answer
}

/// Order an attribute-details answer's `typeNames` alphabetically.
fn sorted_type_names(mut answer: Value) -> Value {
    if let Some(entries) = answer.as_array_mut() {
        for entry in entries.iter_mut() {
            if let Some(names) = entry.get_mut("typeNames").and_then(Value::as_array_mut) {
                names.sort_by_key(|name| name.as_str().unwrap_or_default().to_owned());
            }
        }
    }
    answer
}
