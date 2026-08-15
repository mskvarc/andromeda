//! `contextSourceInfo` integration tests (NGSI-LD 6.3.19).
//!
//! The registration model accepts a `contextSourceInfo` array of
//! key/value pairs that the broker must forward as request metadata when
//! calling the registered Context Source. This suite covers:
//!
//! - reserved keys (`accept`, `contentType`, `jsonldContext`,
//!   `ngsiLdConformance`) map to the corresponding outbound HTTP semantics,
//! - custom keys are forwarded verbatim as headers,
//! - per-registration `@context` overrides the broker default.
//!
//! Full coverage is substantial; this initial test asserts the broker
//! round-trips `contextSourceInfo` through CSR registration + retrieval
//! unchanged. Exercising the forwarding semantics requires the
//! `ContextSourceMock` infrastructure and is deferred to follow-up work.
//!
//! Naming convention: `context_source_info_<area>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_fixtures::context::{COMPOUND_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{
    callback_path::CallbackPath,
    callback_server::MockResponse,
    context_source_server::ContextSourceServer,
    server_config::MockServerConfig,
};
use axum::{
    body::Bytes,
    http::{HeaderMap, HeaderValue, Method, header::CONTENT_TYPE},
};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn context_source_info_round_trips_through_create_and_retrieve() {
    let scope = TestEnvironment::from_suite("context_source_info").await.expect("isolated test environment");
    let client = scope.client();
    let registration_id = scope.id("ContextSourceRegistration:info-round-trip");

    let registration = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": "http://my.csource.org:1026",
        "contextSourceInfo": [
            {"key": "accept", "value": "application/ld+json"},
            {"key": "X-Custom-Auth", "value": "token-abc"}
        ],
        "@context": [COMPOUND_CONTEXT]
    });

    let create = client.create_registration(&registration).await.expect("create registration failed");
    assert_eq!(create.status, StatusCode::CREATED);

    let retrieve = client
        .retrieve_registration(&registration_id, Some(COMPOUND_CONTEXT))
        .await
        .expect("retrieve registration failed");
    assert_eq!(retrieve.status, StatusCode::OK);

    let body = retrieve.json_value().expect("retrieved registration must be valid JSON");
    let info = body
        .get("contextSourceInfo")
        .and_then(|value| value.as_array())
        .expect("contextSourceInfo must be present and encoded as an array after round-trip");
    let entries: Vec<(String, String)> = info
        .iter()
        .filter_map(|entry| {
            let key = entry.get("key")?.as_str()?.to_owned();
            let value = entry.get("value")?.as_str()?.to_owned();
            Some((key, value))
        })
        .collect();
    assert!(entries.contains(&("accept".to_owned(), "application/ld+json".to_owned())));
    assert!(entries.contains(&("X-Custom-Auth".to_owned(), "token-abc".to_owned())));

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn context_source_info_forwarding_maps_reserved_and_custom_entries_to_http_headers() {
    let scope = TestEnvironment::from_suite("context-source-info-forwarding")
        .await
        .expect("isolated test environment");
    let client = scope.client();
    let source = ContextSourceServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("context source must start");
    let source_path = CallbackPath::parse("/ngsi-ld/v1/entities").expect("absolute source path");
    let mut response_headers = HeaderMap::new();
    response_headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"));
    source
        .enqueue(
            Method::GET,
            source_path,
            MockResponse::builder()
                .status(StatusCode::OK)
                .headers(response_headers)
                .body(Bytes::from_static(b"[]"))
                .build(),
        )
        .await;
    let source_root = CallbackPath::parse("/").expect("absolute source root");
    let registration = json!({
        "id": scope.id("ContextSourceRegistration:forwarding"),
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": source.endpoint(&source_root).expect("source endpoint must be valid"),
        "operations": ["queryEntity"],
        "contextSourceInfo": [
            {"key": "accept", "value": "application/ld+json"},
            {"key": "contentType", "value": "application/json"},
            {"key": "jsonldContext", "value": TEST_SUITE_CONTEXT},
            {"key": "X-Custom-Auth", "value": "token-abc"}
        ],
        "@context": [COMPOUND_CONTEXT]
    });
    let created = client.create_registration(&registration).await.expect("registration creation failed");
    assert_eq!(created.status, StatusCode::CREATED);

    let query = client
        .query_entities("type=Vehicle", "application/ld+json", Some(COMPOUND_CONTEXT))
        .await
        .expect("forwarded query failed");
    assert_eq!(query.status, StatusCode::OK);
    let forwarded = source
        .next_request(Duration::from_secs(5))
        .await
        .expect("context source must receive the forwarded request");
    assert_eq!(
        forwarded.headers.get("accept").and_then(|value| value.to_str().ok()),
        Some("application/ld+json")
    );
    assert_eq!(
        forwarded.headers.get("content-type").and_then(|value| value.to_str().ok()),
        Some("application/json")
    );
    assert_eq!(forwarded.headers.get("x-custom-auth").and_then(|value| value.to_str().ok()), Some("token-abc"));
    let link = forwarded
        .headers
        .get("link")
        .and_then(|value| value.to_str().ok())
        .expect("forwarded request must include a JSON-LD Link header");
    assert!(link.contains(TEST_SUITE_CONTEXT.as_str()));

    source.shutdown().await.expect("context source shutdown failed");
    scope.finish().await.expect("cleanup failed");
}
