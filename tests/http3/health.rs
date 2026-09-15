//! The broker answers over HTTP/3 at all, and reports the identity it was configured with.

use crate::client::{Http3Client, wait_until_ready};
use http::{Method, StatusCode};
use pretty_assertions::assert_eq;

#[tokio::test]
async fn health_is_served_over_http3() {
    wait_until_ready().await;
    let client = Http3Client::connect_trusted().await;
    let response = client.request(Method::GET, "/ngsi-ld/v1/health", &[], &[]).await;
    client.close().await;
    assert_eq!(response.status, StatusCode::OK, "GET /health over HTTP/3 should be 200");
}

#[tokio::test]
async fn a_json_response_body_is_returned_over_http3() {
    wait_until_ready().await;
    let client = Http3Client::connect_trusted().await;
    let response = client
        .request(Method::GET, "/ngsi-ld/v1/info/sourceIdentity", &[("accept", "application/json")], &[])
        .await;
    client.close().await;
    assert_eq!(response.status, StatusCode::OK, "GET /info/sourceIdentity over HTTP/3 should be 200");
    let content_type = response.headers.get("content-type").and_then(|value| value.to_str().ok()).unwrap_or_default();
    assert!(content_type.contains("json"), "the source identity is JSON over HTTP/3: {content_type}");
    // The compose stack configures this alias, so its presence proves the real
    // handler ran and returned its body over HTTP/3, not an empty transport echo.
    assert!(
        response.text().contains("http3-broker"),
        "the source identity document should carry the configured alias: {}",
        response.text()
    );
}
