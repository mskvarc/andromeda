//! Clause C.9: the Core `@context` is protected and wins every collision.
//!
//! JSON-LD processes an `@context` array in order and lets a later definition
//! replace an earlier one. NGSI-LD overrides that for the Core `@context`: it
//! is protected, so it takes final precedence wherever it appears in the array,
//! and implementations render it in the last position.
//!
//! The clause's collision is `location`, defined both by the Core `@context`
//! and by schema.org. Only that one term and the `schema:` prefix matter to
//! what is demonstrated, so the suite hosts a stand-in for schema.org rather
//! than the whole vocabulary; the substitution is recorded in
//! `docs/annex-c-corpus.md`.

use crate::{
    BASE_URL,
    clean_tenant,
    client,
    comparison::corpus,
    context_link,
    context_utilization::flattened_context,
    example_context::hosted_contexts,
    get_json,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;

/// The IRI schema.org gives `location`, which the Core definition overrides.
const SCHEMA_LOCATION: &str = "https://schema.org/location";
/// The Core `@context` IRI the broker renders.
///
/// Clause 4.4 names the Core `@context` of this edition
/// `https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld`, and every
/// annex C example prints that; the broker renders that exact versioned IRI.
const RENDERED_CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// Create one Entity under a hosted `@context` and answer the outcome.
async fn create_under(context_document: &str, entity: &Value) -> (StatusCode, String) {
    let link = context_link(hosted_contexts().url(context_document).as_str());
    let response = client()
        .post(format!("{BASE_URL}/entities/"))
        .header("Content-Type", "application/json")
        .header("Link", link)
        .json(entity)
        .send()
        .await
        .expect("entity creation request");
    let status = response.status();
    (status, response.text().await.unwrap_or_default())
}

/// Retrieve the Building with no supplied `@context`, so every user term shows
/// as the IRI it actually expanded to.
async fn retrieve_expanded() -> Value {
    let (status, body) = get_json(&client(), "/entities/urn:ngsi-ld:Building:B1", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {body}");
    body
}

#[tokio::test]
#[serial]
async fn annex_c_9_a_colliding_term_expands_through_the_core_context() {
    let environment = clean_tenant().await;
    let building = corpus(include_str!("corpus/c-9-colliding-building.json"));

    let (status, body) = create_under("colliding-context.jsonld", &building).await;
    assert_eq!(status, StatusCode::CREATED, "the C.9 colliding example was refused: {body}");

    // The answer is compacted against the Core @context alone, so an Attribute
    // that expanded to the Core `location` IRI comes back as the short name
    // `location`, while one that expanded to the schema.org IRI comes back as
    // that IRI -- exactly as `name` does here.
    let actual = retrieve_expanded().await;
    assert!(
        actual.get("location").is_some(),
        "clause C.9: the Core @context takes precedence for `location`, got {actual}"
    );
    assert!(
        actual.get(SCHEMA_LOCATION).is_none(),
        "clause C.9: the schema.org `location` must not win the collision, got {actual}"
    );
    assert!(
        actual.get("https://schema.org/name").is_some(),
        "the stand-in vocabulary is in force for terms the Core @context does not define, got {actual}"
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

#[tokio::test]
#[serial]
async fn annex_c_9_the_core_context_wins_from_any_position_in_the_array() {
    let environment = clean_tenant().await;
    let building = corpus(include_str!("corpus/c-9-colliding-building.json"));

    // The same collision with the Core @context placed first, where the plain
    // JSON-LD rule would let schema.org replace it.
    let (status, body) = create_under("colliding-context-core-first.jsonld", &building).await;
    assert_eq!(status, StatusCode::CREATED, "the C.9 core-first example was refused: {body}");

    let actual = retrieve_expanded().await;
    assert!(
        actual.get("location").is_some(),
        "clause C.9: the Core @context is protected regardless of array position, got {actual}"
    );
    assert!(
        actual.get(SCHEMA_LOCATION).is_none(),
        "clause C.9: a Core @context placed first still wins the collision, got {actual}"
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

#[tokio::test]
#[serial]
async fn annex_c_9_a_prefixed_term_reaches_the_user_definition_the_collision_hid() {
    let environment = clean_tenant().await;
    let building = corpus(include_str!("corpus/c-9-prefixed-building.json"));

    // "the solution is to prefix the conflicting terms, so that there cannot be
    // any clashing".
    let (status, body) = create_under("colliding-context.jsonld", &building).await;
    assert_eq!(status, StatusCode::CREATED, "the C.9 prefixed example was refused: {body}");

    let actual = retrieve_expanded().await;
    assert!(
        actual.get(SCHEMA_LOCATION).is_some(),
        "clause C.9: `schema:location` has to reach https://schema.org/location, got {actual}"
    );
    assert_eq!(
        actual[SCHEMA_LOCATION],
        json!({"type": "Property", "value": "20 West 34th Street, New York City, NY 10001"})
    );

    environment.reset_and_finish().await.expect("tenant reset");
}

#[tokio::test]
#[serial]
async fn annex_c_9_the_rendered_context_array_ends_with_the_core_context() {
    let environment = clean_tenant().await;
    let building = corpus(include_str!("corpus/c-9-colliding-building.json"));
    let (status, body) = create_under("colliding-context.jsonld", &building).await;
    assert_eq!(status, StatusCode::CREATED, "the C.9 colliding example was refused: {body}");

    // "NGSI-LD implementations are required to render content following this
    // approach" -- the Core @context last, for compatibility with off-the-shelf
    // JSON-LD tooling.
    let actual = retrieve_expanded().await;
    let rendered = flattened_context(&actual);
    assert_eq!(
        rendered.last().map(String::as_str),
        Some(RENDERED_CORE_CONTEXT),
        "clause C.9 puts the Core @context in the final position, got {:?}",
        actual["@context"]
    );

    environment.reset_and_finish().await.expect("tenant reset");
}
