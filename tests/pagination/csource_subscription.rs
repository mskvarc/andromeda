//! Opaque-cursor pagination tests for `GET /csourceSubscriptions` (clause
//! 5.5.9.1).
//!
//! Naming convention:
//! `pagination_csource_subscriptions_<scenario>_<expected_outcome>`. The
//! collection is broker-wide, so every test runs under `#[serial(global)]` and
//! wipes broker state on entry. There is no dedicated client method for this
//! endpoint, so the requests are issued raw.

use crate::cursor_walk::{assert_cursor_links, context_link_rel, ids_of, joined_links, link_target};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;

async fn make_scope() -> (SerializedEnvironment, TestClient) {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = scope.client().clone();
    (scope, client)
}

/// The JSON-LD `@context` `Link` header a raw request carries.
fn context_link_header() -> String {
    format!("<{CORE_CONTEXT}>; rel=\"{}\"; type=\"application/ld+json\"", context_link_rel())
}

/// A stable, zero-padded csource-subscription id whose C-collation order is its
/// numeric order.
fn subscription_id(index: u32) -> String {
    format!("urn:ngsi-ld:Subscription:cspg:{index:04}")
}

fn subscription_body(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    })
}

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

/// GET one csource-subscription page by request path, returning its ids and
/// response headers.
async fn get_page(client: &TestClient, path: &str) -> (Vec<String>, HeaderMap) {
    let link_header = context_link_header();
    let response = client
        .raw(Method::GET, path)
        .header("accept", "application/ld+json")
        .header("link", link_header.as_str())
        .send()
        .await
        .expect("csource subscription query failed");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);
    let headers = response.headers.clone();
    (ids_of(&response.json_value().expect("valid JSON body")), headers)
}

/// GET one csource-subscription page and return only its status.
async fn get_status(client: &TestClient, path: &str) -> StatusCode {
    let link_header = context_link_header();
    client
        .raw(Method::GET, path)
        .header("accept", "application/ld+json")
        .header("link", link_header.as_str())
        .send()
        .await
        .expect("csource subscription query failed")
        .status
}

/// Walk `rel="next"` to exhaustion, collecting every id in order.
async fn walk_forward(client: &TestClient, start_path: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut path = start_path.to_string();
    for _ in 0..1000 {
        let (page, headers) = get_page(client, &path).await;
        seen.extend(page);
        let Some(next) = link_target(&headers, "next") else {
            return seen;
        };
        path = next;
    }
    // The iteration cap is far beyond any test collection; a walk that reaches
    // it has already returned everything it will.
    seen
}

/// Clause 5.5.9.1: the first page of a default query offers opaque cursor
/// pointers -- first/next/last carrying a `cursor`, no `prev`, no offset.
#[tokio::test]
#[serial(global)]
async fn pagination_csource_subscriptions_default_first_page_offers_opaque_cursor_pointers() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3).await;

    let (_, headers) = get_page(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=2").await;
    let links = joined_links(&headers);
    assert!(links.contains("rel=\"first\""), "{links}");
    assert!(links.contains("rel=\"next\""), "{links}");
    assert!(links.contains("rel=\"last\""), "{links}");
    assert!(!links.contains("rel=\"prev\""), "the first page has no prev: {links}");
    assert_cursor_links(&headers);
    scope.reset_and_finish().await.expect("post-test reset failed");
}

/// Following `rel="next"` to exhaustion returns every csource subscription
/// exactly once, ascending.
#[tokio::test]
#[serial(global)]
async fn pagination_csource_subscriptions_full_walk_returns_every_subscription_in_order() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 7).await;

    let seen = walk_forward(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=2").await;
    let expected: Vec<String> = (0..7).map(subscription_id).collect();
    assert_eq!(seen, expected, "the walk returns every subscription once, ascending");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

/// Following `rel="prev"` from a mid-collection page returns to the same items.
#[tokio::test]
#[serial(global)]
async fn pagination_csource_subscriptions_following_prev_returns_to_the_same_page() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 6).await;

    let (page1, headers1) = get_page(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=2").await;
    let next = link_target(&headers1, "next").expect("first page has a next");
    let (_, headers2) = get_page(&client, &next).await;
    let prev = link_target(&headers2, "prev").expect("a mid-collection page has a prev");
    let (back, _) = get_page(&client, &prev).await;
    assert_eq!(back, page1, "following prev returns to the same items");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

/// The `rel="last"` cursor jumps to the final page (the last `limit` items).
#[tokio::test]
#[serial(global)]
async fn pagination_csource_subscriptions_last_jumps_to_the_final_page() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 5).await;

    let (_, headers) = get_page(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=2").await;
    let last = link_target(&headers, "last").expect("cursor mode offers a last link");
    let (final_page, final_headers) = get_page(&client, &last).await;
    assert!(final_page.contains(&subscription_id(4)), "the final page holds the greatest id: {final_page:?}");
    assert!(link_target(&final_headers, "next").is_none(), "the final page offers no next");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

/// An explicit `offset` keeps the clause 5.5.9.2 offset pointers.
#[tokio::test]
#[serial(global)]
async fn pagination_csource_subscriptions_explicit_offset_keeps_offset_pointers() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 4).await;

    let (_, headers) = get_page(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=2&offset=2").await;
    let links = joined_links(&headers);
    assert!(links.contains("offset="), "an explicit offset keeps offset pointers: {links}");
    assert!(!links.contains("cursor="), "no cursor is offered under explicit offset: {links}");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

/// A malformed `cursor` is a `BadRequestData`.
#[tokio::test]
#[serial(global)]
async fn pagination_csource_subscriptions_malformed_cursor_is_bad_request() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 1).await;

    let status = get_status(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=2&cursor=zzzz").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.reset_and_finish().await.expect("post-test reset failed");
}

/// `cursor` together with `offset` is a `BadRequestData` (clause 5.5.9).
#[tokio::test]
#[serial(global)]
async fn pagination_csource_subscriptions_cursor_with_offset_is_bad_request() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 1).await;

    let status = get_status(&client, "/ngsi-ld/v1/csourceSubscriptions?limit=2&cursor=zzzz&offset=0").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.reset_and_finish().await.expect("post-test reset failed");
}
