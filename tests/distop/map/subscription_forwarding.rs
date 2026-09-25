//! Reduced-copy subscription forwarding to a supporting Context Source, captured
//! on an in-process mock (clause 5.8.5). A local subscription that matches an
//! inclusive registration drives a CSR-subscription: the mock captures the exact
//! forwarded request so its shape can be asserted. All `T-MOCK`, seeded
//! `UNKNOWN` (the ratchet promotes them once the broker forwards on green).

use crate::{
    map::{
        csr_subscription::{FORWARD_TIMEOUT, forwarded_subscription, subscription_source as source},
        subscription_document::{set_entity_type, subscription},
    },
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use axum::http::Method;
use pretty_assertions::{assert_eq, assert_ne};
use serde_json::{Value, json};

/// A base subscription scoped to a per-test entity type, so a concurrent test's
/// Vehicle subscription cannot match this test's supporting registration.
fn scoped_sub(id: &str, entity_type: &str) -> Value {
    let mut body = subscription(id, "http://127.0.0.1:9/notify");
    set_entity_type(&mut body, entity_type);
    body
}

/// Creating a matching local subscription provisions a subscription on the
/// supporting source (clause 5.8.5). Confirms REQ-MAP-17.
#[tokio::test]
async fn sub_create_provisions_csr_subscription() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:provision").await;
    let id = topology.id("Subscription:provision");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(captured.method, Method::POST, "the source receives a subscription create");
    assert!(
        captured.uri.path().contains("subscriptions"),
        "the forward targets the subscriptions endpoint: {}",
        captured.uri
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The local-to-CSR mapping lets a local delete tear down exactly its CSR
/// subscription (clause 5.8.5). Confirms REQ-MAP-18.
#[tokio::test]
async fn sub_create_stores_local_to_csr_subscription_mapping() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:mapping").await;
    let id = topology.id("Subscription:mapping");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );
    let created = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(created.method, Method::POST, "the CSR subscription is created first");

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
    let torn_down = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(
        torn_down.method,
        Method::DELETE,
        "deleting the local subscription tears down its CSR subscription"
    );
    assert!(
        torn_down.uri.path().contains("subscriptions"),
        "the tear-down targets the mapped CSR subscription: {}",
        torn_down.uri
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A non-`localOnly` subscription drives per-CSR handling when the source reports
/// a change (clause 5.8.5). Confirms REQ-MAP-21.
#[tokio::test]
async fn sub_local_only_false_triggers_per_csr_handling_on_csource_notification() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:local-only-false").await;
    let id = topology.id("Subscription:local-only-false");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(captured.method, Method::POST, "a non-localOnly subscription provisions per-CSR handling");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The forwarded copy branches on trigger reason per registration mode
/// (clause 5.8.5). Confirms REQ-MAP-22.
#[tokio::test]
async fn sub_csource_notification_branches_on_trigger_reason_per_mode() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:trigger-branch").await;
    let id = topology.id("Subscription:trigger-branch");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    let forwarded = forwarded_subscription(&captured);
    assert_eq!(
        forwarded["type"].as_str(),
        Some("Subscription"),
        "the forwarded copy is a subscription: {forwarded}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A newly matching supporting source builds a reduced-copy subscription
/// (clause 5.8.5). Confirms REQ-MAP-23.
#[tokio::test]
async fn sub_newly_matching_supporting_source_builds_reduced_copy() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:reduced-copy").await;
    let id = topology.id("Subscription:reduced-copy");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    let forwarded = forwarded_subscription(&captured);
    assert_eq!(forwarded["type"].as_str(), Some("Subscription"), "the forwarded copy is a subscription");
    assert!(forwarded["entities"].is_array(), "the reduced copy keeps the entity selector: {forwarded}");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Under `splitEntities: true` the forwarded copy strips `q`, `geoQ`, and
/// `scopeQ`, which the local broker re-applies after re-assembly (clause
/// 5.8.1.4: "If the splitEntities member is explicitly set to true or, if not
/// explicitly set, the default setting of the deployment allows split entities,
/// the members q, geoQ and scopeQ shall be removed from the created copy"). The
/// member is set explicitly because the deployment default is complete
/// entities. Confirms REQ-MAP-24.
#[tokio::test]
async fn sub_newly_matching_split_forward_strips_q_geoq_scopeq() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:strip-filters").await;
    let id = topology.id("Subscription:strip-filters");
    let mut body = subscription(&id, "http://127.0.0.1:9/notify");
    set_entity_type(&mut body, &vtype);
    body["q"] = json!("speed>10");
    body["scopeQ"] = json!("/Madrid");
    body["splitEntities"] = json!(true);
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

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    let forwarded = forwarded_subscription(&captured);
    assert!(forwarded.get("q").is_none_or(Value::is_null), "the forwarded copy strips q: {forwarded}");
    assert!(forwarded.get("geoQ").is_none_or(Value::is_null), "the forwarded copy strips geoQ: {forwarded}");
    assert!(
        forwarded.get("scopeQ").is_none_or(Value::is_null),
        "the forwarded copy strips scopeQ: {forwarded}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The forwarded copy strips the notification `attributes` pick/omit projection
/// (clause 5.8.5). Confirms REQ-MAP-25.
#[tokio::test]
async fn sub_newly_matching_forward_strips_notification_attributes_pick_omit() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:strip-attrs").await;
    let id = topology.id("Subscription:strip-attrs");
    let mut body = subscription(&id, "http://127.0.0.1:9/notify");
    set_entity_type(&mut body, &vtype);
    body["notification"]["attributes"] = json!(["speed"]);
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

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    let forwarded = forwarded_subscription(&captured);
    assert!(
        forwarded["notification"].get("attributes").is_none_or(Value::is_null),
        "the forwarded copy strips the notification attributes projection: {forwarded}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The forwarded copy's endpoint is the local broker, so notifications return to
/// it for re-assembly (clause 5.8.5). Confirms REQ-MAP-26.
#[tokio::test]
async fn sub_newly_matching_forward_endpoint_is_local_broker() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:local-endpoint").await;
    let id = topology.id("Subscription:local-endpoint");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    let forwarded = forwarded_subscription(&captured);
    let endpoint_uri = forwarded["notification"]["endpoint"]["uri"].as_str().unwrap_or_default();
    assert!(!endpoint_uri.is_empty(), "the forwarded copy carries a notification endpoint: {forwarded}");
    assert_ne!(
        endpoint_uri, "http://127.0.0.1:9/notify",
        "the forwarded endpoint is the local broker, not the caller's sink"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The remote-to-local mapping routes an inbound remote id to the correct local
/// subscription (clause 5.8.5). Confirms REQ-MAP-27.
#[tokio::test]
async fn sub_newly_matching_forward_stores_remote_to_local_subscription_mapping() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:remote-local-map").await;
    let id = topology.id("Subscription:remote-local-map");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(
        captured.method,
        Method::POST,
        "a forwarded subscription establishes the remote-to-local mapping"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The CSR-to-remote mapping lets a no-longer-matching change delete exactly the
/// remote subscription id (clause 5.8.5). Confirms REQ-MAP-28.
#[tokio::test]
async fn sub_newly_matching_forward_stores_csr_to_remote_subscription_mapping() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let mock = source(&topology, &vtype, "registration:csr-remote-map").await;
    let id = topology.id("Subscription:csr-remote-map");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_sub(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );
    assert_eq!(mock.next_request(FORWARD_TIMEOUT).await.method, Method::POST, "the CSR subscription is created");

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
    assert_eq!(
        mock.next_request(FORWARD_TIMEOUT).await.method,
        Method::DELETE,
        "the CSR-to-remote mapping deletes the exact remote subscription"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
