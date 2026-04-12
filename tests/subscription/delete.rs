use reqwest::{Method, StatusCode};

use crate::helpers::{
    BAD_REQUEST_DATA_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    client,
    create_subscription,
    default_subscription,
    delete_subscription,
    raw_subscription_request,
    retrieve_subscription,
    unique_subscription_id,
};

#[serial_test::serial]
#[tokio::test]
async fn subscription_delete_missing_id_path_returns_method_not_allowed() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;

    let (status, _) = raw_subscription_request(&http_client, Method::DELETE, "/subscriptions/", None, None).await;

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_delete_invalid_id_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;

    let (status, body_bytes) = delete_subscription(&http_client, "InvalidUri").await;
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or_default();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE);
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_delete_unknown_id_returns_resource_not_found() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;

    let (status, body_bytes) = delete_subscription(&http_client, "urn:ngsi-ld:Subscription:unknown").await;
    let body: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap_or_default();

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE);
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_delete_existing_id_returns_no_content_and_removes_subscription() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("delete-existing");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body_bytes) = delete_subscription(&http_client, &subscription_id).await;

    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body_bytes.is_empty());

    let (status, _) = retrieve_subscription(&http_client, &subscription_id, false).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
