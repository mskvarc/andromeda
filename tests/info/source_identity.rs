//! Source identity retrieval tests (NGSI-LD 5.8.1, 5.5.7).

use crate::{client, retrieve_source_identity};
use reqwest::StatusCode;

// ---------------------------------------------------------------------------
// 5.8.1 -- Basic retrieval
// ---------------------------------------------------------------------------

/// Retrieve source identity -- returns 200 with ContextSourceIdentity body (5.8.1).
#[tokio::test]
async fn retrieve_source_identity_returns_200_with_identity() {
    let c = client();
    let (status, _, body) = retrieve_source_identity(&c, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body["id"].as_str().is_some(), "response body should contain an id field");
    assert_eq!(body["type"], "ContextSourceIdentity", "type field should be ContextSourceIdentity");
}

// ---------------------------------------------------------------------------
// 5.8.1, 5.5.7 -- Content negotiation: application/json
// ---------------------------------------------------------------------------

/// Retrieve with application/json Accept -- content-type is application/json, no @context in body, Link header present (5.8.1, 5.5.7).
#[tokio::test]
async fn retrieve_source_identity_with_json_accept_returns_no_context_in_body() {
    let c = client();
    let (status, headers, body) = retrieve_source_identity(&c, Some("application/json")).await;
    assert_eq!(status, StatusCode::OK);

    let ct = headers.get("content-type").expect("response should have Content-Type header");
    assert!(
        ct.to_str().unwrap().contains("application/json"),
        "Content-Type should be application/json, got: {}",
        ct.to_str().unwrap()
    );
    assert!(body.get("@context").is_none(), "application/json response should not embed @context in body");
    assert!(
        headers.get("link").is_some(),
        "application/json response should include a Link header with the context"
    );
}

// ---------------------------------------------------------------------------
// 5.8.1, 5.5.7 -- Content negotiation: application/ld+json
// ---------------------------------------------------------------------------

/// Retrieve with application/ld+json Accept -- content-type is application/ld+json, @context embedded in body (5.8.1, 5.5.7).
#[tokio::test]
async fn retrieve_source_identity_with_ld_json_accept_returns_context_in_body() {
    let c = client();
    let (status, headers, body) = retrieve_source_identity(&c, Some("application/ld+json")).await;
    assert_eq!(status, StatusCode::OK);

    let ct = headers.get("content-type").expect("response should have Content-Type header");
    assert!(
        ct.to_str().unwrap().contains("application/ld+json"),
        "Content-Type should be application/ld+json, got: {}",
        ct.to_str().unwrap()
    );
    assert!(body.get("@context").is_some(), "application/ld+json response should embed @context in body");
}
