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
    assert_eq!(body.as_array().map(std::vec::Vec::len), Some(0), "limit=0 returns no entity data");
    scope.reset_and_finish().await.expect("post-test reset failed");
}
