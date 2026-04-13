#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::helpers::{
    client,
    collect_link_headers,
    create_registration,
    create_subscription,
    default_registration,
    default_subscription,
    delete_registration,
    delete_subscription,
    find_context_link_header,
    find_pagination_link_header,
    query_registrations,
    query_subscriptions,
    reset_broker_state,
    unique_registration_id,
    unique_subscription_id,
};
use reqwest::StatusCode;
use serial_test::serial;

// ---------------------------------------------------------------------------
// Subscription endpoint (OffsetPrevNext style)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn pagination_subscriptions_middle_page_returns_prev_and_next_in_single_link_header() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..3).map(|i| unique_subscription_id(&format!("mid-{i}"))).collect();
    for id in &ids {
        let body = default_subscription(id);
        let (status, _, _) = create_subscription(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED, "setup: create subscription {id}");
    }

    let (status, headers, _) = query_subscriptions(&http, "limit=1&offset=1", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"prev\""), "should have prev: {pagination}");
    assert!(pagination.contains("rel=\"next\""), "should have next: {pagination}");

    // Verify prev points to offset=0
    assert!(pagination.contains("offset=0"), "prev should have offset=0: {pagination}");
    // Verify next points to offset=2
    assert!(pagination.contains("offset=2"), "next should have offset=2: {pagination}");

    for id in &ids {
        delete_subscription(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_subscriptions_first_page_returns_next_only() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..3).map(|i| unique_subscription_id(&format!("first-{i}"))).collect();
    for id in &ids {
        let body = default_subscription(id);
        let (status, _, _) = create_subscription(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_subscriptions(&http, "limit=1&offset=0", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"next\""), "should have next: {pagination}");
    assert!(!pagination.contains("rel=\"prev\""), "should not have prev: {pagination}");

    for id in &ids {
        delete_subscription(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_subscriptions_last_page_returns_prev_only() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..3).map(|i| unique_subscription_id(&format!("last-{i}"))).collect();
    for id in &ids {
        let body = default_subscription(id);
        let (status, _, _) = create_subscription(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_subscriptions(&http, "limit=1&offset=2", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"prev\""), "should have prev: {pagination}");
    assert!(!pagination.contains("rel=\"next\""), "should not have next: {pagination}");

    for id in &ids {
        delete_subscription(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_subscriptions_single_page_returns_no_pagination_link() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..3).map(|i| unique_subscription_id(&format!("single-{i}"))).collect();
    for id in &ids {
        let body = default_subscription(id);
        let (status, _, _) = create_subscription(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_subscriptions(&http, "limit=10&offset=0", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    assert!(
        find_pagination_link_header(&headers).is_none(),
        "no pagination Link when all results fit in one page"
    );

    for id in &ids {
        delete_subscription(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_subscriptions_context_and_pagination_are_separate_link_headers() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..3).map(|i| unique_subscription_id(&format!("sep-{i}"))).collect();
    for id in &ids {
        let body = default_subscription(id);
        let (status, _, _) = create_subscription(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    // Use application/json Accept to get context Link header back
    let (status, headers, _) = query_subscriptions(&http, "limit=1&offset=1", "application/json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let all_links = collect_link_headers(&headers);
    assert!(
        all_links.len() >= 2,
        "should have at least 2 Link headers, got {}: {all_links:?}",
        all_links.len()
    );

    let context = find_context_link_header(&headers);
    let pagination = find_pagination_link_header(&headers);
    assert!(context.is_some(), "context Link header present");
    assert!(pagination.is_some(), "pagination Link header present");

    // They must be distinct header values
    pretty_assertions::assert_ne!(context, pagination, "context and pagination Links must be separate");

    for id in &ids {
        delete_subscription(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_subscriptions_pagination_links_include_json_ld_media_type() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..3).map(|i| unique_subscription_id(&format!("mt-{i}"))).collect();
    for id in &ids {
        let body = default_subscription(id);
        let (status, _, _) = create_subscription(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_subscriptions(&http, "limit=1&offset=1", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(
        pagination.contains("type=\"application/ld+json\""),
        "pagination links should include JSON-LD media type: {pagination}"
    );

    for id in &ids {
        delete_subscription(&http, id).await;
    }
}

// ---------------------------------------------------------------------------
// Registration endpoint (OffsetFull style)
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn pagination_registrations_middle_page_returns_first_last_prev_next() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..5).map(|i| unique_registration_id(&format!("full-mid-{i}"))).collect();
    for id in &ids {
        let body = default_registration(id);
        let (status, _, _) = create_registration(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED, "setup: create registration {id}");
    }

    let (status, headers, _) = query_registrations(&http, "type=Building&limit=1&offset=2", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"first\""), "should have first: {pagination}");
    assert!(pagination.contains("rel=\"last\""), "should have last: {pagination}");
    assert!(pagination.contains("rel=\"prev\""), "should have prev: {pagination}");
    assert!(pagination.contains("rel=\"next\""), "should have next: {pagination}");

    for id in &ids {
        delete_registration(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_registrations_first_page_has_first_last_next_without_prev() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..5).map(|i| unique_registration_id(&format!("full-first-{i}"))).collect();
    for id in &ids {
        let body = default_registration(id);
        let (status, _, _) = create_registration(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_registrations(&http, "type=Building&limit=1&offset=0", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"first\""), "should have first: {pagination}");
    assert!(pagination.contains("rel=\"last\""), "should have last: {pagination}");
    assert!(pagination.contains("rel=\"next\""), "should have next: {pagination}");
    assert!(!pagination.contains("rel=\"prev\""), "should not have prev: {pagination}");

    for id in &ids {
        delete_registration(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_registrations_last_page_has_first_last_prev_without_next() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..5).map(|i| unique_registration_id(&format!("full-last-{i}"))).collect();
    for id in &ids {
        let body = default_registration(id);
        let (status, _, _) = create_registration(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_registrations(&http, "type=Building&limit=1&offset=4", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(pagination.contains("rel=\"first\""), "should have first: {pagination}");
    assert!(pagination.contains("rel=\"last\""), "should have last: {pagination}");
    assert!(pagination.contains("rel=\"prev\""), "should have prev: {pagination}");
    assert!(!pagination.contains("rel=\"next\""), "should not have next: {pagination}");

    for id in &ids {
        delete_registration(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_registrations_first_link_points_to_offset_zero() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..5).map(|i| unique_registration_id(&format!("first-off-{i}"))).collect();
    for id in &ids {
        let body = default_registration(id);
        let (status, _, _) = create_registration(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_registrations(&http, "type=Building&limit=2&offset=2", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");

    // Extract the first link-value (the one with rel="first") and verify offset=0
    let first_segment = extract_link_segment(&pagination, "first");
    assert!(first_segment.is_some(), "should find first segment in: {pagination}");
    let first_url = extract_url(first_segment.unwrap());
    assert!(first_url.contains("offset=0"), "first link should have offset=0: {first_url}");

    for id in &ids {
        delete_registration(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_registrations_last_link_points_to_correct_offset() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..5).map(|i| unique_registration_id(&format!("last-off-{i}"))).collect();
    for id in &ids {
        let body = default_registration(id);
        let (status, _, _) = create_registration(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_registrations(&http, "type=Building&limit=2&offset=0", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");

    let last_segment = extract_link_segment(&pagination, "last");
    assert!(last_segment.is_some(), "should find last segment in: {pagination}");
    let last_url = extract_url(last_segment.unwrap());
    assert!(last_url.contains("offset=4"), "last link should have offset=4: {last_url}");

    for id in &ids {
        delete_registration(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_registrations_no_media_type_on_pagination_links() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..5).map(|i| unique_registration_id(&format!("no-mt-{i}"))).collect();
    for id in &ids {
        let body = default_registration(id);
        let (status, _, _) = create_registration(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_registrations(&http, "type=Building&limit=1&offset=0", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    assert!(
        !pagination.contains("type=\""),
        "registration pagination links should not include media type param: {pagination}"
    );

    for id in &ids {
        delete_registration(&http, id).await;
    }
}

// ---------------------------------------------------------------------------
// Edge cases
// ---------------------------------------------------------------------------

#[tokio::test]
#[serial]
async fn pagination_link_header_urls_preserve_non_pagination_query_params() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..3).map(|i| unique_subscription_id(&format!("preserve-{i}"))).collect();
    for id in &ids {
        let body = default_subscription(id);
        let (status, _, _) = create_subscription(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_subscriptions(&http, "limit=1&offset=1", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");
    // All pagination URLs should contain limit= (preserved)
    assert!(pagination.contains("limit="), "pagination URLs should have limit param: {pagination}");

    for id in &ids {
        delete_subscription(&http, id).await;
    }
}

#[tokio::test]
#[serial]
async fn pagination_each_link_value_is_parseable_rfc8288() {
    reset_broker_state().await;
    let http = client();

    let ids: Vec<String> = (0..3).map(|i| unique_subscription_id(&format!("rfc-{i}"))).collect();
    for id in &ids {
        let body = default_subscription(id);
        let (status, _, _) = create_subscription(&http, &body).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let (status, headers, _) = query_subscriptions(&http, "limit=1&offset=1", "application/ld+json").await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);

    let pagination = find_pagination_link_header(&headers).expect("pagination Link header present");

    // Each comma-separated segment should be a valid link-value
    for segment in pagination.split(',') {
        let trimmed = segment.trim();
        assert!(trimmed.starts_with('<'), "link-value should start with '<': {trimmed}");
        assert!(trimmed.contains("rel="), "link-value should have rel=: {trimmed}");
    }

    for id in &ids {
        delete_subscription(&http, id).await;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a comma-separated link segment that contains `rel="<rel_name>"`.
fn extract_link_segment<'a>(header: &'a str, rel_name: &str) -> Option<&'a str> {
    let target = format!("rel=\"{rel_name}\"");
    header.split(',').map(|s| s.trim()).find(|s| s.contains(&target))
}

/// Extract URL from a link-value like `</path?query>; rel="..."`.
fn extract_url(segment: &str) -> &str {
    let start = segment.find('<').map_or(0, |i| i + 1);
    let end = segment.find('>').unwrap_or(segment.len());
    &segment[start..end]
}
