#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use crate::{
    ALREADY_EXISTS_TYPE,
    BAD_REQUEST_DATA_TYPE,
    assert_problem_detail_type,
    client,
    create_csource_subscription,
    csource_subscription_payload,
    csource_subscription_without_notification_payload,
    empty_watched_attributes_csource_subscription_payload,
    expires_at_csource_subscription_payload,
    inactive_csource_subscription_payload,
    invalid_query_csource_subscription_payload,
    invalid_type_csource_subscription_payload,
    retrieve_csource_subscription,
};
use andromeda_ngsi_ld_lexicon::time::rendering::instant_text;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::{Duration, Utc};
use reqwest::StatusCode;
use serde_json::Value;
use std::time::Duration as StdDuration;
use tokio::time::sleep;

#[tokio::test]
async fn csr_subscription_create_with_id_returns_201_and_location() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-basic");
    let payload = csource_subscription_payload(&subscription_id);

    let (status, location, _) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let location = location.expect("expected Location header");
    assert!(location.contains(&subscription_id), "location should contain subscription id: {location}");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_without_id_returns_201_and_generated_id() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let mut payload = csource_subscription_payload("urn:ngsi-ld:Subscription:placeholder");
    if let Value::Object(object) = &mut payload {
        object.remove("id");
    }

    let (status, location, _) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let location = location.expect("expected Location header");
    let generated_id = location.rsplit('/').next().expect("expected generated id in location header").to_owned();
    assert!(generated_id.starts_with("urn:ngsi-ld:Subscription:"));
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_without_is_active_defaults_status_to_active() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-default-active");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_inactive_sets_status_to_paused() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-inactive");
    let payload = inactive_csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["isActive"], false);
    pretty_assertions::assert_eq!(body["status"], "paused");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_with_expires_at_becomes_expired_after_deadline() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-expiring");
    let expires_at = instant_text(Utc::now() + Duration::seconds(3));
    let payload = expires_at_csource_subscription_payload(&subscription_id, &expires_at);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    sleep(StdDuration::from_secs(5)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "expired");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_without_expires_at_remains_active() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-no-expiration");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    sleep(StdDuration::from_secs(3)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_duplicate_id_returns_409_already_exists() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-duplicate");
    let payload = csource_subscription_payload(&subscription_id);

    let (first_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(first_status, StatusCode::CREATED);

    let (second_status, _, body) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(second_status, StatusCode::CONFLICT);
    assert_problem_detail_type(&body, ALREADY_EXISTS_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_without_notification_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-missing-notification");
    let payload = csource_subscription_without_notification_payload(&subscription_id);

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_with_invalid_type_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-invalid-type");
    let payload = invalid_type_csource_subscription_payload(&subscription_id);

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_with_invalid_query_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-invalid-query");
    let payload = invalid_query_csource_subscription_payload(&subscription_id);

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_with_empty_watched_attributes_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-empty-watched-attributes");
    let payload = empty_watched_attributes_csource_subscription_payload(&subscription_id);

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.1, which clause 5.11.2 applies to a Context Source Registration
/// Subscription as to an entity one: the NGSI-LD Null `urn:ngsi-ld:null` (an
/// array of one where an array is required) in a URI member of a create is
/// `BadRequestData`.
#[tokio::test]
async fn csr_subscription_create_with_ngsi_ld_null_uri_member_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();

    for (member, null) in [
        ("jsonldContext", serde_json::json!("urn:ngsi-ld:null")),
        ("watchedAttributes", serde_json::json!(["urn:ngsi-ld:null"])),
    ] {
        let subscription_id = scope.id(&format!("Subscription:create-null-{member}"));
        let mut payload = csource_subscription_payload(&subscription_id);
        payload[member] = null;

        let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST, "{member}: {body}");
        assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    }
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_create_with_past_expires_at_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:create-past-expiration");
    let payload = expires_at_csource_subscription_payload(&subscription_id, "2018-08-01T22:07:00Z");

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}
