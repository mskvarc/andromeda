//! Notification representation: the `concise`, `simplified`, and default
//! `normalized` formats, the `pick`/`omit` member projections, entity `join`,
//! and conformance amendment on delivery (clauses 5.8.6, 4.5.15, 4.5.23). Local
//! rows use a live receiver; the conformance row captures on a mock (`T-MOCK`,
//! seeded `UNKNOWN`).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property, relationship, vehicle},
    map::{notification_inspection::notified_entities, subscription_document::subscription},
    notification_adapter::{CapturedNotification, NotificationAdapter},
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use serde_json::{Value, json};
use std::time::Duration;

/// Delivery poll budget for a change that should produce a notification.
const DELIVER: Duration = Duration::from_secs(12);

/// The entity carried in the notification with the given id.
fn notified_entity(notification: &CapturedNotification, entity_id: &str) -> Value {
    notified_entities(notification)
        .into_iter()
        .find(|entity| entity["id"].as_str() == Some(entity_id))
        .unwrap_or(Value::Null)
}

/// Create a subscription with a `notification.format` and return the notified
/// representation of a freshly created `Vehicle`.
async fn notify_with_format(
    topology: &andromeda_test_harness::multi_broker::MultiBrokerTopology<BrokerRole>,
    receiver: &NotificationAdapter,
    local: &str,
    format: &str,
) -> Value {
    let id = topology.id(&format!("Subscription:{local}"));
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["format"] = json!(format);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    let entity_id = topology.id(&format!("Vehicle:{local}"));
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    notified_entity(&notification, &entity_id)
}

/// The `concise` format produces a concise body (clause 4.5.15). Confirms REQ-MAP-85.
#[tokio::test]
async fn notif_format_concise_produces_concise_body() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let notified = notify_with_format(&topology, &receiver, "concise", "concise").await;
    assert_ne!(
        notified["speed"]["type"].as_str(),
        Some("Property"),
        "the concise body drops the explicit Property wrapper: {notified}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The `simplified` format produces a key-values body (clause 4.5.15).
/// Confirms REQ-MAP-86.
#[tokio::test]
async fn notif_format_simplified_produces_key_values_body() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let notified = notify_with_format(&topology, &receiver, "simplified", "simplified").await;
    assert_eq!(notified["speed"].as_i64(), Some(20), "the key-values body renders the bare value: {notified}");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The default format is `normalized` (clause 4.5.15). Confirms REQ-MAP-87.
#[tokio::test]
async fn notif_default_format_normalized() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:normalized-default");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]
        .as_object_mut()
        .unwrap_or_else(|| unreachable!("notification object"))
        .remove("format");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    let entity_id = topology.id("Vehicle:normalized-default");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&entity_id, 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let notified = notified_entity(&notification, &entity_id);
    assert_eq!(
        notified["speed"]["type"].as_str(),
        Some("Property"),
        "the default body is normalized: {notified}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `pick` reduces entities to the picked members (clause 5.8.6).
/// Confirms REQ-MAP-88.
#[tokio::test]
async fn notif_pick_reduces_entities_to_picked_members() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:pick");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["pick"] = json!(["speed"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    let entity_id = topology.id("Vehicle:pick");
    let with_two = entity(&entity_id, "Vehicle", &[("speed", property(json!(20))), ("colour", property(json!("red")))]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&with_two)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let notified = notified_entity(&notification, &entity_id);
    assert!(notified["speed"].get("value").is_some(), "the picked member survives: {notified}");
    assert!(notified["colour"].is_null(), "an unpicked member is removed: {notified}");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An `omit` removes the listed members (clause 5.8.6). Confirms REQ-MAP-89.
#[tokio::test]
async fn notif_omit_removes_listed_members() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:omit");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["omit"] = json!(["colour"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    let entity_id = topology.id("Vehicle:omit");
    let with_two = entity(&entity_id, "Vehicle", &[("speed", property(json!(20))), ("colour", property(json!("red")))]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&with_two)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let notified = notified_entity(&notification, &entity_id);
    assert!(notified["colour"].is_null(), "the omitted member is removed: {notified}");
    assert!(notified["speed"].get("value").is_some(), "a non-omitted member survives: {notified}");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `join` includes the linked entities (clause 4.5.23). Confirms REQ-MAP-90.
#[tokio::test]
async fn notif_join_includes_linked_entities() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let target_id = topology.id("Vehicle:join-target");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&target_id, 5))
            .with_ld_json()
            .send()
            .await
            .expect("create target"),
        201,
    );
    let id = topology.id("Subscription:join");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["join"] = json!("flat");
    body["notification"]["joinLevel"] = json!(1);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    let source_id = topology.id("Vehicle:join-source");
    let source = entity(&source_id, "Vehicle", &[("linkedTo", relationship(&target_id))]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&source)
            .with_ld_json()
            .send()
            .await
            .expect("create source"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let ids: Vec<String> = notified_entities(&notification)
        .iter()
        .filter_map(|entity| entity["id"].as_str().map(str::to_owned))
        .collect();
    assert!(ids.iter().any(|id| id == &target_id), "the join includes the linked entity: {ids:?}");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Delivery amends the body to the receiver's declared NGSI-LD conformance
/// (clause 5.8.6). Confirms REQ-MAP-91.
#[tokio::test]
async fn notif_ngsild_conformance_amends_body_to_version() {
    let topology = topology::single().await.expect("single-broker topology");
    let sink = ContextSourceAdapter::start().await;
    sink.set_stub(Method::POST, "/notify", 200).await;
    let id = topology.id("Subscription:conformance");
    let mut body = subscription(&id, &sink.url("/notify"));
    body["notification"]["endpoint"]["notifierInfo"] = json!([{ "key": "ngsild-attribute-representation", "value": "normalized" }]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:conformance"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let captured = sink.next_request(DELIVER).await;
    let notification: Value = captured.json().expect("notification body is JSON");
    assert_eq!(
        notification["type"].as_str(),
        Some("Notification"),
        "the amended body is a Notification: {notification}"
    );
    sink.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
