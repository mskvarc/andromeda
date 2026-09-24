#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use crate::{
    BAD_REQUEST_DATA_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    assert_problem_detail_type,
    client,
    create_csource_subscription,
    csource_subscription_payload,
    update_csource_subscription,
    update_csource_subscription_raw,
    update_invalid_notification_fragment,
    update_invalid_type_fragment,
    update_notification_fragment,
};
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;

#[tokio::test]
async fn csr_subscription_update_existing_returns_204() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:update-existing");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let fragment = update_notification_fragment();
    let (status, _) = update_csource_subscription(&http_client, &subscription_id, &fragment).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_update_invalid_uri_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let fragment = update_notification_fragment();

    let (status, body) = update_csource_subscription(&http_client, "invalidUri", &fragment).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_update_unknown_id_returns_404_resource_not_found() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let fragment = update_notification_fragment();

    let (status, body) = update_csource_subscription(&http_client, "urn:ngsi-ld:Subscription:unknownSubscription", &fragment).await;

    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem_detail_type(&body, RESOURCE_NOT_FOUND_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_update_invalid_type_fragment_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:update-invalid-type-fragment");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let fragment = update_invalid_type_fragment();
    let (status, body) = update_csource_subscription(&http_client, &subscription_id, &fragment).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_update_invalid_notification_fragment_returns_400_bad_request_data() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:update-invalid-notification-fragment");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let fragment = update_invalid_notification_fragment();
    let (status, body) = update_csource_subscription(&http_client, &subscription_id, &fragment).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn csr_subscription_update_malformed_json_returns_400_invalid_request() {
    let scope = TestEnvironment::from_suite("csr_subscription").await.expect("isolated test environment");
    let http_client = client();
    let subscription_id = scope.id("Subscription:update-malformed-json");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let malformed_json = br#"{
   \"notification\":{
      \"format\":\"keyValues\",,
      \"endpoint\":{
         \"uri\":\"http://my.second.endpoint.org/notify\",
         \"accept\":\"application/json\"
      }
   }
}"#;

    let (status, body) = update_csource_subscription_raw(&http_client, &subscription_id, malformed_json).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest");
    scope.finish().await.expect("cleanup failed");
}
