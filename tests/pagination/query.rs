//! Pagination Link header tests for `/subscriptions` and `/csourceRegistrations`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::{assert_eq, assert_ne};
use reqwest::{StatusCode, header::HeaderMap};
use serde_json::{Value, json};
use serial_test::serial;

fn default_subscription(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "timeInterval": 5,
        "entities": [{"type": "Building"}],
        "notification": {
            "format": "keyValues",
            "endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"}
        },
        "@context": [CORE_CONTEXT]
    })
}

fn default_registration(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Building"}], "propertyNames": ["name"]}],
        "endpoint": "http://my.csource.org:1026",
        "@context": [CORE_CONTEXT]
    })
}

async fn make_scope() -> (SerializedEnvironment, TestClient) {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = scope.client().clone();
    (scope, client)
}

async fn create_subscriptions(client: &TestClient, count: usize, tag: &str) -> Vec<String> {
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        let id = format!("urn:ngsi-ld:Subscription:{tag}:{index}:{}", uuid::Uuid::new_v4());
        let response = client
            .create_subscription(&default_subscription(&id))
            .await
            .expect("create_subscription failed");
        assert_eq!(response.status, StatusCode::CREATED);
        ids.push(id);
    }
    ids
}

async fn create_registrations(client: &TestClient, count: usize, tag: &str) -> Vec<String> {
    let mut ids = Vec::with_capacity(count);
    for index in 0..count {
        let id = format!("urn:ngsi-ld:ContextSourceRegistration:{tag}:{index}:{}", uuid::Uuid::new_v4());
        let response = client
            .create_registration(&default_registration(&id))
            .await
            .expect("create_registration failed");
        assert_eq!(response.status, StatusCode::CREATED);
        ids.push(id);
    }
    ids
}

fn collect_link_headers(headers: &HeaderMap) -> Vec<String> {
    headers
        .get_all("link")
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(str::to_owned)
        .collect()
}

fn find_pagination_link_header(headers: &HeaderMap) -> Option<String> {
    collect_link_headers(headers).into_iter().find(|header| {
        let lower = header.to_ascii_lowercase();
        (lower.contains("rel=\"prev\"")
            || lower.contains("rel=\"next\"")
            || lower.contains("rel=\"first\"")
            || lower.contains("rel=\"last\"")
            || lower.contains(" prev\"")
            || lower.contains(" next\"")
            || lower.contains(" first\"")
            || lower.contains(" last\""))
            && !lower.contains("json-ld#context")
    })
}

fn find_context_link_header(headers: &HeaderMap) -> Option<String> {
    collect_link_headers(headers).into_iter().find(|header| header.contains("json-ld#context"))
}

async fn query_subscriptions(client: &TestClient, query: &str, accept: &str) -> (StatusCode, HeaderMap, Value) {
    let path = if query.is_empty() {
        "/ngsi-ld/v1/subscriptions".to_owned()
    } else {
        format!("/ngsi-ld/v1/subscriptions?{query}")
    };
    let response = client
        .raw(reqwest::Method::GET, &path)
        .header("Accept", accept)
        .send()
        .await
        .expect("query subscriptions failed");
    let body = response.json_value().expect("subscription query response must be valid JSON");
    (response.status, response.headers, body)
}

async fn query_registrations(client: &TestClient, query: &str, accept: &str) -> (StatusCode, HeaderMap, Value) {
    let path = if query.is_empty() {
        "/ngsi-ld/v1/csourceRegistrations".to_owned()
    } else {
        format!("/ngsi-ld/v1/csourceRegistrations?{query}")
    };
    let response = client
        .raw(reqwest::Method::GET, &path)
        .header("Accept", accept)
        .send()
        .await
        .expect("query registrations failed");
    let body = response.json_value().expect("registration query response must be valid JSON");
    (response.status, response.headers, body)
}

fn extract_link_segment<'a>(header: &'a str, rel_name: &str) -> Option<&'a str> {
    let target = format!("rel=\"{rel_name}\"");
    header.split(',').map(str::trim).find(|segment| segment.contains(&target))
}

fn extract_url(segment: &str) -> &str {
    let start = segment.find('<').map_or(0, |i| i + 1);
    let end = segment.find('>').unwrap_or(segment.len());
    &segment[start..end]
}

