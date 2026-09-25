//! Clause C.2.1 and C.2.2: the reference property graph and its Vehicle Entity.
//!
//! Every test seeds the Vehicle of figure C.2.1-1 together with the Entities
//! its Relationships target, then retrieves it in the representation the
//! example demonstrates and compares against the printed payload.
//!
//! The seed used by the representation tests is the printed normalized example
//! with the `unitCode` of `tyreTreadDepths` removed, because the broker refuses
//! that member on a `ListProperty`. The refusal has its own test
//! (`annex_c_2_2_vehicle_normalized_representation`), which replays the example
//! exactly as printed so the divergence is named once instead of masking the
//! thirteen representation tests behind it.

use crate::{
    clean_tenant,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    create_entity,
    create_entity_batch,
    example_context::hosted_contexts,
    get_json,
};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};
use serde_json::Value;

/// The Vehicle exactly as clause C.2.2 prints it, `unitCode` included.
const VEHICLE_AS_PRINTED: &str = include_str!("corpus/c-2-2-normalized.json");
/// The Vehicle without the `ListProperty` unit the broker refuses.
const VEHICLE: &str = include_str!("corpus/c-2-2-normalized-unitless.json");
/// The Entities the Vehicle's Relationships target.
const LINKED: &str = include_str!("corpus/c-2-1-linked-entities.json");

/// Seed the reference property graph and answer the wrapper `@context` link.
async fn seed_property_graph(http: &Client) -> String {
    let hosted = hosted_contexts();
    let link = context_link(hosted.url("aggregatedContext.jsonld").as_str());
    create_entity_batch(http, &corpus(LINKED), Some(&link)).await;
    create_entity(http, &corpus(VEHICLE), Some(&link)).await;
    link
}

/// Retrieve the seeded Vehicle with the query string an example implies.
async fn retrieve_vehicle(http: &Client, link: Option<&str>, query: &str) -> Value {
    let (status, body) = get_json(http, &format!("/entities/urn:ngsi-ld:Vehicle:A4567{query}"), "application/ld+json", link).await;
    assert_eq!(status, StatusCode::OK, "Vehicle retrieval failed: {body}");
    body
}

