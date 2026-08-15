#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use crate::{
    BAD_REQUEST_DATA_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    assert_problem_detail_type,
    clean_database,
    client,
    create_csource_subscription,
    csource_subscription_payload,
    delete_csource_subscription,
    retrieve_csource_subscription,
    unique_subscription_id,
};
use reqwest::StatusCode;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn csr_subscription_delete_existing_returns_204_and_resource_not_found_afterwards() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("delete-existing");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let (delete_status, _) = delete_csource_subscription(&http_client, &subscription_id).await;
    pretty_assertions::assert_eq!(delete_status, StatusCode::NO_CONTENT);

    let (retrieve_status, _) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial]
async fn csr_subscription_delete_invalid_uri_returns_400_bad_request_data() {
    let _serialized_environment = clean_database().await;
    let http_client = client();

    let (status, body) = delete_csource_subscription(&http_client, "invalidUri").await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_delete_unknown_id_returns_404_resource_not_found() {
    let _serialized_environment = clean_database().await;
    let http_client = client();

    let (status, body) = delete_csource_subscription(&http_client, "urn:ngsi-ld:Subscription:unknownSubscription").await;

    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    assert_problem_detail_type(&body, RESOURCE_NOT_FOUND_TYPE);
}
