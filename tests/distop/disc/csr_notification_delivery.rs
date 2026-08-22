//! Context Source Notification delivery and bookkeeping (clause 5.11.7): the
//! endpoint POST, the success counters and timestamps, the failure status, and
//! status-gated delivery for paused and expired subscriptions. `T-LOCAL` plus
//! the notification adapter.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::register,
        csr_subscription::{self, CsrSubscriptionBuilder},
    },
    notification_adapter::NotificationAdapter,
    oracle::{assert_status, json_body},
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;

/// The wait budget for an expected notification.
const PRESENT: Duration = Duration::from_secs(10);
/// The quiet window over which an absence is confirmed.
const ABSENT: Duration = Duration::from_secs(4);
/// The settle time for asynchronous notification bookkeeping to persist.
const BOOKKEEPING: Duration = Duration::from_millis(750);

/// The notification is delivered to the subscription endpoint as an
/// `application/ld+json` Context Source Notification (clause 5.11.7). Confirms
/// REQ-DISC-87.
#[tokio::test]
async fn csr_notification_sent_to_endpoint() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("SentType");
    let subscription_id = topology.id("subscription:sent");
    csr_subscription::create_and_assert(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:sent"), PEER_ENDPOINT).entity_type(&vtype).build(),
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    let content_type = notification
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(content_type.contains("ld+json"), "the notification is delivered as ld+json, got {content_type}");
    assert_eq!(
        notification.body["type"].as_str(),
        Some("ContextSourceNotification"),
        "the delivered body is a Context Source Notification"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A successful delivery increments `timesSent`, advances `lastNotification` and
/// `lastSuccess`, and sets the notification status to `ok` (clause 5.11.7).
/// Confirms REQ-DISC-88, REQ-DISC-89, REQ-DISC-90.
#[tokio::test]
async fn csr_notification_success_increments_times_sent_and_updates_last_timestamps() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("SuccessType");
    let subscription_id = topology.id("subscription:success");
    csr_subscription::create_and_assert(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:success"), PEER_ENDPOINT).entity_type(&vtype).build(),
    )
    .await;
    let _notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    sleep(BOOKKEEPING).await;

    let retrieved = csr_subscription::retrieve(topology.broker(BrokerRole::A).client(), &subscription_id, Some("sysAttrs")).await;
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert!(
        body["notification"]["timesSent"].as_i64().is_some_and(|count| count >= 1),
        "timesSent is incremented: {}",
        body["notification"]
    );
    assert_eq!(body["notification"]["status"].as_str(), Some("ok"), "the notification status is ok");
    assert!(body["notification"]["lastNotification"].is_string(), "lastNotification advances");
    assert!(body["notification"]["lastSuccess"].is_string(), "lastSuccess advances");
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A failed delivery sets `lastFailure` and the notification status to `failed`
/// (clause 5.11.7). Confirms REQ-DISC-91, REQ-DISC-92.
#[tokio::test]
async fn csr_notification_failed_delivery_updates_last_failure_and_status() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("FailureType");
    let subscription_id = topology.id("subscription:failure");
    let subscription = CsrSubscriptionBuilder::new(&subscription_id, "http://unreachable.invalid:1/notify", &vtype).build();
    csr_subscription::create_and_assert(&topology, &subscription).await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:failure"), PEER_ENDPOINT).entity_type(&vtype).build(),
    )
    .await;
    sleep(Duration::from_secs(3)).await;

    let retrieved = csr_subscription::retrieve(topology.broker(BrokerRole::A).client(), &subscription_id, Some("sysAttrs")).await;
    assert_status(&retrieved, 200);
    let body = json_body(&retrieved);
    assert_eq!(body["notification"]["status"].as_str(), Some("failed"), "the notification status is failed");
    assert!(body["notification"]["lastFailure"].is_string(), "lastFailure is recorded");
    topology.finish().await.expect("topology cleanup");
}

/// Paused and expired subscriptions do not emit notifications (clause 5.11.7).
/// Confirms REQ-DISC-78.
#[tokio::test]
async fn csr_notification_paused_and_expired_subscriptions_do_not_emit() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("GatedType");

    let paused_id = topology.id("subscription:paused");
    csr_subscription::create_and_assert(
        &topology,
        &CsrSubscriptionBuilder::new(&paused_id, &adapter.endpoint_url(), &vtype).active(false).build(),
    )
    .await;

    let expired_id = topology.id("subscription:expired");
    csr_subscription::create_and_assert(&topology, &CsrSubscriptionBuilder::new(&expired_id, &adapter.endpoint_url(), &vtype).build()).await;
    let expired = csr_subscription::update(
        topology.broker(BrokerRole::A).client(),
        &expired_id,
        &json!({ "expiresAt": "2018-08-01T22:07:00Z" }),
    )
    .await;
    assert_status(&expired, 204);

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:gated"), PEER_ENDPOINT).entity_type(&vtype).build(),
    )
    .await;
    adapter.wait_for_no_subscription_notification(&paused_id, ABSENT).await;
    adapter.wait_for_no_subscription_notification(&expired_id, ABSENT).await;
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
