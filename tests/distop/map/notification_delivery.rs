//! Notification delivery state: endpoint targeting with receiver info, the
//! content type that follows the endpoint `accept`, and the subscription
//! delivery counters and status that record each attempt (clauses 5.8.6, 5.2.14,
//! 6.3.5). All `T-LOCAL`; the sink is an in-process server that controls the
//! delivery outcome.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    entity::vehicle,
    map::subscription_document::subscription,
    oracle::{assert_status, forwarded_header, json_body},
    topology::{self, BrokerRole},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// Delivery poll budget for a change that should reach the sink.
const DELIVER: Duration = Duration::from_secs(12);

/// Grace period for the subscription's delivery counters to persist after the
/// receiver has been contacted.
const SETTLE: Duration = Duration::from_millis(750);

/// A notification is delivered to `endpoint.uri` carrying the subscription's
/// receiver info as a header (clauses 5.8.6, 6.3.5). Confirms REQ-MAP-92.
#[tokio::test]
async fn notif_sent_to_endpoint_uri_with_receiver_info() {
    let topology = topology::single().await.expect("single-broker topology");
    let sink = ContextSourceAdapter::start().await;
    sink.set_stub(Method::POST, "/notify", 200).await;
    let id = topology.id("Subscription:receiver-info");
    let mut body = subscription(&id, &sink.url("/notify"));
    body["notification"]["endpoint"]["receiverInfo"] = json!([{ "key": "x-scope", "value": "acme" }]);
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
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:receiver-info"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );

    let captured = sink.next_request(DELIVER).await;
    assert_eq!(
        forwarded_header(&captured, "x-scope").as_deref(),
        Some("acme"),
        "the receiver info is delivered as a header"
    );

    sink.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The delivery content type follows the endpoint `accept`: an `application/json`
/// endpoint receives a `Link` header carrying the context (clause 5.8.6).
/// Confirms REQ-MAP-93.
#[tokio::test]
async fn notif_content_type_follows_endpoint_accept() {
    let topology = topology::single().await.expect("single-broker topology");
    let sink = ContextSourceAdapter::start().await;
    sink.set_stub(Method::POST, "/notify", 200).await;
    let id = topology.id("Subscription:content-type");
    let body = subscription(&id, &sink.url("/notify"));
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
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:content-type"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );

    let captured = sink.next_request(DELIVER).await;
    let content_type = forwarded_header(&captured, "content-type").unwrap_or_default();
    assert!(
        content_type.contains("application/json"),
        "an application/json endpoint receives application/json: {content_type}"
    );
    assert!(
        forwarded_header(&captured, "link").is_some(),
        "an application/json delivery carries the context in a Link header"
    );

    sink.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Retrieve the subscription's `notification` sub-object after a delivery attempt.
async fn notification_state(topology: &MultiBrokerTopology<BrokerRole>, id: &str) -> serde_json::Value {
    sleep(SETTLE).await;
    let retrieved = topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .retrieve_subscription(id, None)
        .await
        .expect("retrieve subscription");
    assert_status(&retrieved, 200);
    json_body(&retrieved)["notification"].clone()
}

/// A delivery increments `timesSent` and records `lastNotification`
/// (clause 5.2.14). Confirms REQ-MAP-94, REQ-MAP-95.
#[tokio::test]
async fn notif_times_sent_and_last_notification_updated() {
    let topology = topology::single().await.expect("single-broker topology");
    let sink = ContextSourceAdapter::start().await;
    sink.set_stub(Method::POST, "/notify", 200).await;
    let id = topology.id("Subscription:times-sent");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&subscription(&id, &sink.url("/notify")))
            .await
            .expect("create subscription"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:times-sent"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let _delivered = sink.next_request(DELIVER).await;

    let notification = notification_state(&topology, &id).await;
    assert!(notification["timesSent"].as_i64().unwrap_or(0) >= 1, "timesSent is incremented: {notification}");
    assert!(
        notification["lastNotification"].as_str().is_some(),
        "lastNotification is recorded: {notification}"
    );

    sink.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A successful delivery sets `status` `ok` and records `lastSuccess`
/// (clause 5.2.14). Confirms REQ-MAP-96, REQ-MAP-97.
#[tokio::test]
async fn notif_success_sets_status_ok_and_last_success() {
    let topology = topology::single().await.expect("single-broker topology");
    let sink = ContextSourceAdapter::start().await;
    sink.set_stub(Method::POST, "/notify", 200).await;
    let id = topology.id("Subscription:success-state");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&subscription(&id, &sink.url("/notify")))
            .await
            .expect("create subscription"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:success-state"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let _delivered = sink.next_request(DELIVER).await;

    let notification = notification_state(&topology, &id).await;
    assert_eq!(
        notification["status"].as_str(),
        Some("ok"),
        "a successful delivery sets status ok: {notification}"
    );
    assert!(notification["lastSuccess"].as_str().is_some(), "lastSuccess is recorded: {notification}");

    sink.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A failed delivery sets `status` `failed` and records `lastFailure`
/// (clause 5.2.14). Confirms REQ-MAP-98, REQ-MAP-99.
#[tokio::test]
async fn notif_failure_sets_status_failed_and_last_failure() {
    let topology = topology::single().await.expect("single-broker topology");
    let sink = ContextSourceAdapter::start().await;
    sink.set_stub(Method::POST, "/notify", 500).await;
    let id = topology.id("Subscription:failure-state");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&subscription(&id, &sink.url("/notify")))
            .await
            .expect("create subscription"),
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .post_entity(&vehicle(&topology.id("Vehicle:failure-state"), 20))
            .with_ld_json()
            .send()
            .await
            .expect("create entity"),
        201,
    );
    let _attempted = sink.next_request(DELIVER).await;

    let notification = notification_state(&topology, &id).await;
    assert_eq!(
        notification["status"].as_str(),
        Some("failed"),
        "a failed delivery sets status failed: {notification}"
    );
    assert!(notification["lastFailure"].as_str().is_some(), "lastFailure is recorded: {notification}");

    sink.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
