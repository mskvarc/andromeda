#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use crate::{
    BAD_REQUEST_DATA_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    assert_problem_detail_type,
    client,
    create_csource_subscription,
    csource_subscription_payload,
    delete_csource_subscription,
    retrieve_csource_subscription,
};
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;

#[tokio::test]
async fn csr_subscription_delete_existing_returns_204_and_resource_not_found_afterwards() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:delete-existing");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let (delete_status, _) = delete_csource_subscription(&http_client, &subscription_id).await;
    pretty_assertions::assert_eq!(delete_status, StatusCode::NO_CONTENT);

    let (retrieve_status, _) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::NOT_FOUND);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_delete_invalid_uri_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();

    let (status, body) = delete_csource_subscription(&http_client, "invalidUri").await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_delete_unknown_id_returns_404_resource_not_found() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();

    let (status, body) = delete_csource_subscription(&http_client, "urn:ngsi-ld:Subscription:unknownSubscription").await;

    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem_detail_type(&body, RESOURCE_NOT_FOUND_TYPE);
    scope.finish().await.expect("cleanup failed");
}
