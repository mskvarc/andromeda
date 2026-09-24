//! Opaque-cursor pagination tests for `GET /csourceRegistrations` (clause
//! 5.5.9.1).
//!
//! Naming convention: `pagination_registrations_<scenario>_<expected_outcome>`.
//! The registration collection is broker-wide, so every test runs under
//! its own private tenant, whose collection starts empty. Clause 5.10.2.4 requires
//! a narrowing selector, so every query carries `type=Building`.

use crate::cursor_walk::{assert_cursor_links, ids_of, joined_links, link_target, query_string_of};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{StatusCode, header::HeaderMap};
use serde_json::{Value, json};

async fn make_scope() -> (TenantScopedEnvironment, TestClient) {
    let scope = TenantScopedEnvironment::from_suite("pagination").await.expect("tenant-scoped test environment");
    let client = scope.client().clone();
    (scope, client)
}

/// A stable, zero-padded registration id whose C-collation order is its numeric
/// order.
fn registration_id(index: u32) -> String {
    format!("urn:ngsi-ld:ContextSourceRegistration:pg:{index:04}")
}

fn registration_body(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}], "propertyNames": ["name"]}],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

async fn create_registrations(client: &TestClient, count: u32) {
    for index in 0..count {
        let created = client
            .create_registration(&registration_body(&registration_id(index)))
            .await
            .expect("create registration request");
        assert_eq!(created.status, StatusCode::CREATED, "create registration: {:?}", created.body);
    }
}

/// Run one registration query and return the page's ids and response headers.
async fn query_page(client: &TestClient, query_str: &str) -> (Vec<String>, HeaderMap) {
    let response = client
        .query_registrations(query_str, Some(CORE_CONTEXT))
        .await
        .expect("query registrations failed");
    assert_eq!(response.status, StatusCode::OK, "query: {:?}", response.body);
    let headers = response.headers.clone();
    (ids_of(&response.json_value().expect("valid JSON body")), headers)
}

/// Walk `rel="next"` to exhaustion, collecting every registration id in order.
async fn walk_forward(client: &TestClient, start_query: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut query_str = start_query.to_string();
    for _ in 0..1000 {
        let (page, headers) = query_page(client, &query_str).await;
        seen.extend(page);
        let Some(next) = link_target(&headers, "next") else {
            return seen;
        };
        query_str = query_string_of(&next).to_string();
    }
    // The iteration cap is far beyond any test collection; a walk that reaches
    // it has already returned everything it will.
    seen
}

/// Clause 5.5.9.1: the first page of a default registration query offers opaque
/// cursor pointers -- first/next/last carrying a `cursor`, no `prev`, no offset.
#[tokio::test]
async fn pagination_registrations_default_first_page_offers_opaque_cursor_pointers() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 3).await;

    let (_, headers) = query_page(&client, "type=Building&limit=2").await;
    let links = joined_links(&headers);
    assert!(!links.contains("rel=\"first\""), "the first page has no self-pointing first: {links}");
    assert!(links.contains("rel=\"next\""), "{links}");
    assert!(links.contains("rel=\"last\""), "{links}");
    assert!(!links.contains("rel=\"prev\""), "the first page has no prev: {links}");
    assert_cursor_links(&headers);
    scope.finish().await.expect("cleanup failed");
}

/// Following `rel="next"` to exhaustion returns every registration exactly once,
/// ascending.
#[tokio::test]
async fn pagination_registrations_full_walk_returns_every_registration_in_order() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 7).await;

    let seen = walk_forward(&client, "type=Building&limit=2").await;
    let expected: Vec<String> = (0..7).map(registration_id).collect();
    assert_eq!(seen, expected, "the walk returns every registration once, ascending");
    scope.finish().await.expect("cleanup failed");
}

/// Following `rel="prev"` from a mid-collection page returns to the same items.
#[tokio::test]
async fn pagination_registrations_following_prev_returns_to_the_same_page() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 6).await;

    let (page1, headers1) = query_page(&client, "type=Building&limit=2").await;
    let next = link_target(&headers1, "next").expect("first page has a next");
    let (_, headers2) = query_page(&client, query_string_of(&next)).await;
    let prev = link_target(&headers2, "prev").expect("a mid-collection page has a prev");
    let (back, _) = query_page(&client, query_string_of(&prev)).await;
    assert_eq!(back, page1, "following prev returns to the same items");
    scope.finish().await.expect("cleanup failed");
}

/// The `rel="last"` cursor jumps to the final page (the last `limit` items).
#[tokio::test]
async fn pagination_registrations_last_jumps_to_the_final_page() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 5).await;

    let (_, headers) = query_page(&client, "type=Building&limit=2").await;
    let last = link_target(&headers, "last").expect("cursor mode offers a last link");
    let (final_page, final_headers) = query_page(&client, query_string_of(&last)).await;
    assert!(final_page.contains(&registration_id(4)), "the final page holds the greatest id: {final_page:?}");
    assert!(link_target(&final_headers, "next").is_none(), "the final page offers no next");
    scope.finish().await.expect("cleanup failed");
}

/// An explicit `offset` keeps the clause 5.5.9.2 offset pointers.
#[tokio::test]
async fn pagination_registrations_explicit_offset_keeps_offset_pointers() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 4).await;

    let (_, headers) = query_page(&client, "type=Building&limit=2&offset=2").await;
    let links = joined_links(&headers);
    assert!(links.contains("offset="), "an explicit offset keeps offset pointers: {links}");
    assert!(!links.contains("cursor="), "no cursor is offered under explicit offset: {links}");
    scope.finish().await.expect("cleanup failed");
}

/// A malformed `cursor` is a `BadRequestData`.
#[tokio::test]
async fn pagination_registrations_malformed_cursor_is_bad_request() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 1).await;

    let response = client
        .query_registrations("type=Building&limit=2&cursor=zzzz", Some(CORE_CONTEXT))
        .await
        .expect("query registrations failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    scope.finish().await.expect("cleanup failed");
}

/// `cursor` together with `offset` is a `BadRequestData` (clause 5.5.9).
#[tokio::test]
async fn pagination_registrations_cursor_with_offset_is_bad_request() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 1).await;

    let response = client
        .query_registrations("type=Building&limit=2&cursor=zzzz&offset=0", Some(CORE_CONTEXT))
        .await
        .expect("query registrations failed");
    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    scope.finish().await.expect("cleanup failed");
}
