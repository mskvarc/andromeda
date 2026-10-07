//! Subscription notification eligibility: entity-selector and watched-attribute
//! gating, periodic vs change-driven delivery, expiration, and delete tear-down
//! (clauses 5.8.1, 5.8.2, 5.8.6). Each row drives a real broker change against a
//! live notification receiver.

use crate::{
    entity::{entity, property, typed_vehicle, vehicle},
    map::subscription_document::{set_entity_type, set_watched_attributes, subscription},
    notification_adapter::NotificationAdapter,
    oracle::{assert_status, json_body},
    topology::{self, BrokerRole},
};
use chrono::{Duration as ChronoDuration, Utc};
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Delivery poll budget for a change that should produce a notification.
const DELIVER: Duration = Duration::from_secs(10);

/// Silence window during which no notification must arrive.
const SILENCE: Duration = Duration::from_secs(3);

/// A valid subscription persists and is notification-eligible: a matching entity
/// creation delivers a notification carrying its id (clause 5.8.1).
#[tokio::test]
async fn sub_create_valid_persists_and_is_notification_eligible() {
    let topology = topology::pair().await.expect("two-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:eligible");
    let vtype = topology.id("Vehicle");
    let mut sub = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut sub, &vtype);
    let created = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .create_subscription(&sub)
        .await
        .expect("create subscription");
    assert_status(&created, 201);

    let entity_id = topology.id("Vehicle:eligible");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 40))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );

    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(id.as_str()),
        "the notification names the subscription"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The entity-type selector restricts delivery: a `Vehicle` fires, an unrelated
/// `Device` stays silent (clause 5.8.2).
#[tokio::test]
async fn sub_create_entity_selector_restricts_subscription() {
    let topology = topology::pair().await.expect("two-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:selector");
    let vtype = topology.id("Vehicle");
    let mut sub = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut sub, &vtype);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&sub)
            .await
            .expect("create subscription"),
        201,
    );

    let device_id = topology.id("Device:selector");
    let device = entity(&device_id, "Device", &[("speed", property(json!(5)))]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&device)
            .with_ld_json()
            .send()
            .await
            .expect("create device"),
        201,
    );
    receiver.wait_for_no_subscription_notification(&id, SILENCE).await;

    let vehicle_id = topology.id("Vehicle:selector");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&vehicle_id, &vtype, 40))
            .with_ld_json()
            .send()
            .await
            .expect("create vehicle"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(id.as_str()),
        "only the matching Vehicle fires the subscription"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Absent `watchedAttributes` watches all attributes, so any matching change
/// fires (clause 5.8.2).
#[tokio::test]
async fn sub_create_absent_watched_attributes_watches_all() {
    let topology = topology::pair().await.expect("two-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:watch-all");
    let vtype = topology.id("Vehicle");
    let mut sub = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut sub, &vtype);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&sub)
            .await
            .expect("create subscription"),
        201,
    );

    let entity_id = topology.id("Vehicle:watch-all");
    let body = entity(&entity_id, &vtype, &[("colour", property(json!("red")))]);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&body)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );

    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(id.as_str()),
        "a subscription without watchedAttributes fires on any attribute"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A subscription with `watchedAttributes` but no `entities` spans all holders of
/// the attribute (clause 5.8.2).
#[tokio::test]
async fn sub_create_watched_attributes_no_entities_spans_all_holders() {
    let topology = topology::pair().await.expect("two-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:span-holders");
    let mut body = subscription(&id, &receiver.endpoint_url());
    body.as_object_mut().unwrap_or_else(|| panic!("subscription is an object")).remove("entities");
    set_watched_attributes(&mut body, &["speed"]);
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

    let entity_id = topology.id("Vehicle:span-holders");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&entity_id, 33))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );

    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(id.as_str()),
        "the entity-less subscription fires for any holder of the attribute"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A `timeInterval` subscription notifies periodically regardless of change
/// (clause 5.8.6).
#[tokio::test]
async fn sub_create_time_interval_periodic_regardless_of_change() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:periodic");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 22))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );

    let id = topology.id("Subscription:periodic");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut body, &vtype);
    body["timeInterval"] = json!(1);
    body["notification"]
        .as_object_mut()
        .unwrap_or_else(|| panic!("notification is an object"))
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

    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(id.as_str()),
        "the periodic subscription notifies without any change"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// With no `timeInterval` a subscription is change-driven: silent until a matching
/// change, then it fires (clause 5.8.6).
#[tokio::test]
async fn sub_create_no_time_interval_change_driven() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:change-driven");
    let vtype = topology.id("Vehicle");
    let mut sub = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut sub, &vtype);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&sub)
            .await
            .expect("create subscription"),
        201,
    );

    receiver.wait_for_no_subscription_notification(&id, SILENCE).await;

    let entity_id = topology.id("Vehicle:change-driven");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 55))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(id.as_str()),
        "the change-driven subscription fires only on a change"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Reaching `expiresAt` sets `status` `expired` and stops notifications
/// (clause 5.2.12).
#[tokio::test]
async fn sub_expiration_reached_sets_status_expired_and_stops_notifications() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:expiring");
    let vtype = topology.id("Vehicle");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut body, &vtype);
    body["expiresAt"] = json!((Utc::now() + ChronoDuration::seconds(3)).to_rfc3339());
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

    sleep(Duration::from_secs(5)).await;

    let entity_id = topology.id("Vehicle:expiring");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&entity_id, &vtype, 60))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    receiver.wait_for_no_subscription_notification(&id, SILENCE).await;

    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_subscription(&id, None)
        .await
        .expect("retrieve subscription");
    assert_eq!(
        json_body(&retrieved)["status"].as_str(),
        Some("expired"),
        "an expired subscription reports status expired"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Deleting a subscription removes it and stops further notifications
/// (clause 5.8.1).
#[tokio::test]
async fn sub_delete_removes_and_stops_notifications() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let id = topology.id("Subscription:delete-stops");
    let vtype = topology.id("Vehicle");
    let mut sub = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut sub, &vtype);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&sub)
            .await
            .expect("create subscription"),
        201,
    );

    let first_entity = topology.id("Vehicle:delete-stops-1");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&first_entity, &vtype, 10))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(id.as_str()),
        "the live subscription fires before deletion"
    );

    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .delete_subscription(&id)
            .await
            .expect("delete subscription"),
        204,
    );
    receiver.clear_notifications().await;

    let second_entity = topology.id("Vehicle:delete-stops-2");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&typed_vehicle(&second_entity, &vtype, 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    receiver.wait_for_no_subscription_notification(&id, SILENCE).await;

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
