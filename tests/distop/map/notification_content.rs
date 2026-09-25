//! Notification content selection: status gating, periodic vs change-driven
//! assembly, filter and dataset restriction, and multi-instance geoproperty
//! matching (clause 5.8.6). All `T-LOCAL` against a live receiver.

use crate::{
    entity::{property, typed_vehicle},
    map::{
        notification_inspection::{notified_entity_ids, notifies_entity},
        subscription_document::{set_entity_type, set_watched_attributes, subscription},
    },
    notification_adapter::NotificationAdapter,
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;

/// Delivery poll budget for a change that should produce a notification.
const DELIVER: Duration = Duration::from_secs(12);

/// Silence window during which no notification must arrive.
const SILENCE: Duration = Duration::from_secs(4);

/// A periodic subscription over `Vehicle` firing every second.
fn periodic(id: &str, uri: &str, entity_type: &str, query: Option<&str>) -> Value {
    let mut body = subscription(id, uri);
    set_entity_type(&mut body, entity_type);
    body["timeInterval"] = json!(1);
    body["notification"]
        .as_object_mut()
        .expect("the subscription carries a notification object")
        .remove("format");
    if let Some(query) = query {
        body["q"] = json!(query);
    }
    body
}

/// Create a `Vehicle` with the given speed and assert the create is accepted.
async fn create_vehicle(client: &TestClient, entity_type: &str, id: &str, speed: i64) {
    assert_status(
        &client
            .post_entity(&typed_vehicle(id, entity_type, speed))
            .with_ld_json()
            .send()
            .await
            .expect("create vehicle"),
        201,
    );
}

/// A paused subscription sends nothing on a matching change (clause 5.8.6).
/// Confirms REQ-MAP-58.
#[tokio::test]
async fn notif_only_sent_when_status_active() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:status-gate");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut body, &vtype);
    body["isActive"] = json!(false);
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
    create_vehicle(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:status-gate"),
        40,
    )
    .await;
    receiver.wait_for_no_subscription_notification(&id, SILENCE).await;
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A periodic subscription notifies without any change (clause 5.8.6).
/// Confirms REQ-MAP-59.
#[tokio::test]
async fn notif_time_interval_periodic_regardless_of_change() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    create_vehicle(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:periodic-any"),
        40,
    )
    .await;
    let id = topology.id("Subscription:periodic-any");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&periodic(&id, &receiver.endpoint_url(), &vtype, None))
            .await
            .expect("create subscription"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(id.as_str()),
        "the periodic subscription notifies without change"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A periodic notification includes only filter-matching entities (clause 5.8.6).
/// Confirms REQ-MAP-60.
#[tokio::test]
async fn notif_periodic_includes_only_filter_matching_entities() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let fast = topology.id("Vehicle:periodic-fast");
    let slow = topology.id("Vehicle:periodic-slow");
    create_vehicle(topology.broker(BrokerRole::A).expect("topology broker A").client(), &vtype, &fast, 100).await;
    create_vehicle(topology.broker(BrokerRole::A).expect("topology broker A").client(), &vtype, &slow, 20).await;
    let id = topology.id("Subscription:periodic-filter");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&periodic(&id, &receiver.endpoint_url(), &vtype, Some("speed>50")))
            .await
            .expect("create subscription"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(notifies_entity(&notification, &fast), "the fast Vehicle matches the filter");
    assert!(!notifies_entity(&notification, &slow), "the slow Vehicle is excluded by the filter");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A periodic notification with no filters includes all subscribed entities
/// (clause 5.8.6). Confirms REQ-MAP-61.
#[tokio::test]
async fn notif_periodic_no_filters_includes_all_subscribed() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let first = topology.id("Vehicle:periodic-all-1");
    let second = topology.id("Vehicle:periodic-all-2");
    create_vehicle(topology.broker(BrokerRole::A).expect("topology broker A").client(), &vtype, &first, 10).await;
    create_vehicle(topology.broker(BrokerRole::A).expect("topology broker A").client(), &vtype, &second, 20).await;
    let id = topology.id("Subscription:periodic-all");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&periodic(&id, &receiver.endpoint_url(), &vtype, None))
            .await
            .expect("create subscription"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(
        notifies_entity(&notification, &first) && notifies_entity(&notification, &second),
        "both subscribed Vehicles are included: {:?}",
        notified_entity_ids(&notification)
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A periodic notification includes only the subscription's `datasetId`
/// instances (clause 5.8.6). Confirms REQ-MAP-62.
#[tokio::test]
async fn notif_periodic_includes_only_subscription_dataset_id_instances() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:periodic-dataset");
    let entity = json!({
        "id": entity_id,
        "type": vtype,
        "speed": [
            { "type": "Property", "value": 10, "datasetId": "urn:ngsi-ld:ds:one" },
            { "type": "Property", "value": 20, "datasetId": "urn:ngsi-ld:ds:two" }
        ],
        "@context": COMPOUND_CONTEXT.as_str()
    });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&entity)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let id = topology.id("Subscription:periodic-dataset");
    let mut body = periodic(&id, &receiver.endpoint_url(), &vtype, None);
    body["datasetId"] = json!(["urn:ngsi-ld:ds:one"]);
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
    assert!(
        notifies_entity(&notification, &entity_id),
        "the entity holding the subscribed dataset instance is notified"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A periodic subscription with zero matches sends nothing (clause 5.8.6).
/// Confirms REQ-MAP-63.
#[tokio::test]
async fn notif_periodic_zero_matches_sends_nothing() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    create_vehicle(
        topology.broker(BrokerRole::A).expect("topology broker A").client(),
        &vtype,
        &topology.id("Vehicle:periodic-zero"),
        10,
    )
    .await;
    let id = topology.id("Subscription:periodic-zero");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&periodic(&id, &receiver.endpoint_url(), &vtype, Some("speed>9999")))
            .await
            .expect("create subscription"),
        201,
    );
    receiver.wait_for_no_subscription_notification(&id, SILENCE).await;
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Without a `timeInterval`, a watched-attribute change drives a notification
/// (clause 5.8.6). Confirms REQ-MAP-64.
#[tokio::test]
async fn notif_no_time_interval_change_driven_on_watched_change() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:change-watched");
    create_vehicle(topology.broker(BrokerRole::A).expect("topology broker A").client(), &vtype, &entity_id, 10).await;
    let id = topology.id("Subscription:change-watched");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut body, &vtype);
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
    let fragment = json!({ "speed": property(json!(99)), "@context": COMPOUND_CONTEXT.as_str() });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_entity_attrs(&entity_id, &fragment)
            .await
            .expect("update speed"),
        204,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(notifies_entity(&notification, &entity_id), "the watched-attribute change is notified");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A change-driven notification fires only on an actual value difference
