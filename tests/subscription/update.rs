#![allow(clippy::expect_used, clippy::unwrap_used)]
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::{Method, StatusCode};

use crate::helpers::{
    BAD_REQUEST_DATA_TYPE,
    RESOURCE_NOT_FOUND_TYPE,
    activate_fragment,
    activate_with_expires_fragment,
    client,
    create_subscription,
    default_subscription,
    inactive_subscription,
    invalid_update_fragment,
    null_properties_fragment,
    past_expires_fragment,
    pause_fragment,
    raw_subscription_request,
    retrieve_subscription,
    unique_subscription_id,
    update_fragment,
    update_subscription,
    vehicle_entities_fragment,
};

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_missing_id_path_returns_method_not_allowed() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let fragment = update_fragment();

    let (status, _) = raw_subscription_request(
        &http_client,
        Method::PATCH,
        "/subscriptions/",
        Some(serde_json::to_vec(&fragment).expect("serialize fragment").as_slice()),
        Some("application/json"),
    )
    .await;

    pretty_assertions::assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_invalid_id_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;

    let (status, body) = update_subscription(&http_client, "InvalidUri", &update_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_unknown_id_returns_resource_not_found() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;

    let (status, body) = update_subscription(&http_client, "urn:ngsi-ld:Subscription:unknown", &update_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_invalid_fragment_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-invalid-fragment");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = update_subscription(&http_client, &subscription_id, &invalid_update_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_null_mandatory_properties_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-null-mandatory-properties");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = update_subscription(&http_client, &subscription_id, &null_properties_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_entities_with_context_returns_no_content() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-entities-with-context");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &vehicle_entities_fragment(), true).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["entities"][0]["type"], "Vehicle");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_entities_without_accept_context_still_updates_entities() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-entities-without-context");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &vehicle_entities_fragment(), true).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["entities"][0]["type"], "Vehicle");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_notification_endpoint_returns_no_content_and_persists_changes() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-endpoint");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &update_fragment(), true).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["notification"]["endpoint"]["uri"], "http://my.second.endpoint.org/notify");
    pretty_assertions::assert_eq!(body["notification"]["format"], "keyValues");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_paused_subscription_with_is_active_true_sets_active_status() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-is-active-true");
    let payload = inactive_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &activate_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_paused_subscription_with_is_active_and_future_expires_sets_active_status() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-active-with-expires");
    let payload = inactive_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &activate_with_expires_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_active_subscription_with_is_active_false_sets_paused_status() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-pause");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &pause_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "paused");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_expired_subscription_with_future_expires_reactivates_subscription() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-reactivate-expired");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let expires_at = Utc::now() + ChronoDuration::seconds(5);
    let temporary_expiration = serde_json::json!({
        "type": "Subscription",
        "expiresAt": expires_at.format("%Y-%m-%dT%H:%M:%SZ").to_string()
    });

    let (status, _) = update_subscription(&http_client, &subscription_id, &temporary_expiration, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    let (status, _) = update_subscription(&http_client, &subscription_id, &activate_with_expires_fragment(), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_update_with_past_expires_at_returns_bad_request_data() {
    let http_client = client();
    crate::helpers::reset_broker_state().await;
    let subscription_id = unique_subscription_id("update-past-expires");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = update_subscription(&http_client, &subscription_id, &past_expires_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
}
