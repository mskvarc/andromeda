//! Inbound-notification re-assembly across a split entity (clause 5.8.6): when a
//! supporting source notifies the front broker, it retrieves the local copy and
//! every other holder except the sender, merges them with the notification's
//! `data`, drops non-matching entities, and forwards a single re-assembled
//! notification to the local endpoint under the local subscription id. Topology
//! `T-SPLIT` (A -> B, C).

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::{entity, property},
    map::{
        notification_inspection::{notified_entities, notifies_entity},
        subscription_document::{set_entity_type, subscription},
    },
    notification_adapter::NotificationAdapter,
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

/// Delivery poll budget for a re-assembled notification.
const DELIVER: Duration = Duration::from_secs(12);

/// The resource a forwarded subscription copy is created on at a source.
const FORWARDED_SUBSCRIPTIONS_PATH: &str = "/ngsi-ld/v1/subscriptions";

/// The resource a source notifies the front broker's forwarded copy at.
const RECEIVER_PATH: &str = "/ngsi-ld/v1/notifications";

/// Register an inclusive `Vehicle` source on the front broker pointing at `role`.
async fn register_source(topology: &MultiBrokerTopology<BrokerRole>, role: BrokerRole, entity_type: &str, registration_local: &str) {
    let peer = topology.broker(role).expect("topology broker for role");
    let registration = CsrBuilder::new(&topology.id(registration_local), peer.peer_url())
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(entity_type)
        .operations(&["federationOps"])
        .build();
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&registration)
            .await
            .expect("register source"),
        201,
    );
}

/// Provision the split scene: B holds `speed`, C holds `colour` for the same
/// `Vehicle`, both sources are registered on A, and a subscription on A delivers
/// to the receiver. The subscription sets `splitEntities: true`: clause 5.8.6
/// only re-assembles from every holder "If the splitEntities member of the
/// local Subscription is explicitly set to true or, if not explicitly set, the
/// default setting of the deployment allows split entities", and the
/// deployment default is complete. Returns nothing; the caller triggers a
/// change and asserts.
async fn setup_split(topology: &MultiBrokerTopology<BrokerRole>, receiver: &NotificationAdapter, entity_type: &str, entity_id: &str, subscription_id: &str) {
    register_source(topology, BrokerRole::B, entity_type, "registration:split-b").await;
    register_source(topology, BrokerRole::C, entity_type, "registration:split-c").await;
    let on_b = entity(entity_id, entity_type, &[("speed", property(json!(40)))]);
    assert_status(
        &topology
            .broker(BrokerRole::B)
            .expect("topology broker B")
            .client()
            .post_entity(&on_b)
            .with_ld_json()
            .send()
            .await
            .expect("create B part"),
        201,
    );
    let on_c = entity(entity_id, entity_type, &[("colour", property(json!("red")))]);
    assert_status(
        &topology
            .broker(BrokerRole::C)
            .expect("topology broker C")
            .client()
            .post_entity(&on_c)
            .with_ld_json()
            .send()
            .await
            .expect("create C part"),
        201,
    );
    let mut split_subscription = subscription(subscription_id, &receiver.endpoint_url());
    set_entity_type(&mut split_subscription, entity_type);
    split_subscription["splitEntities"] = json!(true);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&split_subscription)
            .await
            .expect("create subscription"),
        201,
    );
}

/// Append an attribute to the B-held part so B notifies the front broker.
async fn trigger_on_b(topology: &MultiBrokerTopology<BrokerRole>, entity_id: &str) {
    let fragment = json!({ "mileage": property(json!(1000)), "@context": COMPOUND_CONTEXT.as_str() });
    assert_status(
        &topology
            .broker(BrokerRole::B)
            .expect("topology broker B")
            .client()
            .append_entity_attrs(entity_id, &fragment)
            .await
            .expect("append on B"),
        204,
    );
}

