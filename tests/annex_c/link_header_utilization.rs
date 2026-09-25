//! Clause C.8: conveying a compound `@context` through one HTTP `Link` header.
//!
//! JSON-LD admits exactly one `http://www.w3.org/ns/json-ld#context` link, so a
//! client that needs terms from several documents hosts a wrapper that
//! references them all and names only that wrapper. The clause then shows the
//! broker echoing the wrapper back, and notes that including the Core
//! `@context` in the wrapper is good practice rather than a requirement.

use crate::{
    BASE_URL,
    clean_tenant,
    comparison::corpus,
    context_link,
    context_utilization::flattened_context,
    example_context::hosted_contexts,
    get_json,
    get_with_headers,
};
use pretty_assertions::assert_eq;
use reqwest::{Client, StatusCode};
use serde_json::json;

/// The Vehicle clause C.8 creates through its wrapper `@context`.
const VEHICLE: &str = include_str!("corpus/c-8-vehicle.json");

/// Create the Vehicle with `application/json` and the wrapper in a `Link`.
async fn create_through_wrapper(http: &Client, wrapper_url: &str) -> (StatusCode, String) {
    let response = http
        .post(format!("{BASE_URL}/entities/"))
        .header("Content-Type", "application/json")
        .header("Link", context_link(wrapper_url))
        .json(&corpus(VEHICLE))
        .send()
        .await
        .expect("entity creation request");
    let status = response.status();
    (status, response.text().await.unwrap_or_default())
}

#[tokio::test]
async fn annex_c_8_a_single_wrapper_context_link_conveys_a_compound_context() {
    let environment = clean_tenant().await;
    let wrapper = hosted_contexts().url("wrapper-context.jsonld").to_string();

    let (status, body) = create_through_wrapper(&environment.client(), &wrapper).await;
    assert_eq!(status, StatusCode::CREATED, "the C.8 creation through a wrapper was refused: {body}");

    // Both halves of the wrapper are in force: `builtYear` comes from the
    // vehicle vocabulary, and the Entity is retrievable under the terms it
    // defines.
    let (status, actual) = get_json(
        &environment.client(),
        "/entities/urn:ngsi-ld:Vehicle:V1",
        "application/ld+json",
        Some(&context_link(&wrapper)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {actual}");
    assert_eq!(actual["builtYear"], json!({"type": "Property", "value": "2014"}));
    assert_eq!(actual["speed"], json!({"type": "Property", "value": 121, "observedAt": "2017-07-29T12:05:02Z"}));

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_8_the_broker_echoes_the_wrapper_context_it_was_given() {
    let environment = clean_tenant().await;
    let wrapper = hosted_contexts().url("wrapper-context.jsonld").to_string();
    let (status, body) = create_through_wrapper(&environment.client(), &wrapper).await;
    assert_eq!(status, StatusCode::CREATED, "the C.8 creation through a wrapper was refused: {body}");

    let (status, headers, actual) = get_with_headers(
        &environment.client(),
        "/entities/urn:ngsi-ld:Vehicle:V1",
        "application/ld+json",
        Some(&context_link(&wrapper)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {actual}");

    // The clause allows either carrier: the Link header of the response, or the
    // @context of the JSON-LD body. One of them has to name the wrapper.
    let in_body = flattened_context(&actual).contains(&wrapper);
    let in_header = headers
        .get_all("Link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .any(|value| value.contains(&wrapper));
    assert!(
        in_body || in_header,
        "clause C.8: the broker answers with the same wrapper @context, got body {:?} and headers {headers:?}",
        actual["@context"]
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_8_a_wrapper_without_the_core_context_is_still_processed() {
    let environment = clean_tenant().await;
    let bare = hosted_contexts().url("wrapper-context-without-core.jsonld").to_string();

    // "it should be noted this is not necessary for NGSI-LD, as the Core
    // @context is always implicitly included."
    let (status, body) = create_through_wrapper(&environment.client(), &bare).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "clause C.8: a wrapper without the Core @context still has to work, got {body}"
    );

    let (status, actual) = get_json(
        &environment.client(),
        "/entities/urn:ngsi-ld:Vehicle:V1",
        "application/ld+json",
        Some(&context_link(&bare)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {actual}");
    assert_eq!(
        actual["builtYear"],
        json!({"type": "Property", "value": "2014"}),
        "the core terms `type` and `value` are implicit, got {actual}"
    );

    environment.finish().await;
}

#[tokio::test]
async fn annex_c_8_one_wrapper_link_is_all_a_compound_context_needs() {
    let environment = clean_tenant().await;
    let wrapper = hosted_contexts().url("wrapper-context.jsonld").to_string();

    // "only one HTTP Link header with the link relationship
    // <http://www.w3.org/ns/json-ld#context> is required to appear": the same
    // payload that carries terms from two documents is accepted with the one
    // wrapper Link, and without it those terms fall through to the default
    // `@context` of clause 5.5.7 instead.
    let (status, body) = create_through_wrapper(&environment.client(), &wrapper).await;
    assert_eq!(status, StatusCode::CREATED, "the C.8 creation through a wrapper was refused: {body}");
    let (status, retrieved) = get_json(&environment.client(), "/entities/urn:ngsi-ld:Vehicle:V1", "application/ld+json", None).await;
    assert_eq!(status, StatusCode::OK, "retrieval failed: {retrieved}");
    assert!(
        retrieved.get("http://example.org/vehicle/builtYear").is_some(),
        "the wrapper's vehicle vocabulary decided how builtYear expanded, got {retrieved}"
    );

    environment.finish().await;
}