/// (clause 5.8.6). Confirms REQ-MAP-65.
#[tokio::test]
async fn notif_change_only_on_actual_value_difference() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:change-diff");
    create_vehicle(topology.broker(BrokerRole::A).expect("topology broker A").client(), &vtype, &entity_id, 50).await;
    let id = topology.id("Subscription:change-diff");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut body, &vtype);
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

    let same = json!({ "speed": property(json!(50)), "@context": COMPOUND_CONTEXT.as_str() });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_entity_attrs(&entity_id, &same)
            .await
            .expect("no-op update"),
        204,
    );
    receiver.wait_for_no_subscription_notification(&id, SILENCE).await;

    let different = json!({ "speed": property(json!(70)), "@context": COMPOUND_CONTEXT.as_str() });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_entity_attrs(&entity_id, &different)
            .await
            .expect("value change"),
        204,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(notifies_entity(&notification, &entity_id), "only the real value change is notified");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A change-driven notification includes the changed, filter-matching entity
/// (clause 5.8.6). Confirms REQ-MAP-66.
#[tokio::test]
async fn notif_change_driven_includes_changed_filter_matching() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:change-filter");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut body, &vtype);
    body["q"] = json!("speed>50");
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
    let entity_id = topology.id("Vehicle:change-filter");
    create_vehicle(topology.broker(BrokerRole::A).expect("topology broker A").client(), &vtype, &entity_id, 100).await;
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(notifies_entity(&notification, &entity_id), "the changed, filter-matching entity is notified");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A change-driven notification with no filters includes any changed entity
/// (clause 5.8.6). Confirms REQ-MAP-67.
#[tokio::test]
async fn notif_change_driven_no_filters_includes_all_changed() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:change-nofilter");
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
    let entity_id = topology.id("Vehicle:change-nofilter");
    create_vehicle(topology.broker(BrokerRole::A).expect("topology broker A").client(), &vtype, &entity_id, 15).await;
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(notifies_entity(&notification, &entity_id), "the changed entity is notified");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Any matching instance of a multi-instance geoproperty qualifies the entity
/// (clause 5.8.6). Confirms REQ-MAP-68.
#[tokio::test]
async fn notif_multi_instance_geoproperty_any_matching_instance_qualifies() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:geo-multi");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut body, &vtype);
    body["geoQ"] = json!({ "georel": "near;maxDistance==2000", "geometry": "Point", "coordinates": [0, 0] });
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
    let entity_id = topology.id("Vehicle:geo-multi");
    let entity = json!({
        "id": entity_id,
        "type": vtype,
        "location": [
            { "type": "GeoProperty", "value": { "type": "Point", "coordinates": [0, 0] }, "datasetId": "urn:ngsi-ld:ds:here" },
            { "type": "GeoProperty", "value": { "type": "Point", "coordinates": [80, 80] }, "datasetId": "urn:ngsi-ld:ds:far" }
        ],
        "@context": COMPOUND_CONTEXT.as_str()
    });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&entity)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(
        notifies_entity(&notification, &entity_id),
        "one matching geoproperty instance qualifies the entity"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A change-driven notification includes only the subscription's `datasetId`
/// instances (clause 5.8.6). Confirms REQ-MAP-69.
#[tokio::test]
async fn notif_change_driven_includes_only_dataset_id_instances() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:change-dataset");
    let mut body = subscription(&id, &receiver.endpoint_url());
    set_entity_type(&mut body, &vtype);
    body["datasetId"] = json!(["urn:ngsi-ld:ds:one"]);
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
    let entity_id = topology.id("Vehicle:change-dataset");
    let entity = json!({
        "id": entity_id,
        "type": vtype,
        "speed": { "type": "Property", "value": 40, "datasetId": "urn:ngsi-ld:ds:one" },
        "@context": COMPOUND_CONTEXT.as_str()
    });
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&entity)
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let notification = receiver.wait_for_subscription_notification(&id, DELIVER).await;
    assert!(
        notifies_entity(&notification, &entity_id),
        "the entity holding the subscribed dataset instance is notified"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
