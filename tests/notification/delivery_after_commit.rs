//! A delivered subscription notification reflects state the triggering write has
//! already committed durably.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.10.1: a notification carries the entities
//! matching the subscription as they are after the change that triggered it, and
//! clause 5.8.1 ties the notification to that change having occurred. Andromeda's
//! durable side-effect drain publishes a change only after the write's side
//! effect is acknowledged committed, so a delivered notification never carries a
//! value the write has not made durable. This asserts that ordering black-box:
//! the triggering append returns success first, the notification then arrives
//! carrying the appended value, and a fresh retrieval of the entity shows the
//! same committed value the notification carried.
//!
//! Naming convention: `notification_<feature>_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use std::time::Duration;

use andromeda_test_harness::configuration::RUNTIME_BROKER_BASE as BASE_URL;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

use crate::notification_adapter::NotificationAdapter as NotificationReceiver;

use crate::helpers::{
    append_entity_attrs,
    building_simple_entity,
    client,
    create_entity,
    create_subscription,
    fragment_air_quality_level,
    reset_broker_state,
    set_endpoint_uri,
    subscription_with_endpoint,
    unique_entity_id,
    unique_subscription_id,
    with_is_active,
};

/// Retrieve one entity in the default (normalized) representation.
async fn retrieve_entity(http_client: &Client, entity_id: &str) -> (StatusCode, Value) {
    let encoded_id = urlencoding::encode(entity_id);
    let response = http_client
        .get(format!("{BASE_URL}/entities/{encoded_id}"))
        .header("Accept", "application/json")
        .send()
        .await
        .expect("retrieve entity request failed");
    let status = response.status();
    let body = response.json::<Value>().await.unwrap_or(Value::Null);
    (status, body)
}

/// The `airQualityLevel` value the notification carries for `entity_id`, if any.
fn notified_air_quality(notification: &Value, entity_id: &str) -> Option<i64> {
    let entities = notification["data"].as_array()?;
    let entity = entities.iter().find(|entity| entity["id"] == json!(entity_id))?;
    entity["airQualityLevel"]["value"].as_i64()
}

/// Clause 5.10.1 / 5.8.1: the value a notification carries is durably retrievable
/// -- the drain never publishes a change the triggering write has not committed.
#[serial_test::serial]
#[tokio::test]
async fn notification_reflects_committed_state_of_triggering_write() {
    let http_client = client();
    let _serialized_environment = reset_broker_state().await;
    let receiver = NotificationReceiver::start().await;

    let entity_id = unique_entity_id("urn:ngsi-ld:Building:delivery-after-commit");
    let (status, _, _) = create_entity(&http_client, &building_simple_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let subscription_id = unique_subscription_id("delivery-after-commit");
    let mut subscription = with_is_active(subscription_with_endpoint(&subscription_id, &receiver.endpoint_url()), true);
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, _) = create_subscription(&http_client, &subscription).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    receiver.clear_notifications().await;

    // The triggering write acknowledges success before we look for its
    // notification: the notification is a consequence of a committed write.
    let (status, _) = append_entity_attrs(&http_client, &entity_id, &fragment_air_quality_level()).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT);

    let notification = receiver.wait_for_subscription_notification(&subscription_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(notification.body["subscriptionId"], subscription_id);
    pretty_assertions::assert_eq!(
        notified_air_quality(&notification.body, &entity_id),
        Some(5),
        "the notification carries the appended value"
    );

    let (status, body) = retrieve_entity(&http_client, &entity_id).await;
    pretty_assertions::assert_eq!(status, StatusCode::OK);
    pretty_assertions::assert_eq!(
        body["airQualityLevel"]["value"].as_i64(),
        Some(5),
        "the retrieved entity reflects the same committed value the notification carried"
    );
}