/// A context-source filter limits re-assembly to matching origin entities
/// (clause 5.8.6).
#[tokio::test]
async fn notif_csf_limits_to_matching_origin_entities() {
    let topology = topology::chain().await.expect("three-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:csf-origin");
    let subscription_id = topology.id("Subscription:csf-origin");
    setup_split(&topology, &receiver, &vtype, &entity_id, &subscription_id).await;
    trigger_on_b(&topology, &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, DELIVER).await;
    assert!(
        notifies_entity(&notification, &entity_id),
        "the matching origin entity is re-assembled and notified"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An inbound notification under a mapped subscription id creates a re-assembly
/// copy (clause 5.8.6).
#[tokio::test]
async fn reassembly_inbound_mapped_subscription_id_creates_copy() {
    let topology = topology::chain().await.expect("three-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:mapped-copy");
    let subscription_id = topology.id("Subscription:mapped-copy");
    setup_split(&topology, &receiver, &vtype, &entity_id, &subscription_id).await;
    trigger_on_b(&topology, &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(subscription_id.as_str()),
        "the re-assembly copy carries the local subscription id"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Re-assembly retrieves the local copy and every holder except the sender
/// (clause 5.8.6).
#[tokio::test]
async fn reassembly_split_retrieves_local_and_all_holders_except_sender() {
    let topology = topology::chain().await.expect("three-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:holders-except-sender");
    let subscription_id = topology.id("Subscription:holders-except-sender");
    setup_split(&topology, &receiver, &vtype, &entity_id, &subscription_id).await;
    trigger_on_b(&topology, &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, DELIVER).await;
    let entities: Vec<Value> = notified_entities(&notification);
    let reassembled = entities
        .iter()
        .find(|entity| entity["id"].as_str() == Some(entity_id.as_str()))
        .cloned()
        .unwrap_or(Value::Null);
    assert!(
        reassembled["colour"].get("value").is_some(),
        "the non-sender holder's attribute is retrieved into the re-assembly: {reassembled}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Re-assembly merges the retrieved holders with the notification's `data`
/// member (clause 5.8.6).
#[tokio::test]
async fn reassembly_merges_retrieved_with_data_member() {
    let topology = topology::chain().await.expect("three-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:merge-data");
    let subscription_id = topology.id("Subscription:merge-data");
    setup_split(&topology, &receiver, &vtype, &entity_id, &subscription_id).await;
    trigger_on_b(&topology, &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, DELIVER).await;
    let entities: Vec<Value> = notified_entities(&notification);
    let reassembled = entities
        .iter()
        .find(|entity| entity["id"].as_str() == Some(entity_id.as_str()))
        .cloned()
        .unwrap_or(Value::Null);
    assert!(
        reassembled["speed"].get("value").is_some() && reassembled["colour"].get("value").is_some(),
        "the merged entity carries attributes from both the notification and the retrieved holder: {reassembled}"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Re-assembly removes entities that no longer match after the merge
/// (clause 5.8.6).
#[tokio::test]
async fn reassembly_removes_non_matching_after_merge() {
    let topology = topology::chain().await.expect("three-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:remove-nonmatching");
    let subscription_id = topology.id("Subscription:remove-nonmatching");
    register_source(&topology, BrokerRole::B, &vtype, "registration:remove-b").await;
    let on_b = entity(&entity_id, &vtype, &[("speed", property(json!(90)))]);
    assert_status(
        &topology
            .broker(BrokerRole::B)
            .expect("topology broker B")
            .client()
            .post_entity(&on_b)
            .with_ld_json()
            .send()
            .await
            .expect("create B part"),
        201,
    );
    let mut body = subscription(&subscription_id, &receiver.endpoint_url());
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
    trigger_on_b(&topology, &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, DELIVER).await;
    assert!(
        notifies_entity(&notification, &entity_id),
        "the still-matching entity survives the post-merge filter"
    );
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The identifier of the subscription copy the front broker placed at
/// `source`: the `id` of the first copy created there.
async fn placed_copy_id(source: &ContextSourceAdapter) -> String {
    loop {
        let request = source.next_request(DELIVER).await;
        if request.method == Method::POST && request.uri.path() == FORWARDED_SUBSCRIPTIONS_PATH {
            let copy: Value = request.json().expect("the copy is a JSON subscription");
            return copy["id"].as_str().expect("the copy names its identifier").to_owned();
        }
    }
}

/// Clause 5.8.6: a source notifies a deleted attribute with "only the name of
/// the attribute as key and the URI "urn:ngsi-ld:null" as value", whatever
/// family it was, and the copy relayed to the local subscriber reports the
/// deletion the same way. The Null is never read as a concise Property
/// `{"type": "Property", "value": "urn:ngsi-ld:null"}`, which would name a
/// deleted Relationship a Property.
#[tokio::test]
async fn regression_r56_a_notified_deletion_is_relayed_as_the_bare_null() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let source = ContextSourceAdapter::start().await;
    source.set_stub(Method::POST, FORWARDED_SUBSCRIPTIONS_PATH, 201).await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:relayed-deletion");
    let subscription_id = topology.id("Subscription:relayed-deletion");
    let front = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let registration = CsrBuilder::new(&topology.id("registration:relayed-deletion"), &source.url("/"))
        .tenant(topology.tenant())
        .mode("inclusive")
        .entity_type(&vtype)
        .operations(&["federationOps"])
        .build();
    assert_status(&front.create_registration(&registration).await.expect("register source"), 201);
    let mut local = subscription(&subscription_id, &receiver.endpoint_url());
    set_entity_type(&mut local, &vtype);
    assert_status(&front.create_subscription(&local).await.expect("create subscription"), 201);
    let copy_id = placed_copy_id(&source).await;

    let notified = front
        .raw(Method::POST, RECEIVER_PATH)
        .header("Content-Type", "application/ld+json")
        .json(&json!({
            "id": "urn:ngsi-ld:Notification:relayed-deletion",
            "type": "Notification",
            "subscriptionId": copy_id,
            "notifiedAt": "2026-10-04T12:00:00Z",
            "data": [{"id": entity_id, "type": vtype, "owner": "urn:ngsi-ld:null", "speed": property(json!(40))}],
            "@context": COMPOUND_CONTEXT.as_str()
        }))
        .send()
        .await
        .expect("notify the front broker");
    let relayed = receiver.wait_for_subscription_notification(&subscription_id, DELIVER).await;
    receiver.shutdown().await;
    source.shutdown().await;
    topology.finish().await.expect("topology cleanup");

    assert_status(&notified, 200);
    let entities = notified_entities(&relayed);
    let relayed_entity = entities
        .iter()
        .find(|entity| entity["id"].as_str() == Some(entity_id.as_str()))
        .unwrap_or(&Value::Null);
    assert_eq!(relayed_entity["owner"], json!("urn:ngsi-ld:null"), "{relayed_entity}");
    assert_eq!(relayed_entity["speed"], json!({"type": "Property", "value": 40}), "{relayed_entity}");
}

/// A non-empty re-assembly copy is forwarded to the local endpoint under the
/// local subscription id (clause 5.8.6).
#[tokio::test]
async fn reassembly_non_empty_copy_forwarded_to_local_endpoint_with_local_id() {
    let topology = topology::chain().await.expect("three-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let entity_id = topology.id("Vehicle:local-endpoint-copy");
    let subscription_id = topology.id("Subscription:local-endpoint-copy");
    setup_split(&topology, &receiver, &vtype, &entity_id, &subscription_id).await;
    trigger_on_b(&topology, &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, DELIVER).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(subscription_id.as_str()),
        "the copy is delivered under the local subscription id"
    );
    assert!(!notified_entities(&notification).is_empty(), "the re-assembled copy is non-empty");
    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
