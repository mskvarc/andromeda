//! Clause 5.6.7's batch create must expand terms exactly as clause 5.6.1 does.
//!
//! A `@context` may define a term by a compact IRI whose prefix the same
//! `@context` defines -- `{"example": "http://example.org/", "Place":
//! "example:Place"}` -- and JSON-LD 1.1 clause 4.1.2 resolves that while the
//! context is being processed. Clause 5.5.7 hands both write paths the same
//! `@context`, so both have to reach the same expanded IRIs.
//!
//! Found while replaying annex C clause C.10, whose `Place` ontology is written
//! that way: the batch path stored the Entity under the literal string
//! `example:Place`, and a later request phrased in the terms that wrote it then
//! answered `404 Not Found` for an Entity the broker had acknowledged creating.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

use crate::hosted_context::{context_link, forget_cached, serve};
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::http::HeaderMap;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::slice::from_ref;

/// A `@context` whose term definitions are written as compact IRIs.
fn prefixed_context() -> Value {
    json!({"@context": {
        "example": "http://example.org/",
        "Place": "example:Place",
        "address": "example:address"
    }})
}

/// The expanded IRI `Place` has to reach through that `@context`.
const EXPANDED_TYPE: &str = "http://example.org/Place";
/// The expanded IRI `address` has to reach through that `@context`.
const EXPANDED_ATTRIBUTE: &str = "http://example.org/address";

/// One Place Entity, named after the write path that will create it.
fn place(identifier: &str) -> Value {
    json!({
        "id": identifier,
        "type": "Place",
        "address": {"type": "Property", "value": "Pariser Platz, Berlin, Germany"}
    })
}

/// Serve the prefixed `@context` under a path unique to this run.
async fn serve_prefixed_context(server: &JsonLdContextServer) -> String {
    let (_, url) = serve(server, "batch-context-expansion", &prefixed_context().to_string(), HeaderMap::new()).await;
    url
}

#[tokio::test]
async fn regression_batch_context_expansion_resolves_prefixed_terms_like_the_single_write_path() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server");
    let context_url = serve_prefixed_context(&server).await;
    let link = context_link(&context_url);
    let client = scope.client();

    let single = scope.id("Place:single");
    let created = client
        .raw(Method::POST, "entities/")
        .header("content-type", "application/json")
        .header("link", &link)
        .json(&place(&single))
        .send()
        .await
        .expect("single entity creation request");
    assert_eq!(created.status, StatusCode::CREATED, "single creation must succeed: {:?}", created.body);

    let batched = scope.id("Place:batched");
    let created = client
        .raw(Method::POST, "entityOperations/create")
        .header("content-type", "application/json")
        .header("link", &link)
        .json(&json!([place(&batched)]))
        .send()
        .await
        .expect("batch creation request");
    assert_eq!(created.status, StatusCode::CREATED, "batch creation must succeed: {:?}", created.body);

    // Retrieved with no `@context` at all, so every user term shows as the IRI
    // it actually expanded to.
    let single_entity = client
        .get_entity(&single, "", "application/json", None)
        .await
        .expect("single entity retrieval")
        .json_value()
        .expect("an Entity body");
    assert_eq!(
        single_entity["type"],
        json!(EXPANDED_TYPE),
        "clause 5.6.1 resolves the compact IRI in the term definition"
    );

    let batched_entity = client
        .get_entity(&batched, "", "application/json", None)
        .await
        .expect("batch entity retrieval")
        .json_value()
        .expect("an Entity body");
    assert_eq!(
        batched_entity["type"],
        json!(EXPANDED_TYPE),
        "clause 5.6.7 has to resolve the same compact IRI clause 5.6.1 resolves, got {batched_entity}"
    );
    assert!(
        batched_entity.get(EXPANDED_ATTRIBUTE).is_some(),
        "the batch-created Attribute has to carry the same expanded name, got {batched_entity}"
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown");
    scope.finish().await.expect("environment cleanup");
}

#[tokio::test]
async fn regression_batch_context_expansion_leaves_the_entity_reachable_under_its_own_context() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("callback host"))
        .await
        .expect("context server");
    let context_url = serve_prefixed_context(&server).await;
    let link = context_link(&context_url);
    let client = scope.client();

    let identifier = scope.id("Place:reachable");
    let created = client
        .raw(Method::POST, "entityOperations/create")
        .header("content-type", "application/json")
        .header("link", &link)
        .json(&json!([place(&identifier)]))
        .send()
        .await
        .expect("batch creation request");
    assert_eq!(created.status, StatusCode::CREATED, "batch creation must succeed: {:?}", created.body);

    // Clause C.7: the `@context` a client reads an Entity back with is the one
    // it names on the retrieval. An Entity the broker acknowledged creating and
    // then cannot find under the very `@context` that wrote it has been lost,
    // not filtered.
    let retrieved = client
        .raw(Method::GET, &format!("entities/{identifier}"))
        .header("accept", "application/json")
        .header("link", &link)
        .send()
        .await
        .expect("entity retrieval request");
    assert_eq!(
        retrieved.status,
        StatusCode::OK,
        "the created Entity has to be retrievable under the @context that wrote it: {:?}",
        retrieved.body
    );

    let matched = client
        .raw(Method::GET, "entities/?type=Place")
        .header("accept", "application/json")
        .header("link", &link)
        .send()
        .await
        .expect("entity query request")
        .json_value()
        .expect("a query answer body");
    let identifiers: Vec<&str> = matched
        .as_array()
        .map(|entities| entities.iter().filter_map(|entity| entity["id"].as_str()).collect())
        .unwrap_or_default();
    assert!(
        identifiers.contains(&identifier.as_str()),
        "the created Entity has to answer a query in the terms it was written with, got {matched}"
    );

    forget_cached(scope.database_pool(), from_ref(&context_url)).await;
    server.shutdown().await.expect("context server shutdown");
    scope.finish().await.expect("environment cleanup");
}
