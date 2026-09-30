//! Clause C.7: which `@context` decides how a stored Entity is rendered.
//!
//! The clause creates an `OffStreetParking` under one vocabulary and then
//! retrieves it twice: once under a vocabulary that gives the same IRIs
//! different short names, and once under none at all. What it demonstrates is
//! that expansion and compaction always use the `@context` of the current
//! request, never the one the Entity was written with.
//!
//! The printed answer for the abbreviated retrieval renders `name` as a short
//! name although the abbreviated vocabulary defines no `name` term. The corpus
//! prints the Fully Qualified Name the clause's own rule produces, and the
//! divergence is recorded in `docs/annex-c-corpus.md`.

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

/// Create the `OffStreetParking` under the vocabulary clause C.7 writes it with.
async fn create_under_the_parking_vocabulary(http: &Client) {
    let link = context_link(hosted_contexts().url("parking-c7.jsonld").as_str());
    let mut entity = corpus(include_str!("corpus/c-7-created-entity.json"));
    // The Entity is written with the vocabulary in the Link header rather than
    // inline, so the payload's own printed `@context` is dropped.
    if let Some(members) = entity.as_object_mut() {
        members.remove("@context");
    }
    create_entity(http, &entity, Some(&link)).await;
}

/// Retrieve the created Entity under whatever `@context` the caller names.
async fn retrieve(http: &Client, link: Option<&str>) -> Value {
    let (status, body) = get_json(http, "/entities/urn:ngsi-ld:OffStreetParking:Downtown1", "application/ld+json", link).await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {body}");
    body
}

#[tokio::test]
async fn annex_c_7_retrieval_renders_terms_from_the_retrieval_time_context() {
    let environment = clean_tenant().await;
    create_under_the_parking_vocabulary(&environment.client()).await;

    let abbreviated = context_link(hosted_contexts().url("parking-abbreviated.jsonld").as_str());
    let actual = retrieve(&environment.client(), Some(&abbreviated)).await;
    let expected = corpus(include_str!("corpus/c-7-abbreviated-answer.json"));
    assert_matches_example("C.7 abbreviated retrieval", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_7_the_response_context_array_reflects_the_header_supplied_context() {
    let environment = clean_tenant().await;
    create_under_the_parking_vocabulary(&environment.client()).await;

    let abbreviated_url = hosted_contexts().url("parking-abbreviated.jsonld").to_string();
    let actual = retrieve(&environment.client(), Some(&context_link(&abbreviated_url))).await;

    // C.7 prints the header-supplied @context followed by the Core @context,
    // which the abbreviated vocabulary does not import (clause 5.2.3).
    assert_eq!(
        flattened_context(&actual),
        vec![abbreviated_url, "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned()],
        "clause C.7: the response @context carries the header-supplied @context and the Core @context last, got {:?}",
        actual["@context"]
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_7_an_unmatched_context_renders_user_names_as_fully_qualified_names() {
    let environment = clean_tenant().await;
    create_under_the_parking_vocabulary(&environment.client()).await;

    let actual = retrieve(&environment.client(), None).await;
    let expected = corpus(include_str!("corpus/c-7-unmatched-answer.json"));
    assert_matches_example("C.7 no supplied @context", &expected, &actual, ContextMember::Ignored, EntityOrder::AsPrinted);

    // "the core API terms have been compacted, as the Core @context is always
    // considered to be implicitly present": `type` and `value` are core terms
    // and stay short even with no supplied @context.
    assert_eq!(
        actual["http://example.org/parking/totalSpotNumber"],
        json!({"type": "Property", "value": 200}),
        "clause C.7 keeps the core API terms compacted, got {}",
        actual["http://example.org/parking/totalSpotNumber"]
    );
    // Clause 4.4 names the Core @context of this edition
    // `...ngsi-ld-core-context-v1.9.jsonld` and clause C.7 prints that; the
    // broker renders that exact versioned IRI.
    assert_eq!(
        flattened_context(&actual),
        vec!["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned()],
        "clause C.7 renders the Core @context in the answer even when none was supplied"
    );

    environment.finish().await;
}

/// The rendered `@context` as a flat list of entries.
///
/// A broker may render one string, an array of strings, or an array mixing
/// strings and inline objects; the clause's statements are about which URLs are
/// present, so the shape is flattened before they are checked.
pub fn flattened_context(payload: &Value) -> Vec<String> {
    match &payload["@context"] {
        Value::String(single) => vec![single.clone()], // clone: the caller owns the flattened list.
        Value::Array(entries) => entries.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => Vec::new(),
    }
}
