//! Clause C.2.3: the `OffStreetParking` Entity in every printed representation.
//!
//! The clause prints eight payloads for the same Entity: normalized, concise
//! and simplified, then each of those again as `GeoJSON`, once for a single
//! Entity and once for a `FeatureCollection`.
//!
//! The printed `GeoJSON` examples disagree with the printed NGSI-LD ones about
//! where Downtown1 is -- the same Entity is drawn at `[-8.5, 41.2]`,
//! `[-8.51, 41.1]` and `[-8.5, 41.1]` across the clause. One Entity has one
//! location, so the corpus uses the `[-8.5, 41.2]` the first example prints.

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

/// Both parking Entities the clause draws, as one batch payload.
const PARKING: &str = include_str!("corpus/c-2-3-parking-seed.json");

/// Seed both parking Entities and answer the wrapper `@context` link.
async fn seed_parking(http: &Client) -> String {
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    create_entity_batch(http, &corpus(PARKING), Some(&link)).await;
    link
}

/// Retrieve Downtown1 with the query string and media type an example implies.
async fn retrieve_downtown_one(http: &Client, link: &str, query: &str, accept: &str) -> Value {
    let (status, body) = get_json(http, &format!("/entities/urn:ngsi-ld:OffStreetParking:Downtown1{query}"), accept, Some(link)).await;
    assert_eq!(status, StatusCode::OK, "OffStreetParking retrieval failed: {body}");
    body
}

/// Query both parking Entities with the query string an example implies.
async fn query_parking(http: &Client, link: &str, query: &str, accept: &str) -> Value {
    let (status, body) = get_json(http, &format!("/entities/?type=OffStreetParking{query}"), accept, Some(link)).await;
    assert_eq!(status, StatusCode::OK, "OffStreetParking query failed: {body}");
    body
}

#[tokio::test]
async fn annex_c_2_3_parking_normalized_representation() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = retrieve_downtown_one(&environment.client(), &link, "", "application/ld+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-normalized.json"));
    assert_matches_example("C.2.3 normalized", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_3_parking_concise_representation() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = retrieve_downtown_one(&environment.client(), &link, "?format=concise", "application/ld+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-concise.json"));
    assert_matches_example("C.2.3 concise", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_3_parking_simplified_representation() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = retrieve_downtown_one(&environment.client(), &link, "?format=simplified", "application/ld+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-simplified.json"));
    assert_matches_example("C.2.3 simplified", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_3_parking_normalized_geojson_representation_of_a_single_entity() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = retrieve_downtown_one(&environment.client(), &link, "", "application/geo+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-geojson-normalized.json"));
    assert_matches_example("C.2.3 normalized GeoJSON", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_3_parking_concise_geojson_representation_of_a_single_entity() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = retrieve_downtown_one(&environment.client(), &link, "?format=concise", "application/geo+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-geojson-concise.json"));
    assert_matches_example("C.2.3 concise GeoJSON", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_3_parking_simplified_geojson_representation_of_a_single_entity() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = retrieve_downtown_one(&environment.client(), &link, "?format=simplified", "application/geo+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-geojson-simplified.json"));
    assert_matches_example("C.2.3 simplified GeoJSON", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_3_parking_normalized_geojson_representation_of_multiple_entities() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = query_parking(&environment.client(), &link, "", "application/geo+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-geojson-normalized-collection.json"));
    assert_matches_example(
        "C.2.3 normalized GeoJSON FeatureCollection",
        &expected,
        &sorted_features(actual),
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_3_parking_concise_geojson_representation_of_multiple_entities() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = query_parking(&environment.client(), &link, "&format=concise", "application/geo+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-geojson-concise-collection.json"));
    assert_matches_example(
        "C.2.3 concise GeoJSON FeatureCollection",
        &expected,
        &sorted_features(actual),
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_2_3_parking_simplified_geojson_representation_of_multiple_entities() {
    let environment = clean_tenant().await;
    let link = seed_parking(&environment.client()).await;

    let actual = query_parking(&environment.client(), &link, "&format=simplified", "application/geo+json").await;
    let expected = corpus(include_str!("corpus/c-2-3-geojson-simplified-collection.json"));
    assert_matches_example(
        "C.2.3 simplified GeoJSON FeatureCollection",
        &expected,
        &sorted_features(actual),
        ContextMember::Ignored,
        EntityOrder::AsPrinted,
    );

    environment.finish().await;
}

/// Order a `FeatureCollection`'s features by `id`.
///
/// The features of a query answer are Entities, which clause 5.7.2 leaves
/// unordered unless `orderBy` was given; the collection wrapper around them is
/// not an array the comparison rule can sort on its own.
fn sorted_features(mut collection: Value) -> Value {
    if let Some(features) = collection.get_mut("features").and_then(Value::as_array_mut) {
        features.sort_by_key(|feature| feature.get("id").and_then(Value::as_str).unwrap_or_default().to_owned());
    }
    collection
}