#[tokio::test]
async fn annex_c_2_1_property_graph_reference_example_round_trips() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    // Figure C.2.1-1 is the graph the rest of clause C.2 draws from: a Vehicle
    // whose isParked Relationship reaches an OffStreetParking and whose
    // providedBy sub-Relationship reaches a Person. Retrieving each node and
    // the edge between them is what "the graph is representable" means.
    let vehicle = retrieve_vehicle(&http, Some(&link), "").await;
    assert_eq!(
        vehicle["isParked"]["object"],
        Value::String("urn:ngsi-ld:OffStreetParking:Downtown1".to_owned())
    );
    assert_eq!(vehicle["isParked"]["providedBy"]["object"], Value::String("urn:ngsi-ld:Person:Bob".to_owned()));
    assert_eq!(vehicle["isParked"]["observedAt"], Value::String("2017-07-29T12:00:04Z".to_owned()));

    let (status, parking) = get_json(&http, "/entities/urn:ngsi-ld:OffStreetParking:Downtown1", "application/ld+json", Some(&link)).await;
    assert_eq!(status, StatusCode::OK, "linked OffStreetParking retrieval failed: {parking}");
    assert_eq!(parking["type"], Value::String("OffStreetParking".to_owned()));

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_normalized_representation() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let hosted = hosted_contexts();
    let link = context_link(hosted.url("aggregatedContext.jsonld").as_str());
    let expected = corpus(VEHICLE_AS_PRINTED);

    let response = http
        .post(format!("{}/entities/", crate::BASE_URL))
        .header("Content-Type", "application/json")
        .header("Link", &link)
        .json(&crate::without_context(&expected))
        .send()
        .await
        .expect("entity creation request");
    let status = response.status();
    let refusal = response.text().await.unwrap_or_default();
    assert_eq!(status, StatusCode::CREATED, "the C.2.2 normalized example was refused: {refusal}");

    let actual = retrieve_vehicle(&http, Some(&link), "").await;
    assert_matches_example("C.2.2 normalized", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_normalized_representation_with_inline_linked_entities() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?join=inline").await;
    let expected = corpus(include_str!("corpus/c-2-2-normalized-inline.json"));
    assert_matches_example(
        "C.2.2 normalized inline join",
        &expected,
        &actual,
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_normalized_representation_with_flattened_linked_entities() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?join=flat").await;
    let expected = corpus(include_str!("corpus/c-2-2-normalized-flattened.json"));
    assert_matches_example("C.2.2 normalized flat join", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_normalized_representation_with_language_filter() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?lang=fr").await;
    let expected = corpus(include_str!("corpus/c-2-2-normalized-language-filter.json"));
    assert_matches_example("C.2.2 normalized lang=fr", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_concise_representation() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?format=concise").await;
    let expected = corpus(include_str!("corpus/c-2-2-concise.json"));
    assert_matches_example("C.2.2 concise", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_concise_representation_with_inline_linked_entities() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?format=concise&join=inline").await;
    let expected = corpus(include_str!("corpus/c-2-2-concise-inline.json"));
    assert_matches_example("C.2.2 concise inline join", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_concise_representation_with_flattened_linked_entities() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?format=concise&join=flat").await;
    let expected = corpus(include_str!("corpus/c-2-2-concise-flattened.json"));
    assert_matches_example("C.2.2 concise flat join", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_concise_representation_with_language_filter() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?format=concise&lang=fr").await;
    let expected = corpus(include_str!("corpus/c-2-2-concise-language-filter.json"));
    assert_matches_example("C.2.2 concise lang=fr", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_simplified_representation() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?format=simplified").await;
    let expected = corpus(include_str!("corpus/c-2-2-simplified.json"));
    assert_matches_example("C.2.2 simplified", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_simplified_representation_with_inline_linked_entities() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?format=simplified&join=inline").await;
    let expected = corpus(include_str!("corpus/c-2-2-simplified-inline.json"));
    assert_matches_example(
        "C.2.2 simplified inline join",
        &expected,
        &actual,
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_simplified_representation_with_flattened_linked_entities() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?format=simplified&join=flat").await;
    let expected = corpus(include_str!("corpus/c-2-2-simplified-flattened.json"));
    assert_matches_example("C.2.2 simplified flat join", &expected, &actual, ContextMember::Ignored, EntityOrder::Unordered);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_simplified_representation_of_natural_language_attributes() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let link = seed_property_graph(&http).await;

    let actual = retrieve_vehicle(&http, Some(&link), "?format=simplified&lang=fr").await;
    let expected = corpus(include_str!("corpus/c-2-2-simplified-language-filter.json"));
    assert_matches_example("C.2.2 simplified lang=fr", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_multiple_attribute_instances_of_speed() {
    let environment = clean_tenant().await;
    let http = environment.client();
    let expected = corpus(include_str!("corpus/c-2-2-multi-attribute.json"));
    create_entity(&http, &expected, None).await;
    let multi_link = context_link(hosted_contexts().url("multiAttribute.jsonld").as_str());

    let actual = retrieve_vehicle(&http, Some(&multi_link), "").await;
    assert_matches_example("C.2.2 multi-attribute", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_2_vehicle_simplified_representation_of_a_multiple_attribute() {
    let environment = clean_tenant().await;
    let http = environment.client();
    create_entity(&http, &corpus(include_str!("corpus/c-2-2-multi-attribute.json")), None).await;
    let multi_link = context_link(hosted_contexts().url("multiAttribute.jsonld").as_str());

    let actual = retrieve_vehicle(&http, Some(&multi_link), "?format=simplified").await;
    let expected = corpus(include_str!("corpus/c-2-2-simplified-multi-attribute.json"));
    assert_matches_example(
        "C.2.2 simplified multi-attribute",
        &expected,
        &actual,
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.finish().await;
}
