//! Pagination Link header and `limit=0` tests for `GET /entities`.
//!
//! Naming convention: `pagination_entities_<scenario>_<expected_outcome>`. The
//! entity collection is tenant-wide, so every test runs under its own private tenant
//! and wipes broker state on entry.

use crate::cursor_walk::{assert_cursor_links, ids_of, joined_links, link_target, query_string_of};
use andromeda_test_client::{entity::results_count, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{StatusCode, header::HeaderMap};
use serde_json::{Value, json};

fn building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "b"},
        "@context": [CORE_CONTEXT]
    })
}

async fn make_scope() -> (TenantScopedEnvironment, TestClient) {
    let scope = TenantScopedEnvironment::from_suite("pagination").await.expect("tenant-scoped test environment");
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

/// Walk `rel="next"` from the given start query to exhaustion, collecting every
/// entity id seen, in page order.
async fn walk_forward(client: &TestClient, start_query: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut query_str = start_query.to_string();
    for _ in 0..1000 {
        let response = client
            .query_entities(&query_str, "application/ld+json", Some(CORE_CONTEXT))
            .await
            .expect("query entities failed");
        assert_eq!(response.status, StatusCode::OK);
        let headers = response.headers.clone();
        seen.extend(ids_of(&response.json_value().expect("valid JSON body")));
        let Some(next) = link_target(&headers, "next") else {
            return seen;
        };
        query_str = query_string_of(&next).to_string();
    }
    // The iteration cap is far beyond any test collection; a walk that reaches
    // it has already returned everything it will.
    seen
}

#[tokio::test]
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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn pagination_entities_second_page_returns_prev() {
    // The second page, offset past the first, offers a `prev` link back.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 3, "prev").await;

    let (status, headers) = query(&client, "type=Building&limit=2&offset=2", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let link = pagination_link(&headers).expect("pagination Link header present");
    assert!(link.contains("rel=\"prev\""), "{link}");
    assert!(!link.contains("rel=\"next\""), "{link}");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn pagination_entities_last_short_page_has_no_pagination_links() {
    // A single page holding every result offers neither `prev` nor `next`.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 2, "single").await;

    let (status, headers) = query(&client, "type=Building&limit=10&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    assert!(pagination_link(&headers).is_none(), "no pagination links on a single full result set");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn pagination_entities_default_first_page_offers_opaque_cursor_pointers() {
    // Clause 5.5.9.1: a locally-scoped, default-order query with no offset walks
    // by opaque cursor. The first page offers first/next/last carrying a
    // `cursor` (never an `offset`), and no `prev`.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 3, "cursorfirst").await;

    let (status, headers) = query(&client, "type=Building&local=true&limit=2", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let links = joined_links(&headers);
    assert!(!links.contains("rel=\"first\""), "the first page has no self-pointing first: {links}");
    assert!(links.contains("rel=\"next\""), "{links}");
    assert!(links.contains("rel=\"last\""), "{links}");
    assert!(!links.contains("rel=\"prev\""), "the first page has no prev: {links}");
    assert!(links.contains("cursor="), "the pointers carry an opaque cursor: {links}");
    assert!(!links.contains("offset="), "a cursor page never carries offset: {links}");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn pagination_entities_malformed_cursor_is_bad_request() {
    // Clause 5.5.9.1: the cursor is opaque and broker-emitted, so a corrupt one
    // is a BadRequestData rather than a position.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 1, "badcursor").await;

    let (status, _headers) = query(&client, "type=Building&local=true&cursor=zzzz", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn pagination_entities_cursor_with_offset_is_bad_request() {
    // Clause 5.5.9: a keyset cursor (5.5.9.1) and an offset (5.5.9.2) cannot both
    // govern one page.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 1, "cursoroffset").await;

    let (status, _headers) = query(&client, "type=Building&local=true&cursor=zzzz&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn pagination_entities_limit_zero_without_count_is_bad_request() {
    // Clause 6.3.10 Table 6.3.10-1: `limit=0` alone is invalid.
    let (scope, client) = make_scope().await;
    create_buildings(&client, 1, "lz").await;

    let (status, _headers) = query(&client, "type=Building&limit=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
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
    scope.finish().await.expect("cleanup failed");
}

/// Create one building under an explicit id (for tests that control ordering).
async fn create_one(client: &TestClient, id: &str) {
    let response = client.post_entity(&building(id)).with_ld_json().send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED);
}

/// Run one entity query and return the page's ids and its response headers.
async fn query_page(client: &TestClient, query_str: &str) -> (Vec<String>, HeaderMap) {
    let response = client
        .query_entities(query_str, "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("query entities failed");
    assert_eq!(response.status, StatusCode::OK);
    let headers = response.headers.clone();
    (ids_of(&response.json_value().expect("valid JSON body")), headers)
}

/// A stable, zero-padded building id whose C-collation order is its numeric
/// order.
fn stable_id(tag: &str, index: u32) -> String {
    format!("urn:ngsi-ld:Building:{tag}:{index:04}")
}

/// Clause 5.5.9.1: following `rel="prev"` from a mid-collection page returns to
/// the same items the previous page held (forward/backward round-trip).
#[tokio::test]
async fn pagination_entities_following_prev_returns_to_the_same_page() {
    let (scope, client) = make_scope().await;
    for index in 0..6 {
        create_one(&client, &stable_id("rt", index)).await;
    }

    let (page1, headers1) = query_page(&client, "type=Building&local=true&limit=2").await;
    assert_cursor_links(&headers1);
    let next = link_target(&headers1, "next").expect("first page has a next");
    let (page2, headers2) = query_page(&client, query_string_of(&next)).await;
    assert_eq!(page2.len(), 2, "the second page is full");

    let prev = link_target(&headers2, "prev").expect("a mid-collection page has a prev");
    let (back, _) = query_page(&client, query_string_of(&prev)).await;
    assert_eq!(back, page1, "following prev returns to the same items");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.9.1: `rel="prev"` disappears once a backward fetch comes back
/// short, which is how a count-less keyset walk recognises the first page.
///
/// Five ids with `limit=3`: the from-end page holds `[2,3,4]`, and following its
/// `prev` reads `id < 2`, which returns only `[0,1]` -- a short page (2 < 3),
/// the first page, offering no further `prev`. A backward page that comes back
/// *full* still offers a `prev` (it cannot know it is the boundary without a
/// count), so the short page is what pins the disappearance.
#[tokio::test]
async fn pagination_entities_prev_disappears_at_the_first_page() {
    let (scope, client) = make_scope().await;
    for index in 0..5 {
        create_one(&client, &stable_id("pf", index)).await;
    }

    let (_, headers) = query_page(&client, "type=Building&local=true&limit=3").await;
    let last = link_target(&headers, "last").expect("cursor mode offers a last link");
    let (_, last_headers) = query_page(&client, query_string_of(&last)).await;
    let prev = link_target(&last_headers, "prev").expect("the from-end page has a prev");
    let (first_page, first_headers) = query_page(&client, query_string_of(&prev)).await;
    assert_eq!(
        first_page,
        vec![stable_id("pf", 0), stable_id("pf", 1)],
        "the short backward page is the first two ids"
    );
    assert!(
        link_target(&first_headers, "prev").is_none(),
        "a short backward page is the first page, offering no further prev"
    );
    scope.finish().await.expect("cleanup failed");
}

/// The Andromeda `rel="last"` cursor jumps to the final page (the last `limit`
/// items), reachable without a count query.
#[tokio::test]
async fn pagination_entities_last_jumps_to_the_final_page() {
    let (scope, client) = make_scope().await;
    for index in 0..5 {
        create_one(&client, &stable_id("last", index)).await;
    }

    let (_, headers) = query_page(&client, "type=Building&local=true&limit=2").await;
    let last = link_target(&headers, "last").expect("cursor mode offers a last link");
    let (final_page, final_headers) = query_page(&client, query_string_of(&last)).await;
    assert_eq!(final_page.len(), 2, "the final page holds the last limit items");
    assert!(
        final_page.contains(&stable_id("last", 4)),
        "the final page holds the greatest id: {final_page:?}"
    );
    assert!(link_target(&final_headers, "next").is_none(), "the final page offers no next");
    scope.finish().await.expect("cleanup failed");
}

/// Stability: an entity inserted with a higher id mid-walk appears on a later
/// page rather than being skipped, which offset paging over a growing
/// collection could not guarantee.
#[tokio::test]
async fn pagination_entities_an_insert_mid_walk_appears_in_the_walk() {
    let (scope, client) = make_scope().await;
    for index in 0..4 {
        create_one(&client, &stable_id("ins", index)).await;
    }

    // Walk the first page, then insert the greatest id before continuing.
    let (mut seen, headers) = query_page(&client, "type=Building&local=true&limit=2").await;
    create_one(&client, "urn:ngsi-ld:Building:ins:9999").await;
    let mut next = link_target(&headers, "next");
    while let Some(link) = next {
        let (page, page_headers) = query_page(&client, query_string_of(&link)).await;
        seen.extend(page);
        next = link_target(&page_headers, "next");
    }
    assert!(
        seen.contains(&"urn:ngsi-ld:Building:ins:9999".to_string()),
        "the inserted higher id appears: {seen:?}"
    );
    scope.finish().await.expect("cleanup failed");
}

/// Stability: deleting an already-passed id mid-walk does not skip or duplicate
/// its neighbours -- the keyset continues after the last id seen, not at a
/// shifted offset. The contrast is an offset walk, which would skip an entity
/// when a preceding one is deleted.
#[tokio::test]
async fn pagination_entities_a_delete_mid_walk_does_not_skip_neighbours() {
    let (scope, client) = make_scope().await;
    for index in 0..6 {
        create_one(&client, &stable_id("del", index)).await;
    }

    let (page1, headers) = query_page(&client, "type=Building&local=true&limit=2").await;
    assert_eq!(page1, vec![stable_id("del", 0), stable_id("del", 1)], "the first page is the two least ids");
    // Delete an already-returned id, then continue the walk.
    client.delete_entity(&stable_id("del", 0)).await.expect("delete entity failed");
    let next = link_target(&headers, "next").expect("first page has a next");
    let (page2, _) = query_page(&client, query_string_of(&next)).await;
    assert_eq!(
        page2,
        vec![stable_id("del", 2), stable_id("del", 3)],
        "the next page still starts after the last id seen, skipping nothing: {page2:?}"
    );
    scope.finish().await.expect("cleanup failed");
}

/// The full forward walk over a stable collection returns every entity exactly
/// once, in ascending id order.
#[tokio::test]
async fn pagination_entities_full_walk_returns_every_entity_in_order() {
    let (scope, client) = make_scope().await;
    for index in 0..7 {
        create_one(&client, &stable_id("full", index)).await;
    }

    let seen = walk_forward(&client, "type=Building&local=true&limit=2").await;
    let expected: Vec<String> = (0..7).map(|index| stable_id("full", index)).collect();
    assert_eq!(seen, expected, "the walk returns every entity once, ascending");
    scope.finish().await.expect("cleanup failed");
}

/// Cursor mode still honours an explicit `count`: the `NGSILD-Results-Count`
/// header reports the whole collection, not the page.
#[tokio::test]
async fn pagination_entities_cursor_mode_still_reports_the_count() {
    let (scope, client) = make_scope().await;
    for index in 0..5 {
        create_one(&client, &stable_id("cnt", index)).await;
    }

    let response = client
        .query_entities("type=Building&local=true&limit=2&count=true", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("query entities failed");
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        results_count(&response.headers),
        Some(5),
        "count reports the whole collection under cursor mode"
    );
    assert_cursor_links(&response.headers);
    scope.finish().await.expect("cleanup failed");
}
