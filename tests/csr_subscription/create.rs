#![allow(clippy::expect_used, clippy::unwrap_used)]
use crate::{
    ALREADY_EXISTS_TYPE,
    BAD_REQUEST_DATA_TYPE,
    assert_problem_detail_type,
    clean_database,
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
    unique_subscription_id,
};
use chrono::{Duration, SecondsFormat, Utc};
use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn csr_subscription_create_with_id_returns_201_and_location() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-basic");
    let payload = csource_subscription_payload(&subscription_id);

    let (status, location, _) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let location = location.expect("expected Location header");
    assert!(location.contains(&subscription_id), "location should contain subscription id: {location}");
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_without_id_returns_201_and_generated_id() {
    let _serialized_environment = clean_database().await;
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
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_without_is_active_defaults_status_to_active() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-default-active");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_inactive_sets_status_to_paused() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-inactive");
    let payload = inactive_csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["isActive"], false);
    pretty_assertions::assert_eq!(body["status"], "paused");
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_with_expires_at_becomes_expired_after_deadline() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-expiring");
    let expires_at = (Utc::now() + Duration::seconds(3)).to_rfc3339_opts(SecondsFormat::Secs, true);
    let payload = expires_at_csource_subscription_payload(&subscription_id, &expires_at);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(5)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "expired");
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_without_expires_at_remains_active() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-no-expiration");
    let payload = csource_subscription_payload(&subscription_id);

    let (create_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(create_status, StatusCode::CREATED);

    tokio::time::sleep(std::time::Duration::from_secs(3)).await;

    let (retrieve_status, body) = retrieve_csource_subscription(&http_client, &subscription_id, None).await;
    pretty_assertions::assert_eq!(retrieve_status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_duplicate_id_returns_409_already_exists() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-duplicate");
    let payload = csource_subscription_payload(&subscription_id);

    let (first_status, _, _) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(first_status, StatusCode::CREATED);

    let (second_status, _, body) = create_csource_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(second_status, StatusCode::CONFLICT);
    assert_problem_detail_type(&body, ALREADY_EXISTS_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_without_notification_returns_400_bad_request_data() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-missing-notification");
    let payload = csource_subscription_without_notification_payload(&subscription_id);

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_with_invalid_type_returns_400_bad_request_data() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-invalid-type");
    let payload = invalid_type_csource_subscription_payload(&subscription_id);

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_with_invalid_query_returns_400_bad_request_data() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-invalid-query");
    let payload = invalid_query_csource_subscription_payload(&subscription_id);

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_with_empty_watched_attributes_returns_400_bad_request_data() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-empty-watched-attributes");
    let payload = empty_watched_attributes_csource_subscription_payload(&subscription_id);

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}

#[tokio::test]
#[serial]
async fn csr_subscription_create_with_past_expires_at_returns_400_bad_request_data() {
    let _serialized_environment = clean_database().await;
    let http_client = client();
    let subscription_id = unique_subscription_id("create-past-expiration");
    let payload = expires_at_csource_subscription_payload(&subscription_id, "2018-08-01T22:07:00Z");

    let (status, _, body) = create_csource_subscription(&http_client, &payload).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_problem_detail_type(&body, BAD_REQUEST_DATA_TYPE);
}
