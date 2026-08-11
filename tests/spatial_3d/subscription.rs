//! Altitude through subscription matching and notification delivery
//! (NGSI-LD 5.8.1, 5.8.6).
//!
//! A subscription `geoQ` is evaluated with the same 2D footprint semantics as a
//! query, and the notification payload carries the entity representation
//! unchanged — altitude included.
//!
//! These tests need an empty broker so that unrelated subscriptions cannot
//! observe the fixtures, so they take the serialized environment lock.

use crate::notification_adapter::NotificationAdapter as NotificationReceiver;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::serialized_environment::SerializedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// How long to wait for a notification before declaring the test failed.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(10);

/// A subscription on `Building` entities with the supplied `geoQ` and endpoint.
fn geo_subscription(id: &str, endpoint: &str, geo_q: Value, format: Option<&str>) -> Value {
    let mut notification = json!({"endpoint": {"uri": endpoint, "accept": "application/json"}});
    if let Some(value) = format {
        notification["format"] = json!(value);
    }
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "geoQ": geo_q,
        "notification": notification,
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// A `Building` with a three-dimensional location.
fn building_3d(id: &str, altitude: f64) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52, altitude]}},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Creates a subscription and asserts the broker accepted it.
async fn create_subscription(client: &TestClient, subscription: &Value) {
    let response = client.create_subscription(subscription).await.expect("create subscription failed");
    assert_eq!(response.status, StatusCode::CREATED, "subscription create must succeed");
}

/// Creates an entity and asserts the broker accepted it.
async fn create_entity(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(response.status, StatusCode::CREATED, "entity create must succeed");
}

/// Extracts the first notified entity from a notification payload.
fn first_notified_entity(notification: &Value) -> &Value {
    notification.pointer("/data/0").expect("notification must carry at least one entity in data")
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_geoq_near_3d_reference_matches_horizontally() {
    let environment = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = environment.client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = "urn:ngsi-ld:Subscription:spatial3d-near";
    let geo_q = json!({
        "georel": "near;maxDistance==100",
        "geometry": "Point",
        "coordinates": [13.4, 52.52, 0.0]
    });
    create_subscription(client, &geo_subscription(subscription_id, &receiver.endpoint_url(), geo_q, None)).await;
    receiver.clear_notifications().await;

    // 500 m of vertical separation must not defeat a 100 m horizontal radius.
    let entity_id = "urn:ngsi-ld:Building:spatial3d-near";
    create_entity(client, &building_3d(entity_id, 500.0)).await;

    let notification = receiver.wait_for_subscription_notification(subscription_id, NOTIFICATION_TIMEOUT).await;
    assert_eq!(notification.body["subscriptionId"], json!(subscription_id));
    let entity = first_notified_entity(&notification.body);
    assert_eq!(entity["id"], json!(entity_id));
    assert_eq!(
        entity
            .pointer("/location/value/coordinates")
            .expect("notified entity must carry location coordinates"),
        &json!([13.4, 52.52, 500.0])
    );

    receiver.shutdown().await;
    environment.finish().await.expect("cleanup failed");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_geoq_within_3d_polygon_matches_2d_entity() {
    let environment = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = environment.client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = "urn:ngsi-ld:Subscription:spatial3d-within";
    let geo_q = json!({
        "georel": "within",
        "geometry": "Polygon",
        "coordinates": [[
            [13.0, 52.0, 250.0],
            [14.0, 52.0, 250.0],
            [14.0, 53.0, 250.0],
            [13.0, 53.0, 250.0],
            [13.0, 52.0, 250.0]
        ]]
    });
    create_subscription(client, &geo_subscription(subscription_id, &receiver.endpoint_url(), geo_q, None)).await;
    receiver.clear_notifications().await;

    let entity_id = "urn:ngsi-ld:Building:spatial3d-within";
    let entity = json!({
        "id": entity_id,
        "type": "Building",
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.52]}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    create_entity(client, &entity).await;

    let notification = receiver.wait_for_subscription_notification(subscription_id, NOTIFICATION_TIMEOUT).await;
    assert_eq!(notification.body["subscriptionId"], json!(subscription_id));
    assert_eq!(first_notified_entity(&notification.body)["id"], json!(entity_id));

    receiver.shutdown().await;
    environment.finish().await.expect("cleanup failed");
}

#[serial_test::serial]
#[tokio::test]
async fn subscription_notification_keyvalues_format_keeps_altitude() {
    let environment = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let client = environment.client();
    let receiver = NotificationReceiver::start().await;

    let subscription_id = "urn:ngsi-ld:Subscription:spatial3d-keyvalues";
    let geo_q = json!({
        "georel": "near;maxDistance==100",
        "geometry": "Point",
        "coordinates": [13.4, 52.52]
    });
    let subscription = geo_subscription(subscription_id, &receiver.endpoint_url(), geo_q, Some("keyValues"));
    create_subscription(client, &subscription).await;
    receiver.clear_notifications().await;

    let entity_id = "urn:ngsi-ld:Building:spatial3d-keyvalues";
    create_entity(client, &building_3d(entity_id, 300.0)).await;

    let notification = receiver.wait_for_subscription_notification(subscription_id, NOTIFICATION_TIMEOUT).await;
    let entity = first_notified_entity(&notification.body);
    assert_eq!(entity["id"], json!(entity_id));
    // NGSI-LD 5.8.6: a keyValues notification carries the bare geometry.
    let coordinates = entity
        .pointer("/location/coordinates")
        .and_then(Value::as_array)
        .expect("keyValues notification must carry the bare location geometry");
    assert_eq!(coordinates.len(), 3, "the altitude must survive the keyValues projection");

    receiver.shutdown().await;
    environment.finish().await.expect("cleanup failed");
}
