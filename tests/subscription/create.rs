#![allow(clippy::expect_used, clippy::unwrap_used)]
use reqwest::{Method, StatusCode};
use serde_json::json;

use crate::helpers::{
    ALREADY_EXISTS_TYPE,
    BAD_REQUEST_DATA_TYPE,
    INVALID_REQUEST_TYPE,
    LD_CONTEXT_NOT_AVAILABLE_TYPE,
    client,
    create_subscription,
    dataset_id_subscription,
    default_subscription,
    invalid_jsonld_context_subscription,
    invalid_negative_throttling_subscription,
    invalid_throttling_and_time_interval_subscription,
    raw_subscription_request,
    unavailable_jsonld_context_subscription,
    unique_subscription_id,
};

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_valid_payload_returns_created_and_retrievable() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("create-valid");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_invalid_json_returns_bad_request() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let raw = r#"{"id":"urn:ngsi-ld:Subscription:invalid",,"type":"Subscription"}"#;

    let (status, body) = raw_subscription_request(&http_client, Method::POST, "/subscriptions", Some(raw.as_bytes()), Some("application/ld+json")).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], INVALID_REQUEST_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_empty_payload_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;

    let (status, _, body) = create_subscription(&http_client, &json!({})).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_invalid_id_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let mut payload = default_subscription("invalidId");
    payload.as_object_mut().expect("payload object").remove("timeInterval");

    let (status, _, body) = create_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_empty_id_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let mut payload = default_subscription("");
    payload.as_object_mut().expect("payload object").remove("timeInterval");

    let (status, _, body) = create_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_duplicate_id_returns_conflict() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("create-duplicate");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _, body) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CONFLICT);
    pretty_assertions::assert_eq!(body["type"], ALREADY_EXISTS_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_throttling_and_time_interval_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("create-invalid-throttling-time-interval");
    let payload = invalid_throttling_and_time_interval_subscription(&subscription_id);

    let (status, _, body) = create_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_negative_throttling_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("create-invalid-negative-throttling");
    let payload = invalid_negative_throttling_subscription(&subscription_id);

    let (status, _, body) = create_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_with_dataset_id_returns_created() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("create-with-dataset-id");
    let payload = dataset_id_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_invalid_jsonld_context_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("create-invalid-jsonld-context");
    let payload = invalid_jsonld_context_subscription(&subscription_id);

    let (status, _, body) = create_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_create_unavailable_jsonld_context_returns_gateway_timeout() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("create-unavailable-jsonld-context");
    let payload = unavailable_jsonld_context_subscription(&subscription_id);

    let (status, _, body) = create_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    pretty_assertions::assert_eq!(body["type"], LD_CONTEXT_NOT_AVAILABLE_TYPE.as_str());
}
