//! Source identity retrieval tests (NGSI-LD 5.8.1, 5.5.7).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::{client::NgsiLdClient, scope::TestScope};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;

/// Retrieve source identity — returns 200 with ContextSourceIdentity body (5.8.1).
#[tokio::test]
async fn info_source_identity_default_returns_200() {
    let scope = TestScope::new("info");
    let client = NgsiLdClient::new(&scope).await;
    let response = client.source_identity(None).await.expect("source_identity request failed");
    assert_eq!(response.status, StatusCode::OK);
    assert!(response.body["id"].as_str().is_some(), "response body should contain an id field");
    assert_eq!(response.body["type"], "ContextSourceIdentity", "type field should be ContextSourceIdentity");
}

/// `application/json` Accept — Content-Type is application/json, no `@context`
/// in body, `Link` header present (5.8.1, 5.5.7).
#[tokio::test]
async fn info_source_identity_json_accept_returns_no_context_in_body() {
    let scope = TestScope::new("info");
    let client = NgsiLdClient::new(&scope).await;
    let response = client.source_identity(Some("application/json")).await.expect("source_identity request failed");
    assert_eq!(response.status, StatusCode::OK);
    let content_type = response.headers.get("content-type").expect("response should have Content-Type header");
    assert!(
        content_type.to_str().unwrap().contains("application/json"),
        "Content-Type should be application/json, got: {}",
        content_type.to_str().unwrap()
    );
    assert!(
        response.body.get("@context").is_none(),
        "application/json response should not embed @context in body"
    );
    assert!(
        response.headers.get("link").is_some(),
        "application/json response should include a Link header with the context"
    );
}

/// `application/ld+json` Accept — Content-Type is application/ld+json,
/// `@context` embedded in body (5.8.1, 5.5.7).
#[tokio::test]
async fn info_source_identity_ld_json_accept_returns_context_in_body() {
    let scope = TestScope::new("info");
    let client = NgsiLdClient::new(&scope).await;
    let response = client
        .source_identity(Some("application/ld+json"))
        .await
        .expect("source_identity request failed");
    assert_eq!(response.status, StatusCode::OK);
    let content_type = response.headers.get("content-type").expect("response should have Content-Type header");
    assert!(
        content_type.to_str().unwrap().contains("application/ld+json"),
        "Content-Type should be application/ld+json, got: {}",
        content_type.to_str().unwrap()
    );
    assert!(
        response.body.get("@context").is_some(),
        "application/ld+json response should embed @context in body"
    );
}
