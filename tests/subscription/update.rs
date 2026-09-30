#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
use chrono::{Duration as ChronoDuration, Utc};
use reqwest::{Method, StatusCode};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

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
    update_fragment,
    update_subscription,
    vehicle_entities_fragment,
    watched_attributes_subscription,
};
use andromeda_test_fixtures::context::{CORE_CONTEXT, TEST_SUITE_CONTEXT};
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE, environment::TestEnvironment};

#[tokio::test]
async fn subscription_update_missing_id_path_returns_method_not_allowed() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
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
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_invalid_id_returns_bad_request_data() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");

    let (status, body) = update_subscription(&http_client, "InvalidUri", &update_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_unknown_id_returns_resource_not_found() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");

    let (status, body) = update_subscription(&http_client, "urn:ngsi-ld:Subscription:unknown", &update_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND);
    pretty_assertions::assert_eq!(body["type"], RESOURCE_NOT_FOUND_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_invalid_fragment_returns_bad_request_data() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-invalid-fragment");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = update_subscription(&http_client, &subscription_id, &invalid_update_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_null_mandatory_properties_returns_bad_request_data() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-null-mandatory-properties");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = update_subscription(&http_client, &subscription_id, &null_properties_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_entities_with_context_returns_no_content() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-entities-with-context");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &vehicle_entities_fragment(), true).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["entities"][0]["type"], "Vehicle");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_entities_without_accept_context_still_updates_entities() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-entities-without-context");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &vehicle_entities_fragment(), true).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["entities"][0]["type"], "Vehicle");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_notification_endpoint_returns_no_content_and_persists_changes() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-endpoint");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &update_fragment(), true).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["notification"]["endpoint"]["uri"], "http://my.second.endpoint.org/notify");
    pretty_assertions::assert_eq!(body["notification"]["format"], "keyValues");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_paused_subscription_with_is_active_true_sets_active_status() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-is-active-true");
    let payload = inactive_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &activate_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_paused_subscription_with_is_active_and_future_expires_sets_active_status() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-active-with-expires");
    let payload = inactive_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &activate_with_expires_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_active_subscription_with_is_active_false_sets_paused_status() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-pause");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, _) = update_subscription(&http_client, &subscription_id, &pause_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "paused");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_expired_subscription_with_future_expires_reactivates_subscription() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-reactivate-expired");
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

    sleep(Duration::from_secs(10)).await;

    let (status, _) = update_subscription(&http_client, &subscription_id, &activate_with_expires_fragment(), false).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["status"], "active");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn subscription_update_with_past_expires_at_returns_bad_request_data() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-past-expires");
    let payload = default_subscription(&subscription_id);

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let (status, body) = update_subscription(&http_client, &subscription_id, &past_expires_fragment(), false).await;

    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST);
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str());
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.2.12 (Table 5.2.12-1) makes `timeInterval` exclusive with both
/// `throttling` and `watchedAttributes`. An update is validated as the merged
/// subscription it would leave behind, so a fragment that is valid alone but
/// conflicts with a stored member is refused as `BadRequestData` and changes
/// nothing, rather than reaching the schema's own check as a server error.
#[tokio::test]
async fn subscription_update_adding_time_interval_to_a_conflicting_subscription_returns_bad_request_data() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");

    let throttled_id = scope.id("Subscription:update-throttled-time-interval");
    let mut throttled = default_subscription(&throttled_id);
    throttled.as_object_mut().expect("subscription object").remove("timeInterval");
    throttled["throttling"] = json!(30);
    let watching_id = scope.id("Subscription:update-watching-time-interval");
    let watching = watched_attributes_subscription(&watching_id);

    for (subscription_id, payload) in [(&throttled_id, &throttled), (&watching_id, &watching)] {
        let (status, _, _) = create_subscription(&http_client, payload).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{subscription_id}");

        let (status, body) = update_subscription(&http_client, subscription_id, &json!({"timeInterval": 5}), true).await;
        pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST, "{subscription_id}: {body}");
        pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "{subscription_id}");

        let (status, stored) = retrieve_subscription(&http_client, subscription_id, true).await;
        pretty_assertions::assert_eq!(status, StatusCode::OK);
        assert!(stored.get("timeInterval").is_none(), "the refused update changed nothing: {stored}");
    }
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-1 gives `notificationTrigger` cardinality 0..1 and makes each
/// member a trigger, so a present but empty array names no trigger at all and
/// the subscription could never fire. It is refused as `BadRequestData` both
/// when a subscription is created with it and when an update would leave it
/// behind, rather than reaching the schema's own non-empty check as a server
/// error.
#[tokio::test]
async fn subscription_an_empty_notification_trigger_is_bad_request_data() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");

    let refused_id = scope.id("Subscription:create-empty-notification-trigger");
    let mut refused = watched_attributes_subscription(&refused_id);
    refused["notificationTrigger"] = json!([]);
    let (status, _, body) = create_subscription(&http_client, &refused).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST, "create: {body}");
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "create: {body}");

    let (status, _) = retrieve_subscription(&http_client, &refused_id, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "the refused subscription was not stored");

    let accepted_id = scope.id("Subscription:update-empty-notification-trigger");
    let mut accepted = watched_attributes_subscription(&accepted_id);
    accepted["notificationTrigger"] = json!(["entityCreated", "entityUpdated"]);
    let (status, _, body) = create_subscription(&http_client, &accepted).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "create: {body}");

    let (status, body) = update_subscription(&http_client, &accepted_id, &json!({"notificationTrigger": []}), true).await;
    pretty_assertions::assert_eq!(status, StatusCode::BAD_REQUEST, "update: {body}");
    pretty_assertions::assert_eq!(body["type"], BAD_REQUEST_DATA_TYPE.as_str(), "update: {body}");

    let (status, stored) = retrieve_subscription(&http_client, &accepted_id, true).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    let mut kept: Vec<&str> = stored["notificationTrigger"]
        .as_array()
        .expect("the stored triggers are an array")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    kept.sort_unstable();
    let kept = kept;
    pretty_assertions::assert_eq!(kept, ["entityCreated", "entityUpdated"], "the refused update changed nothing: {stored}");
    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.8.2.4 and 5.5.8: an update keeps every member its fragment does
/// not name, and the `jsonldContext` is initialised from the request's
/// `@context` only when the subscription is created (clause 5.8.1.4). A PATCH
/// whose `Link` header names another context changes the terms the fragment
/// is read with, never the stored `jsonldContext`.
#[tokio::test]
async fn subscription_update_under_another_link_context_keeps_the_stored_jsonld_context() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("subscription").await.expect("isolated test environment");
    let subscription_id = scope.id("Subscription:update-keeps-jsonld-context");
    let mut payload = default_subscription(&subscription_id);
    payload["jsonldContext"] = json!(CORE_CONTEXT.as_str());

    let (status, _, _) = create_subscription(&http_client, &payload).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let response = http_client
        .patch(format!("{RUNTIME_BROKER_BASE}/subscriptions/{}", urlencoding::encode(&subscription_id)))
        .header("Content-Type", "application/json")
        .header(
            "Link",
            format!("<{TEST_SUITE_CONTEXT}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""),
        )
        .json(&update_fragment())
        .send()
        .await
        .expect("update subscription request failed");
    pretty_assertions::assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (status, body) = retrieve_subscription(&http_client, &subscription_id, false).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(body["jsonldContext"], json!(CORE_CONTEXT.as_str()), "{body}");
    scope.finish().await.expect("cleanup failed");
}
