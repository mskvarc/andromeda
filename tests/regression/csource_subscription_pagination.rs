//! `GET /csourceSubscriptions` paginates by `limit`/`offset`, never `page`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.9.2 defines pagination as `limit` and
//! `offset`; `page` is not an NGSI-LD parameter. A prior handler emitted
//! `?limit=N&page=M` `Link` values for this collection, which are unfollowable
//! through the offset interface the endpoint accepts. These pin that an explicit
//! `offset` query carries `prev`/`next` `Link` values expressed in `offset`, that
//! no link value carries a `page` parameter, and that the last page offers only a
//! `prev` link (clause 5.5.9.1). The `first`/`last` relations are not asserted:
//! clause 5.5.9.1 mandates only the `prev` pointer on the last page and neither
//! forbids nor requires additional relations (see the interpretations register in
//! `docs/etsi-patch-audit.md`).
//!
//! The collection is broker-wide, so every test resets broker state on entry and
//! runs under `#[serial(global)]`. There is no dedicated client method for this
//! endpoint, so the requests are issued raw.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use regex::Regex;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;
use std::collections::HashMap;

/// A stable, zero-padded csource-subscription id whose C-collation order is its
/// numeric order.
fn subscription_id(index: u32) -> String {
    format!("urn:ngsi-ld:Subscription:cspg-offset:{index:04}")
}

/// A minimal csource-subscription body under the core context.
fn subscription_body(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    })
}

/// The JSON-LD `@context` `Link` header value a raw request carries (clause 5.5.7).
fn context_link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}

/// Create `count` csource subscriptions with predictable ids.
async fn create_subscriptions(client: &TestClient, count: u32) {
    for index in 0..count {
        let created = client
            .raw(Method::POST, "/ngsi-ld/v1/csourceSubscriptions")
            .header("content-type", "application/ld+json")
            .json(&subscription_body(&subscription_id(index)))
            .send()
            .await
            .expect("create csource subscription request");
        assert_eq!(created.status, StatusCode::CREATED, "create csource subscription: {:?}", created.body);
    }
}

/// GET one csource-subscription page and return its response headers.
async fn get_page_headers(client: &TestClient, path: &str) -> HeaderMap {
    let response = client
        .raw(Method::GET, path)
        .header("accept", "application/ld+json")
        .header("link", context_link_header().as_str())
        .send()
        .await
        .expect("csource subscription query");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);
    response.headers
}

/// The `Link` header text of a response, or the empty string when absent.
fn link_header(headers: &HeaderMap) -> String {
    headers
        .get("link")
        .map(|value| value.to_str().expect("the Link header is valid text").to_owned())
        .unwrap_or_default()
}

/// Parse an RFC 8288 `Link` header into a map of `rel` to target URI, tolerant of
/// insignificant whitespace around the delimiters.
fn links_by_rel(header: &str) -> HashMap<String, String> {
    let pattern = Regex::new(r#"<([^>]+)>[^,]*?rel="([^"]+)""#).expect("valid link-header pattern");
    pattern
        .captures_iter(header)
        .map(|capture| (capture[2].to_owned(), capture[1].to_owned()))
        .collect()
}

/// Clause 5.5.9.2: a middle page requested with an explicit `offset` carries
/// `prev`/`next` `Link` values expressed in `offset` and `limit`, targeting the
/// `csourceSubscriptions` endpoint, and no link value carries a `page` parameter.
#[tokio::test]
#[serial(global)]
async fn regression_csource_subscription_pagination_uses_offset_not_page() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = scope.client().clone();
    create_subscriptions(&client, 3).await;

    let headers = get_page_headers(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=1&offset=1").await;
    let header = link_header(&headers);
    assert!(!header.is_empty(), "a paginated middle page carries a Link header");
    assert!(!header.contains("page="), "no link value carries a page parameter: {header}");

    let links = links_by_rel(&header);
    let prev = links.get("prev").unwrap_or_else(|| panic!("a middle page has a prev link: {header}"));
    assert!(
        prev.contains("csourceSubscriptions"),
        "the prev link points at the csourceSubscriptions endpoint: {prev}"
    );
    assert!(prev.contains("offset=0"), "the prev link steps back to offset 0: {prev}");
    assert!(prev.contains("limit=1"), "the prev link preserves the limit: {prev}");

    let next = links.get("next").unwrap_or_else(|| panic!("a middle page has a next link: {header}"));
    assert!(
        next.contains("csourceSubscriptions"),
        "the next link points at the csourceSubscriptions endpoint: {next}"
    );
    assert!(next.contains("offset=2"), "the next link steps forward to offset 2: {next}");
    assert!(next.contains("limit=1"), "the next link preserves the limit: {next}");

    scope.reset_and_finish().await.expect("post-test reset");
}

/// Clause 5.5.9.1: the last page carries the pointer to the previous page and no
/// `next` link, and no link value carries a `page` parameter.
#[tokio::test]
#[serial(global)]
async fn regression_csource_subscription_pagination_last_page_has_only_prev() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = scope.client().clone();
    create_subscriptions(&client, 3).await;

    let headers = get_page_headers(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=1&offset=2").await;
    let header = link_header(&headers);
    assert!(!header.contains("page="), "no link value carries a page parameter: {header}");

    let links = links_by_rel(&header);
    assert!(links.contains_key("prev"), "the last page carries a prev link: {header}");
    assert!(!links.contains_key("next"), "the last page offers no next link: {header}");

    scope.reset_and_finish().await.expect("post-test reset");
}
