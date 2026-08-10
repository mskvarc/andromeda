#![allow(clippy::expect_used, clippy::unwrap_used)]
//! Consumption: List @contexts tests (NGSI-LD 6.25.3.3)

use reqwest::StatusCode;
use serial_test::serial;
use std::collections::HashMap;

use crate::{
    BAD_REQUEST_DATA_TYPE,
    ContextServer,
    clean_database,
    client,
    create_context,
    create_entity,
    create_subscription,
    delete_subscription,
    extract_id,
    list_contexts,
    unique_id,
};
use serde_json::{Value, json};

/// Assert listing contains no non-core context URIs.
fn assert_no_non_core(contexts: &[Value]) {
    let non_core: Vec<_> = contexts.iter().filter(|v| v.as_str().is_none_or(|s| !s.contains("core-context"))).collect();
    assert!(non_core.is_empty(), "expected no non-core contexts, got {non_core:?}");
}

/// Assert listing contains a URI ending with the given string.
fn assert_contains(contexts: &[Value], uri: &str) {
    assert!(
        contexts.iter().any(|v| v.as_str().is_some_and(|s| s.ends_with(uri))),
        "should contain context URI {uri}"
    );
}

/// Assert listing contains all given URIs.
fn assert_contains_all(contexts: &[Value], uris: &[String]) {
    for uri in uris {
        assert_contains(contexts, uri);
    }
}

/// Create one hosted context and return its URI.
async fn create_one_context(c: &reqwest::Client) -> String {
    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });
    let (status, location, _) = create_context(c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    location.unwrap()
}

/// Create 3 hosted contexts and return their URIs.
async fn create_three_contexts(c: &reqwest::Client) -> Vec<String> {
    let payloads = vec![
        json!({"@context": {"A1": "urn:ngsi-ld:attributes:A1", "A2": "urn:ngsi-ld:attributes:A2", "A3": "urn:ngsi-ld:attributes:A3"}}),
        json!({"@context": {"B1": "urn:ngsi-ld:attributes:B1", "B2": "urn:ngsi-ld:attributes:B2", "B3": "urn:ngsi-ld:attributes:B3"}}),
        json!({"@context": {"C1": "urn:ngsi-ld:attributes:C1", "C2": "urn:ngsi-ld:attributes:C2", "C3": "urn:ngsi-ld:attributes:C3"}}),
    ];
    let mut uris = Vec::new();
    for p in &payloads {
        let (status, location, _) = create_context(c, p).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
        uris.push(location.unwrap());
    }
    uris
}

/// Create a subscription that generates an ImplicitlyCreated context.
async fn create_implicit_context_for_list(c: &reqwest::Client) -> String {
    let sub = json!({
        "type": "Subscription",
        "entities": [{"type": "T1"}],
        "notification": {"endpoint": {"uri": "http://a.b.c/123"}},
        "@context": [
            "https://fiware.github.io/NGSI-LD_TestSuite/ldContext/testContext.jsonld",
            "http://smartdatamodels.org/context.jsonld"
        ]
    });
    let (status, location) = create_subscription(c, &sub).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    extract_id(location.as_ref().unwrap())
}

// ── 052_01: List @contexts with no previous creation ──────────────

#[tokio::test]
#[serial]
async fn list_no_contexts_no_params() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (status, contexts) = list_contexts(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
}

#[tokio::test]
#[serial]
async fn list_no_contexts_kind_hosted() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (status, contexts) = list_contexts(&c, "kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
}

#[tokio::test]
#[serial]
async fn list_no_contexts_kind_cached() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (status, contexts) = list_contexts(&c, "kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
}

#[tokio::test]
#[serial]
async fn list_no_contexts_kind_implicitly_created() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (status, contexts) = list_contexts(&c, "kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
}

#[tokio::test]
#[serial]
async fn list_no_contexts_details_false() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (status, contexts) = list_contexts(&c, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
}

#[tokio::test]
#[serial]
async fn list_no_contexts_details_false_kind_hosted() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (status, contexts) = list_contexts(&c, "details=false&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
}

#[tokio::test]
#[serial]
async fn list_no_contexts_details_false_kind_cached() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (status, contexts) = list_contexts(&c, "details=false&kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
}

