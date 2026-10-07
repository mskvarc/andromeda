//! Clause C.10: `valueType` as untyped datatype metadata beside a value.
//!
//! The clause's `Place` ontology admits two shapes for the same `address`
//! Property -- an unstructured string and a structured `PostalAddress` object
//! -- and distinguishes them by the Property's `valueType` rather than by a
//! JSON-LD typed value. Both have to survive a round trip with their
//! `valueType` intact, or a consumer cannot tell which shape it received.

use crate::{
    clean_tenant,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    create_entity,
    example_context::hosted_contexts,
    get_json,
};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

/// Both `Place` Entities exactly as clause C.10 prints them.
const PLACES: &str = include_str!("corpus/c-10-places.json");

/// Seed both Places and answer the `@context` link the ontology is hosted at.
///
/// The two Entities are written one at a time rather than as a batch: clause
/// 5.6.7's batch path does not resolve the compact IRIs this ontology's term
/// definitions are written with, which would store them under a different set
/// of IRIs than clause 5.6.1's path does.
async fn seed_places(http: &Client) -> String {
    let link = context_link(hosted_contexts().url("place-context.jsonld").as_str());
    for place in corpus(PLACES).as_array().expect("the printed C.10 payload is an array") {
        create_entity(http, place, Some(&link)).await;
    }
    link
}

/// Retrieve one Place under the ontology it was written with.
async fn retrieve_place(http: &Client, link: &str, identifier: &str) -> Value {
    let (status, body) = get_json(http, &format!("/entities/{identifier}"), "application/ld+json", Some(link)).await;
    assert_eq!(status, StatusCode::OK, "Place retrieval failed: {body}");
    body
}

#[tokio::test]
async fn annex_c_10_a_value_type_holds_untyped_datatype_metadata_for_a_value() {
    let environment = clean_tenant().await;
    let link = seed_places(&environment.client()).await;

    let actual = retrieve_place(&environment.client(), &link, "urn:ngsi-ld:Place:27182").await;
    let expected = corpus(PLACES)[0].clone(); // clone: the expected payload is one member of the printed array.
    assert_matches_example("C.10 String address", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);
    assert_eq!(
        actual["address"]["value"],
        json!("Pariser Platz, Berlin, Germany"),
        "clause C.10 keeps the value untyped and puts the datatype beside it"
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_10_two_value_shapes_of_one_property_stay_distinguishable() {
    let environment = clean_tenant().await;
    let link = seed_places(&environment.client()).await;

    let unstructured = retrieve_place(&environment.client(), &link, "urn:ngsi-ld:Place:27182").await;
    let structured = retrieve_place(&environment.client(), &link, "urn:ngsi-ld:Place:31415").await;

    assert_eq!(unstructured["address"]["valueType"], json!("String"));
    assert_eq!(structured["address"]["valueType"], json!("PostalAddress"));
    assert_eq!(
        structured["address"]["value"],
        json!({"street": "Straße des 17. Juni", "city": "Berlin", "country": "Germany"}),
        "clause C.10 keeps the structured value as the JSON object it was given"
    );

    let expected = corpus(PLACES)[1].clone(); // clone: the expected payload is one member of the printed array.
    assert_matches_example(
        "C.10 PostalAddress address",
        &expected,
        &structured,
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.finish().await;
}
