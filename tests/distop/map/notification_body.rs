//! Notification body shape: the `Notification` structure, the context source,
//! attribute projection, system attributes, and the null-URI rendering of
//! deleted members (clauses 5.8.6, 5.2.25, 4.5.6). All `T-LOCAL` against a live
//! receiver.

use crate::{
    entity::{entity, property, vehicle},
    map::{
        notification_inspection::notified_entities,
        subscription_document::{set_notification_attributes, subscription},
    },
    notification_adapter::{CapturedNotification, NotificationAdapter},
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use andromeda_test_client::entity_attribute::DatasetSelection;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::{Value, json};
use std::time::Duration;

/// Delivery poll budget for a change that should produce a notification.
const DELIVER: Duration = Duration::from_secs(12);

/// The NGSI-LD null sentinel used to render deleted members (clause 4.5.6).
const NULL_URI: &str = "urn:ngsi-ld:null";

/// The entity carried in the notification with the given id.
fn notified_entity(notification: &CapturedNotification, entity_id: &str) -> Value {
    notified_entities(notification)
        .into_iter()
        .find(|entity| entity["id"].as_str() == Some(entity_id))
        .unwrap_or(Value::Null)
}

/// The notification body conforms to the `Notification` structure (clause 5.2.25).
/// Confirms REQ-MAP-76.
#[tokio::test]
async fn notif_body_conforms_to_notification_structure() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:structure");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&subscription(&id, &receiver.endpoint_url()))
            .await
            .expect("create subscription"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:structure"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(notification.body["type"].as_str(), Some("Notification"), "type is Notification");
    assert!(notification.body["id"].as_str().is_some(), "the notification carries an id");
    assert!(notification.body["notifiedAt"].as_str().is_some(), "the notification carries notifiedAt");
    assert!(notification.body["data"].is_array(), "the notification carries a data array");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The notification body carries the context from the subscription's context
/// when delivered as `application/ld+json` (clause 5.8.6). Confirms REQ-MAP-77.
#[tokio::test]
async fn notif_context_from_subscription_jsonld_context() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:context");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["endpoint"]["accept"] = json!("application/ld+json");
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
            .post_entity(&vehicle(&topology.id("Vehicle:context"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(
        notification.body.get("@context").is_some(),
        "the ld+json notification embeds the context: {}",
        notification.body
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The notification limits attributes to `notification.attributes` (clause 5.8.6).
/// Confirms REQ-MAP-78.
#[tokio::test]
async fn notif_attributes_limited_to_notification_attributes() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:attrs-limited");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_notification_attributes(&mut body, &["speed"]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    let entity_id = topology.id("Vehicle:attrs-limited");
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
    assert!(notified["speed"].get("value").is_some(), "the picked attribute is present: {notified}");
    assert!(notified["colour"].is_null(), "the unpicked attribute is omitted: {notified}");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Absent `notification.attributes` includes all attributes (clause 5.8.6).
/// Confirms REQ-MAP-79.
#[tokio::test]
async fn notif_absent_notification_attributes_includes_all() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:attrs-all");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&subscription(&id, &receiver.endpoint_url()))
            .await
            .expect("create subscription"),
        201,
    );
    let entity_id = topology.id("Vehicle:attrs-all");
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
    assert!(
        notified["speed"].get("value").is_some() && notified["colour"].get("value").is_some(),
        "all attributes are included: {notified}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// With `showChanges` off, a deletion notification carries only `deletedAt`
/// (clause 5.8.6). Confirms REQ-MAP-80.
#[tokio::test]
async fn notif_deletion_show_changes_off_only_deleted_at() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let entity_id = topology.id("Vehicle:deletion-off");
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
    let id = topology.id("Subscription:deletion-off");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notificationTrigger"] = json!(["entityDeleted"]);
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
        &topology.broker(BrokerRole::A).client().delete_entity(&entity_id).await.expect("delete entity"),
        204,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let notified = notified_entity(&notification, &entity_id);
    assert!(
        notified["deletedAt"].as_str().is_some(),
        "the deletion notification carries deletedAt: {notified}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// `sysAttrs` true includes `createdAt` and `modifiedAt` (clause 5.8.6).
/// Confirms REQ-MAP-81.
#[tokio::test]
async fn notif_sys_attrs_true_includes_created_and_modified() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:sys-attrs");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["sysAttrs"] = json!(true);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .create_subscription(&body)
            .await
            .expect("create subscription"),
        201,
    );
    let entity_id = topology.id("Vehicle:sys-attrs");
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
    assert!(notified["createdAt"].as_str().is_some(), "sysAttrs true includes createdAt: {notified}");
    assert!(notified["modifiedAt"].as_str().is_some(), "sysAttrs true includes modifiedAt: {notified}");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A deleted attribute is rendered with the null URI when `showChanges` is on
/// (clause 4.5.6). Confirms REQ-MAP-82.
#[tokio::test]
async fn notif_deleted_attribute_rendered_as_null_uri() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let entity_id = topology.id("Vehicle:deleted-null");
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
    let id = topology.id("Subscription:deleted-null");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["showChanges"] = json!(true);
    body["notificationTrigger"] = json!(["attributeDeleted"]);
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
            .delete_entity_attr(&entity_id, "colour", None, DatasetSelection::Default, Some(COMPOUND_CONTEXT.as_str()))
            .await
            .expect("delete attribute"),
        204,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let notified = notified_entity(&notification, &entity_id);
    assert!(
        notified.to_string().contains(NULL_URI),
        "the deleted attribute is rendered with the null URI: {notified}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A deleted attribute renders as a full object with the null URI when more
/// information is needed (clause 4.5.6). Confirms REQ-MAP-83.
#[tokio::test]
async fn notif_deleted_attribute_full_object_null_uri_when_more_info_needed() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let entity_id = topology.id("Vehicle:deleted-object-null");
    let observed = json!({ "type": "Property", "value": 20, "observedAt": "2021-01-01T00:00:00Z" });
    let with_two = entity(&entity_id, "Vehicle", &[("speed", observed), ("colour", property(json!("red")))]);
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
    let id = topology.id("Subscription:deleted-object-null");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["showChanges"] = json!(true);
    body["notificationTrigger"] = json!(["attributeDeleted"]);
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
            .delete_entity_attr(&entity_id, "speed", None, DatasetSelection::Default, Some(COMPOUND_CONTEXT.as_str()))
            .await
            .expect("delete attribute"),
        204,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let notified = notified_entity(&notification, &entity_id);
    assert!(
        notified.to_string().contains(NULL_URI),
        "the deleted attribute object carries the null URI: {notified}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A deleted language property renders its language map with the null URI
/// (clause 4.5.6). Confirms REQ-MAP-84.
#[tokio::test]
async fn notif_deleted_language_property_language_map_null_uri() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let entity_id = topology.id("Vehicle:deleted-language");
    let language = json!({ "type": "LanguageProperty", "languageMap": { "en": "Main Street", "fr": "Rue Principale" } });
    let with_language = entity(&entity_id, "Vehicle", &[("speed", property(json!(20))), ("street", language)]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .client()
            .post_entity(&with_language)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let id = topology.id("Subscription:deleted-language");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body["notification"]["showChanges"] = json!(true);
    body["notificationTrigger"] = json!(["attributeDeleted"]);
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
            .delete_entity_attr(&entity_id, "street", None, DatasetSelection::Default, Some(COMPOUND_CONTEXT.as_str()))
            .await
            .expect("delete language property"),
        204,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    let notified = notified_entity(&notification, &entity_id);
    assert!(
        notified.to_string().contains(NULL_URI),
        "the deleted language property is rendered with the null URI: {notified}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