#[tokio::test]
#[serial]
async fn list_no_contexts_details_false_kind_implicitly_created() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let (status, contexts) = list_contexts(&c, "details=false&kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_no_non_core(&contexts);
}

// ── 052_02: List with one created context ────────────────────────��

#[tokio::test]
#[serial]
async fn list_one_context_no_params() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let uri = create_one_context(&c).await;
    let (status, contexts) = list_contexts(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains(&contexts, &uri);
}

#[tokio::test]
#[serial]
async fn list_one_context_kind_hosted() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let uri = create_one_context(&c).await;
    let (status, contexts) = list_contexts(&c, "kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains(&contexts, &uri);
}

#[tokio::test]
#[serial]
async fn list_one_context_details_false() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let uri = create_one_context(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains(&contexts, &uri);
}

#[tokio::test]
#[serial]
async fn list_one_context_details_false_kind_hosted() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let uri = create_one_context(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains(&contexts, &uri);
}

// ── 052_03: List with multiple created contexts ───────────────────

#[tokio::test]
#[serial]
async fn list_multiple_contexts_no_params() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let uris = create_three_contexts(&c).await;
    let (status, contexts) = list_contexts(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains_all(&contexts, &uris);
}

#[tokio::test]
#[serial]
async fn list_multiple_contexts_kind_hosted() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let uris = create_three_contexts(&c).await;
    let (status, contexts) = list_contexts(&c, "kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains_all(&contexts, &uris);
}

#[tokio::test]
#[serial]
async fn list_multiple_contexts_details_false() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let uris = create_three_contexts(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains_all(&contexts, &uris);
}

#[tokio::test]
#[serial]
async fn list_multiple_contexts_details_false_kind_hosted() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let uris = create_three_contexts(&c).await;
    let (status, contexts) = list_contexts(&c, "details=false&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert_contains_all(&contexts, &uris);
}

// ── 052_04: Invalid params -> 400 ────────────────────────────────

#[tokio::test]
async fn list_invalid_kind_returns_400() {
    let c = client();
    let resp = c.get(crate::broker_url("jsonldContexts?kind=other")).send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[tokio::test]
async fn list_invalid_details_returns_400() {
    let c = client();
    let resp = c.get(crate::broker_url("jsonldContexts?details=other")).send().await.expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[tokio::test]
async fn list_invalid_kind_with_details_true_returns_400() {
    let c = client();
    let resp = c
        .get(crate::broker_url("jsonldContexts?details=true&kind=other"))
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[tokio::test]
async fn list_invalid_details_with_kind_hosted_returns_400() {
    let c = client();
    let resp = c
        .get(crate::broker_url("jsonldContexts?details=other&kind=Hosted"))
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[tokio::test]
async fn list_invalid_details_and_kind_returns_400() {
    let c = client();
    let resp = c
        .get(crate::broker_url("jsonldContexts?details=other&kind=other"))
        .send()
        .await
        .expect("request failed");
    let status = resp.status();
    let body: Value = resp.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

// ── 052_05: details=true ──────────────────────────────────────────

#[tokio::test]
#[serial]
async fn list_details_true_no_kind() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });
    let (status, _, _) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, contexts) = list_contexts(&c, "details=true").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let hosted = contexts.iter().find(|c| c.get("kind").and_then(|k| k.as_str()) == Some("Hosted"));
    assert!(hosted.is_some(), "should have a Hosted context in detailed list");

    let entry = hosted.unwrap();
    assert!(entry.get("URL").is_some(), "should have URL");
    assert!(entry.get("kind").is_some(), "should have kind");
    assert!(entry.get("createdAt").is_some(), "should have createdAt");
}

#[tokio::test]
#[serial]
async fn list_details_true_kind_hosted() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let body = json!({
        "@context": {
            "A1": "urn:ngsi-ld:attributes:A1",
            "A2": "urn:ngsi-ld:attributes:A2",
            "A3": "urn:ngsi-ld:attributes:A3"
        }
    });
    let (status, _, _) = create_context(&c, &body).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, contexts) = list_contexts(&c, "details=true&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let hosted = contexts.iter().find(|c| c.get("kind").and_then(|k| k.as_str()) == Some("Hosted"));
    assert!(hosted.is_some(), "should have a Hosted context");

    let entry = hosted.unwrap();
    assert!(entry.get("URL").is_some());
    assert!(entry.get("kind").is_some());
    assert!(entry.get("createdAt").is_some());
}

