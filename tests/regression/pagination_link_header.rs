//! A paginated subscription query carries RFC 8288 `Link` headers.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.9 requires a paginated result to provide
//! the previous and next pages through the HTTP `Link` header, and clause 6.3.10
//! carries the negotiated media type on each link. RFC 8288 defines that header:
//! its link-values are separated by commas, each is `<target-URI>` followed by
//! parameters, and optional whitespace around the delimiters is not significant.
//!
//! This pins the broker's `Link` output by its RFC 8288 semantics -- the `rel`
//! of each link, the page it points at, and the query parameters it preserves --
//! rather than by an exact byte string, which would be sensitive to
//! insignificant whitespace the broker is free to emit; the meaning asserted
//! here is what clause 5.5.9 mandates.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use regex::Regex;
use reqwest::StatusCode;
use serde_json::json;
use std::collections::HashMap;

/// Create one subscription so the query has a populated page to paginate.
async fn create_subscription(client: &TestClient, scope: &TestEnvironment, suffix: &str) {
    let id = scope.id(&format!("Subscription:pagination-{suffix}"));
    let body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_subscription(&body).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "create subscription: {:?}", created.body);
}

/// Parse an RFC 8288 `Link` header into a map of `rel` to target URI, tolerant
/// of insignificant whitespace around the delimiters.
fn links_by_rel(header: &str) -> HashMap<String, String> {
    let pattern = Regex::new(r#"<([^>]+)>[^,]*?rel="([^"]+)""#).expect("valid link-header pattern");
    pattern
        .captures_iter(header)
        .map(|capture| (capture[2].to_owned(), capture[1].to_owned()))
        .collect()
}

/// Clause 5.5.9: a middle page of a subscription query carries a `prev` and a
/// `next` link, each pointing at its page and preserving `limit` and `offset`.
#[tokio::test]
async fn regression_pagination_link_header_carries_prev_and_next_pages() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    // Four subscriptions, so `limit=1&offset=1` is a genuine middle page with a
    // page before it and pages after it.
    for suffix in ["a", "b", "c", "d"] {
        create_subscription(client, &scope, suffix).await;
    }

    let response = client
        .query_subscriptions("limit=1&offset=1", Some(CORE_CONTEXT))
        .await
        .expect("paginated subscription query");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);

    let header = response
        .headers
        .get("link")
        .expect("a paginated result carries a Link header")
        .to_str()
        .expect("the Link header is valid text");
    let links = links_by_rel(header);

    let prev = links.get("prev").unwrap_or_else(|| panic!("a middle page has a prev link: {header}"));
    assert!(prev.contains("subscriptions"), "the prev link points at the subscriptions endpoint: {prev}");
    assert!(prev.contains("offset=0"), "the prev link steps back to offset 0: {prev}");
    assert!(prev.contains("limit=1"), "the prev link preserves the limit: {prev}");

    let next = links.get("next").unwrap_or_else(|| panic!("a middle page has a next link: {header}"));
    assert!(next.contains("subscriptions"), "the next link points at the subscriptions endpoint: {next}");
    assert!(next.contains("offset=2"), "the next link steps forward to offset 2: {next}");
    assert!(next.contains("limit=1"), "the next link preserves the limit: {next}");

    scope.finish().await.expect("environment cleanup");
}
