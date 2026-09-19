//! Pagination Link header and `limit=0` tests for `GET /entities`.
//!
//! Naming convention: `pagination_entities_<scenario>_<expected_outcome>`. The
//! entity collection is broker-wide, so every test runs under `#[serial(global)]`
//! and wipes broker state on entry.

use andromeda_test_client::{entity::results_count, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;

fn building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "b"},
        "@context": [CORE_CONTEXT]
    })
}

async fn make_scope() -> (SerializedEnvironment, TestClient) {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = scope.client().clone();
    (scope, client)
}

async fn create_buildings(client: &TestClient, count: usize, tag: &str) {
    for index in 0..count {
        let id = format!("urn:ngsi-ld:Building:{tag}:{index}:{}", uuid::Uuid::new_v4());
        let response = client.post_entity(&building(&id)).with_ld_json().send().await.expect("create entity failed");
        assert_eq!(response.status, StatusCode::CREATED);
    }
}

fn pagination_link(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::to_owned)
        .find(|header| {
            let lower = header.to_ascii_lowercase();
            (lower.contains("rel=\"prev\"") || lower.contains("rel=\"next\"")) && !lower.contains("json-ld#context")
        })
}

async fn query(client: &TestClient, query_str: &str, accept: &str) -> (StatusCode, HeaderMap) {
    let response = client
        .query_entities(query_str, accept, Some(CORE_CONTEXT))
        .await
        .expect("query entities failed");
    (response.status, response.headers)
}

/// Every `Link` header value the response carries, joined for inspection.
fn joined_links(headers: &HeaderMap) -> String {
    headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The target URL of the `rel="<rel>"` pagination link, if the response carries
/// one. Link values are comma-separated and the cursor tokens are base64url, so
/// a comma never appears inside a target here.
fn link_target(headers: &HeaderMap, rel: &str) -> Option<String> {
    let needle = format!("rel=\"{rel}\"");
    headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|header| header.split(','))
        .find(|value| value.contains(&needle))
        .and_then(|value| {
            let start = value.find('<')? + 1;
            let end = value[start..].find('>')? + start;
            Some(value[start..end].to_string())
        })
}

/// The query-string portion of a link target, which is what the query helper
/// takes.
fn query_string_of(target: &str) -> &str {
    target.split_once('?').map_or("", |(_, query)| query)
}