// ── 052_06: ImplicitlyCreated contexts ────────────────────────────

#[tokio::test]
#[serial]
async fn list_implicit_no_params() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        contexts.iter().any(|v| v.as_str().is_none_or(|s| !s.contains("core-context"))),
        "should have non-core contexts (implicit)"
    );

    delete_subscription(&c, &sub_id).await;
}

#[tokio::test]
#[serial]
async fn list_implicit_kind_hosted_is_empty() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(contexts.is_empty(), "Hosted filter should not include implicit contexts");

    delete_subscription(&c, &sub_id).await;
}

#[tokio::test]
#[serial]
async fn list_implicit_kind_cached() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, _contexts) = list_contexts(&c, "kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    // External @context URIs may get cached -- just verify valid response

    delete_subscription(&c, &sub_id).await;
}

#[tokio::test]
#[serial]
async fn list_implicit_kind_implicitly_created() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(!contexts.is_empty(), "should have ImplicitlyCreated contexts");

    delete_subscription(&c, &sub_id).await;
}

#[tokio::test]
#[serial]
async fn list_implicit_details_false() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "details=false").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(
        contexts.iter().any(|v| v.as_str().is_none_or(|s| !s.contains("core-context"))),
        "should have non-core contexts (implicit)"
    );

    delete_subscription(&c, &sub_id).await;
}

#[tokio::test]
#[serial]
async fn list_implicit_details_false_kind_hosted_is_empty() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "details=false&kind=Hosted").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(contexts.is_empty(), "Hosted filter should not include implicit contexts");

    delete_subscription(&c, &sub_id).await;
}

#[tokio::test]
#[serial]
async fn list_implicit_details_false_kind_cached() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, _contexts) = list_contexts(&c, "details=false&kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    // External @context URIs may get cached -- just verify valid response

    delete_subscription(&c, &sub_id).await;
}

#[tokio::test]
#[serial]
async fn list_implicit_details_false_kind_implicitly_created() {
    let _serialized_environment = clean_database().await;
    let c = client();
    let sub_id = create_implicit_context_for_list(&c).await;

    let (status, contexts) = list_contexts(&c, "details=false&kind=ImplicitlyCreated").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    assert!(!contexts.is_empty(), "should have ImplicitlyCreated contexts");

    delete_subscription(&c, &sub_id).await;
}

// ── 052_07: List cached contexts with details=true ────────────────

#[tokio::test]
#[serial]
async fn list_cached_contexts_with_details() {
    let _serialized_environment = clean_database().await;
    let c = client();

    let ctx_content = r#"{"@context":{"X1":"urn:ngsi-ld:attributes:X1"}}"#;
    let server = ContextServer::start(HashMap::from([("/context.jsonld".to_string(), ctx_content.to_string())])).await;
    let ctx_url = server.url("/context.jsonld");

    // Create entity to trigger caching
    let entity_id = unique_id("urn:ngsi-ld:Testing:list-cached");
    let entity = json!({
        "id": &entity_id,
        "type": "Testing",
        "X1": {"type": "Property", "value": "test"},
        "@context": [&ctx_url]
    });
    let (status, _) = create_entity(&c, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(1)).await;

    // List with details=true and kind=Cached
    let (status, contexts) = list_contexts(&c, "details=true&kind=Cached").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let cached = contexts.iter().find(|c| c.get("URL").and_then(|u| u.as_str()).is_some_and(|u| u == ctx_url));
    assert!(cached.is_some(), "should find cached context in detailed list");

    let entry = cached.unwrap();
    pretty_assertions::assert_eq!(entry["kind"], "Cached");
    assert!(entry.get("URL").is_some());
    assert!(entry.get("createdAt").is_some());

    crate::delete_entity(&c, &entity_id).await;
    let _ = server;
}
