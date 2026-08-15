//! Remote `@context` cache-control integration tests (NGSI-LD 6.3.16).

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use andromeda_test_mocks::{callback_path::CallbackPath, jsonld_context_server::JsonLdContextServer, server_config::MockServerConfig};
use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue, header::CACHE_CONTROL},
};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use uuid::Uuid;

#[tokio::test]
async fn cache_expiration_max_age_zero_refetches_the_remote_context() {
    let environment = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = environment.client().clone(); // clone: the client remains usable while the environment retains cleanup ownership.
    let server = JsonLdContextServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context server must start");
    let path = CallbackPath::parse(format!("/cache-expiration-{}.jsonld", Uuid::new_v4())).expect("absolute context path");
    let mut headers = HeaderMap::new();
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("max-age=0"));
    let context_url = server
        .serve(
            path.clone(), // clone: the server retains the route while the test uses it for request counts.
            Bytes::from_static(br#"{"@context":{"Device":"https://example.org/Device","temperature":"https://example.org/temperature"}}"#),
            headers,
        )
        .await
        .expect("context route must be configured");

    for sequence in 1..=2 {
        let entity = json!({
            "id": format!("urn:ngsi-ld:Device:cache-expiration:{sequence}:{}", Uuid::new_v4()),
            "type": "Device",
            "temperature": {"type": "Property", "value": sequence},
            "@context": [context_url]
        });
        let created = client.post_entity(&entity).send().await.expect("entity creation failed");
        assert_eq!(created.status, StatusCode::CREATED);
    }
    assert!(
        server.request_count(&path) >= 2,
        "max-age=0 must force a fresh remote context fetch for the second request"
    );

    server.shutdown().await.expect("context server shutdown failed");
    environment.reset_and_finish().await.expect("post-test reset failed");
}