/// The `id` of every entity in a query response body, in order.
fn ids_of(body: &Value) -> Vec<String> {
    body.as_array()
        .expect("a collection response")
        .iter()
        .map(|entity| entity.get("id").and_then(Value::as_str).expect("an entity id").to_string())
        .collect()
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_full_page_returns_next_with_the_reflected_type() {
    // Clause 6.3.10 / 4.12: a full page (results == limit) offers a `next` link,
    // and clause 6.3.10 requires its `type` to equal the negotiated media type.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 3, "next").await;

    let (status, headers) = query(&client, "type=Building&limit=2&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let link = pagination_link(&headers).expect("pagination Link header present");
    assert!(link.contains("rel=\"next\""), "{link}");
    assert!(!link.contains("rel=\"prev\""), "{link}");
    assert!(link.contains("type=\"application/ld+json\""), "{link}");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_reflects_plain_json_media_type() {
    // The reflected `type` follows the request: a plain JSON query gets
    // application/json on its pagination link.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 3, "json").await;

    let (status, headers) = query(&client, "type=Building&limit=2&offset=0", "application/json").await;
    assert_eq!(status, StatusCode::OK);
    let link = pagination_link(&headers).expect("pagination Link header present");
    assert!(link.contains("type=\"application/json\""), "{link}");
    assert!(!link.contains("ld+json"), "{link}");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_second_page_returns_prev() {
    // The second page, offset past the first, offers a `prev` link back.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 3, "prev").await;

    let (status, headers) = query(&client, "type=Building&limit=2&offset=2", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let link = pagination_link(&headers).expect("pagination Link header present");
    assert!(link.contains("rel=\"prev\""), "{link}");
    assert!(!link.contains("rel=\"next\""), "{link}");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_last_short_page_has_no_pagination_links() {
    // A single page holding every result offers neither `prev` nor `next`.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 2, "single").await;

    let (status, headers) = query(&client, "type=Building&limit=10&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(pagination_link(&headers).is_none(), "no pagination links on a single full result set");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_default_first_page_offers_opaque_cursor_pointers() {
    // Clause 5.5.9.1: a locally-scoped, default-order query with no offset walks
    // by opaque cursor. The first page offers first/next/last carrying a
    // `cursor` (never an `offset`), and no `prev`.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 3, "cursorfirst").await;

    let (status, headers) = query(&client, "type=Building&local=true&limit=2", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let links = joined_links(&headers);
    assert!(links.contains("rel=\"first\""), "{links}");
    assert!(links.contains("rel=\"next\""), "{links}");
    assert!(links.contains("rel=\"last\""), "{links}");
    assert!(!links.contains("rel=\"prev\""), "the first page has no prev: {links}");
    assert!(links.contains("cursor="), "the pointers carry an opaque cursor: {links}");
    assert!(!links.contains("offset="), "a cursor page never carries offset: {links}");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_following_next_walks_every_entity_exactly_once() {
    // Clause 5.5.9.1: following `rel="next"` to exhaustion returns every entity
    // exactly once, and the last page has a `prev` but no `next`.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 5, "walk").await;

    let mut seen: Vec<String> = Vec::new();
    let mut query_str = "type=Building&local=true&limit=2".to_string();
    let mut pages = 0;
    loop {
        let response = client
            .query_entities(&query_str, "application/ld+json", Some(CORE_CONTEXT))
            .await
            .expect("query entities failed");
        assert_eq!(response.status, StatusCode::OK);
        let headers = response.headers.clone();
        seen.extend(ids_of(&response.json_value().expect("valid JSON body")));
        pages += 1;
        assert!(pages <= 10, "the walk must terminate");
        if let Some(next) = link_target(&headers, "next") {
            query_str = query_string_of(&next).to_string();
        } else {
            assert!(link_target(&headers, "prev").is_some(), "the last page still points back");
            break;
        }
    }
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), 5, "every entity is returned exactly once across the walk");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_explicit_order_by_falls_back_to_offset_pointers() {
    // An explicit non-id `orderBy` is not the default order, so the keyset
    // cursor is not offered and the clause 5.5.9.2 offset pointers stand.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 3, "orderby").await;

    let (status, headers) = query(&client, "type=Building&local=true&orderBy=name&limit=2", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let links = joined_links(&headers);
    assert!(links.contains("rel=\"next\""), "{links}");
    assert!(links.contains("offset="), "an explicit orderBy keeps offset pointers: {links}");
    assert!(!links.contains("cursor="), "no cursor is offered for a non-default order: {links}");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_malformed_cursor_is_bad_request() {
    // Clause 5.5.9.1: the cursor is opaque and broker-emitted, so a corrupt one
    // is a BadRequestData rather than a position.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 1, "badcursor").await;

    let (status, _headers) = query(&client, "type=Building&local=true&cursor=zzzz", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_cursor_with_offset_is_bad_request() {
    // Clause 5.5.9: a keyset cursor (5.5.9.1) and an offset (5.5.9.2) cannot both
    // govern one page.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 1, "cursoroffset").await;

    let (status, _headers) = query(&client, "type=Building&local=true&cursor=zzzz&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_limit_zero_without_count_is_bad_request() {
    // Clause 6.3.10 Table 6.3.10-1: `limit=0` alone is invalid.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 1, "lz").await;

    let (status, _headers) = query(&client, "type=Building&limit=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_entities_limit_zero_with_count_returns_the_count() {
    // Clause 6.3.10 Table 6.3.10-1: `limit=0&count=true` returns the matching
    // count and no entity data.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 3, "lzc").await;

    let response = client
        .query_entities("type=Building&limit=0&count=true", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("query entities failed");
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(results_count(&response.headers), Some(3), "the count header reports every matching entity");
    let body = response.json_value().expect("entity query response must be valid JSON");
    assert_eq!(body.as_array().map(Vec::len), Some(0), "limit=0 returns no entity data");
    scope.reset_and_finish().await.expect("post-test reset failed");
}
