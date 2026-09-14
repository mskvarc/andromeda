//! Notification representation: the `concise`, `simplified`, and default
//! `normalized` formats, the `pick`/`omit` member projections, entity `join`,
//! and conformance amendment on delivery (clauses 5.8.6, 4.5.15, 4.5.23). Every
//! row uses a live receiver and asserts the delivered body.

use crate::{
    entity::{entity, property, relationship, vehicle},
    map::{notification_inspection::notified_entities, subscription_document::subscription},
    notification_adapter::{CapturedNotification, NotificationAdapter},
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;

/// Delivery poll budget for a change that should produce a notification.
const DELIVER: Duration = Duration::from_secs(12);

/// The entity carried in the notification with the given id.
///
/// A notification that carries no `data`, or none for this entity, is a failure
/// of the case under test rather than a `Value::Null` for it to assert against:
/// a null propagates through every `["member"]` index and satisfies any
/// assertion phrased as an absence.
fn notified_entity(notification: &CapturedNotification, entity_id: &str) -> Value {
    let absent = format!("the notification carries {entity_id}: {}", notification.body);
    notified_entities(notification)
        .into_iter()
        .find(|entity| entity["id"].as_str() == Some(entity_id))
        .expect(&absent)
}

/// Create a subscription with a `notification.format`, then a `Vehicle`
/// carrying `attributes`, and return that entity as the notification rendered
/// it.
async fn notify_with_format(
    topology: &MultiBrokerTopology<BrokerRole>,
    receiver: &NotificationAdapter,
    local: &str,
    format: &str,
    attributes: &[(&str, Value)],
) -> Value {
    let id = topology.id(&format!("Subscription:{local}"));
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["format"] = json!(format);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
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
            .expect("topology broker A")
            .client()
            .post_entity(&entity(&entity_id, "Vehicle", attributes))
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
///
/// The watched Property carries a sub-Property, which is what separates the
/// three formats: clause 4.5.2.3 renders a Property with sub-attributes as an
/// object whose mandatory member is `value`, whose `type` is left to be
/// inferred from that `value`, and whose sub-attributes are themselves concise,
/// so `speed` arrives as `{"value": 20, "accuracy": 0.5}`. Normalized keeps
/// `"type": "Property"` on both levels and simplified reduces the whole
/// Property to `20`, so each of the three assertions below fails against one of
/// the other two formats.
#[tokio::test]
async fn notif_format_concise_produces_concise_body() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let watched = [(
        "speed",
        json!({ "type": "Property", "value": 20, "accuracy": { "type": "Property", "value": 0.5 } }),
    )];
    let notified = notify_with_format(&topology, &receiver, "concise", "concise", &watched).await;
    assert_eq!(
        notified["speed"]["value"].as_i64(),
        Some(20),
        "clause 4.5.2.3 keeps `value` on a Property that has sub-attributes: {notified}"
    );
    assert!(
        notified["speed"].get("type").is_none(),
        "the concise body drops the explicit Property wrapper: {notified}"
    );
    assert_eq!(
        notified["speed"]["accuracy"].as_f64(),
        Some(0.5),
        "a sub-attribute is itself concise, so it is its bare value: {notified}"
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
    let watched = [("speed", property(json!(20)))];
    let notified = notify_with_format(&topology, &receiver, "simplified", "simplified", &watched).await;
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
        .unwrap_or_else(|| panic!("notification object"))
        .remove("format");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
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
            .expect("topology broker A")
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
            .expect("topology broker A")
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
            .expect("topology broker A")
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
            .expect("topology broker A")
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
            .expect("topology broker A")
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
            .expect("topology broker A")
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
            .expect("topology broker A")
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
            .expect("topology broker A")
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

/// Delivery amends the body to the NGSI-LD version the Subscription's
/// `ngsildConformance` names (clause 5.8.6, the last step before "A
/// Notification shall be sent"; Tables 4.3.6.8-1 and -2). Confirms REQ-MAP-91.
#[tokio::test]
async fn notif_ngsild_conformance_amends_body_to_version() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:conformance");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["ngsildConformance"] = json!("1.0");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    let entity_id = topology.id("Vehicle:conformance");
    let watched = entity(
        &entity_id,
        "Vehicle",
        &[
            // Table 4.3.6.8-2: `observedAt` was introduced in 1.3.
            ("speed", json!({ "type": "Property", "value": 20, "observedAt": "2025-01-01T00:00:00Z" })),
            // Table 4.3.6.8-1: the LanguageProperty was introduced in 1.4.
            ("label", json!({ "type": "LanguageProperty", "languageMap": { "en": "car", "es": "coche" } })),
        ],
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&watched)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let notified = notified_entity(&notification, &entity_id);

    assert!(
        notified["speed"].get("observedAt").is_none(),
        "Table 4.3.6.8-2 removes observedAt for a 1.0 target: {notified}"
    );
    assert_eq!(
        notified["label"]["type"].as_str(),
        Some("Property"),
        "Table 4.3.6.8-1 reformats a LanguageProperty as a Property for a 1.0 target: {notified}"
    );
    assert_eq!(
        notified["label"]["value"],
        json!({ "en": "car", "es": "coche" }),
        "the language map becomes the Property value: {notified}"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
