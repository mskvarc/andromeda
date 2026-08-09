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

#![allow(clippy::expect_used, clippy::unwrap_used)]

use andromeda_test_common::{client::NgsiLdClient, config::COMPOUND_CONTEXT, scope::TestScope};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;

#[tokio::test]
async fn context_source_info_round_trips_through_create_and_retrieve() {
    let scope = TestScope::new("context_source_info");
    let client = NgsiLdClient::new(&scope).await;
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

    let info = retrieve
        .body
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

    scope.cleanup().await.expect("cleanup failed");
}

#[tokio::test]
#[ignore = "Forwarding-semantics coverage (reserved keys → outbound headers, custom keys → header passthrough, per-registration @context override) requires end-to-end plumbing through ContextSourceMock; follow-up: populate once the fixture wiring is in place."]
async fn context_source_info_forwarding_semantics_follow_up() {}