#[tokio::test]
#[serial(global)]
async fn pagination_subscriptions_middle_page_returns_prev_and_next_in_single_link_header() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3, "mid").await;

    let (status, headers, _) = query_subscriptions(&client, "limit=1&offset=1", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"prev\""));
    assert!(pagination.contains("rel=\"next\""));
    assert!(pagination.contains("offset=0"));
    assert!(pagination.contains("offset=2"));
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_subscriptions_first_page_has_first_last_next_without_prev() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3, "first").await;

    let (status, headers, _) = query_subscriptions(&client, "limit=1&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"first\""));
    assert!(pagination.contains("rel=\"last\""));
    assert!(pagination.contains("rel=\"next\""));
    assert!(!pagination.contains("rel=\"prev\""));
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_subscriptions_last_page_has_first_last_prev_without_next() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3, "last").await;

    let (status, headers, _) = query_subscriptions(&client, "limit=1&offset=2", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"first\""));
    assert!(pagination.contains("rel=\"last\""));
    assert!(pagination.contains("rel=\"prev\""));
    assert!(!pagination.contains("rel=\"next\""));
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_subscriptions_single_page_has_first_last_without_prev_or_next() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3, "single").await;

    // A counted collection that fits in one page still knows its total, so it
    // carries the `first`/`last` pair (both at offset 0) with no `prev`/`next`.
    let (status, headers, _) = query_subscriptions(&client, "limit=10&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"first\""));
    assert!(pagination.contains("rel=\"last\""));
    assert!(!pagination.contains("rel=\"prev\""));
    assert!(!pagination.contains("rel=\"next\""));
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_subscriptions_context_and_pagination_are_separate_link_headers() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3, "sep").await;

    let (status, headers, _) = query_subscriptions(&client, "limit=1&offset=1", "application/json").await;
    assert_eq!(status, StatusCode::OK);

    let links = collect_link_headers(&headers);
    assert!(links.len() >= 2, "expected ≥2 Link headers, got {}: {links:?}", links.len());

    let context = find_context_link_header(&headers);
    let pagination = find_pagination_link_header(&headers);
    assert!(context.is_some());
    assert!(pagination.is_some());
    assert_ne!(context, pagination);
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_subscriptions_pagination_links_include_json_ld_media_type() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3, "mt").await;

    let (status, headers, _) = query_subscriptions(&client, "limit=1&offset=1", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("type=\"application/ld+json\""));
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_registrations_middle_page_returns_first_last_prev_next() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 5, "full-mid").await;

    let (status, headers, _) = query_registrations(&client, "type=Building&limit=1&offset=2", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    for rel in ["first", "last", "prev", "next"] {
        assert!(pagination.contains(&format!("rel=\"{rel}\"")), "should have {rel}: {pagination}");
    }
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_registrations_first_page_has_first_last_next_without_prev() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 5, "full-first").await;

    let (status, headers, _) = query_registrations(&client, "type=Building&limit=1&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"first\""));
    assert!(pagination.contains("rel=\"last\""));
    assert!(pagination.contains("rel=\"next\""));
    assert!(!pagination.contains("rel=\"prev\""));
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_registrations_last_page_has_first_last_prev_without_next() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 5, "full-last").await;

    let (status, headers, _) = query_registrations(&client, "type=Building&limit=1&offset=4", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"first\""));
    assert!(pagination.contains("rel=\"last\""));
    assert!(pagination.contains("rel=\"prev\""));
    assert!(!pagination.contains("rel=\"next\""));
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_registrations_first_link_points_to_offset_zero() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 5, "first-off").await;

    let (status, headers, _) = query_registrations(&client, "type=Building&limit=2&offset=2", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    let first_segment = extract_link_segment(&pagination, "first").expect("first segment missing");
    let first_url = extract_url(first_segment);
    assert!(first_url.contains("offset=0"), "first link should have offset=0: {first_url}");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_registrations_last_link_points_to_correct_offset() {
    let (scope, client) = make_scope().await;
    create_registrations(&client, 5, "last-off").await;

    let (status, headers, _) = query_registrations(&client, "type=Building&limit=2&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    let last_segment = extract_link_segment(&pagination, "last").expect("last segment missing");
    let last_url = extract_url(last_segment);
    assert!(last_url.contains("offset=4"), "last link should have offset=4: {last_url}");
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_registrations_link_type_reflects_the_request_media_type() {
    // Clause 6.3.10: a pagination link carries a mandatory `type` equal to the
    // negotiated response media type, so an `application/ld+json` request gets it
    // back on the link rather than no `type` at all.
    let (scope, client) = make_scope().await;
    create_registrations(&client, 5, "reflect-mt").await;

    let (status, headers, _) = query_registrations(&client, "type=Building&limit=1&offset=0", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(
        pagination.contains("type=\"application/ld+json\""),
        "registration pagination reflects the request media type: {pagination}"
    );
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_link_header_urls_preserve_non_pagination_query_params() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3, "preserve").await;

    let (status, headers, _) = query_subscriptions(&client, "limit=1&offset=1", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("limit="));
    scope.reset_and_finish().await.expect("post-test reset failed");
}

#[tokio::test]
#[serial(global)]
async fn pagination_each_link_value_is_parseable_rfc8288() {
    let (scope, client) = make_scope().await;
    create_subscriptions(&client, 3, "rfc").await;

    let (status, headers, _) = query_subscriptions(&client, "limit=1&offset=1", "application/ld+json").await;
    assert_eq!(status, StatusCode::OK);
    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    for segment in pagination.split(',') {
        let trimmed = segment.trim();
        assert!(trimmed.starts_with('<'), "link-value should start with '<': {trimmed}");
        assert!(trimmed.contains("rel="), "link-value should have rel=: {trimmed}");
    }
    scope.reset_and_finish().await.expect("post-test reset failed");
}
