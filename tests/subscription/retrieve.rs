#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use reqwest::StatusCode;

use crate::helpers::{BAD_REQUEST_DATA_TYPE, RESOURCE_NOT_FOUND_TYPE, client, create_subscription, default_subscription, retrieve_subscription};
use andromeda_test_harness::environment::TestEnvironment;

#[tokio::test]
async fn subscription_retrieve_invalid_id_returns_bad_request_data() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");

    let (status, body) = retrieve_subscription(&http_client, "InvalidUri", false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_retrieve_unknown_id_returns_resource_not_found() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");

    let (status, body) = retrieve_subscription(&http_client, "urn:ngsi-ld:Subscription:unknown", false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_retrieve_existing_id_returns_subscription_payload() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:retrieve-existing");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, true).await;

    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["id"], subscription_id);
    pretty_assertions::assert_eq!(body["type"], "Subscription");
    assert!(body["entities"].is_array());
    scope.finish().await.expect("cleanup failed");
}
