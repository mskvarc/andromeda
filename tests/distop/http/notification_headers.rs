//! `CSourceNotification` dispatch binding (clause 6.3.9): a matching Context Source
//! Registration triggers a `ContextSourceNotification` `POSTed` to the csource
//! subscription's `endpoint.uri`, with the MIME type driven by `endpoint.accept`
//! (`application/json` default, `application/ld+json` opt-in), a JSON-LD `Link`
//! header on the JSON target, and one custom header per `receiverInfo` entry.

use crate::{
    http::forwarding::{PLACEHOLDER_ENDPOINT, context_link},
    notification_adapter::NotificationAdapter,
    oracle::assert_status,
    registration::csr_basic_typed,
    topology::{self, BrokerRole},
};
use andromeda_test_client::response::TestResponse;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use pretty_assertions::assert_eq;
use reqwest::Method;
use serde_json::{Value, json};
use std::time::Duration;

/// POST a csource subscription body to the front broker.
async fn create_csource_subscription(topology: &MultiBrokerTopology<BrokerRole>, body: &Value) -> TestResponse {
    topology
        .broker(BrokerRole::A)
        .expect("topology broker A")
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/csourceSubscriptions")
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("create csource subscription")
}

/// A csource subscription over the given entity type's registrations delivering
/// to `endpoint`, with the given notification endpoint object spliced in. The
/// type is derived per test so the subscription only matches this test's own
/// registration, never a concurrent test's.
fn csource_subscription(id: &str, entity_type: &str, notification_endpoint: &Value) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [ { "type": entity_type } ],
        "notification": { "endpoint": notification_endpoint },
        "@context": context_link()
    })
}

/// A registration covering `entity_type` at the placeholder endpoint, so a
/// matching csource subscription over that same type is triggered.
fn typed_registration(id: &str, entity_type: &str) -> Value {
    csr_basic_typed(id, PLACEHOLDER_ENDPOINT, entity_type)
}

/// A `CSourceNotification` is `POSTed` to the subscription's `endpoint.uri`
/// (clause 6.3.9). Confirms REQ-HTTP-4.
#[tokio::test]
async fn dist_http_csource_notification_posted_to_endpoint_uri() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let sub_id = topology.id("Subscription:csnotif-post");
    let reg_id = topology.id("registration:csnotif-post");

    let subscription = csource_subscription(&sub_id, &vtype, &json!({ "uri": receiver.endpoint_url(), "accept": "application/json" }));
    assert_status(&create_csource_subscription(&topology, &subscription).await, 201);
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&typed_registration(&reg_id, &vtype))
            .await
            .expect("matching registration"),
        201,
    );

    let notification = receiver.wait_for_subscription_notification(&sub_id, Duration::from_secs(10)).await;
    assert_eq!(
        notification.body["subscriptionId"].as_str(),
        Some(sub_id.as_str()),
        "the notification names its subscription"
    );
    assert_eq!(
        notification.body["type"].as_str(),
        Some("ContextSourceNotification"),
        "the payload is a ContextSourceNotification"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The notification MIME defaults to `application/json` and becomes
/// `application/ld+json` when `endpoint.accept` requests it (clause 6.3.9).
/// Confirms REQ-HTTP-5.
#[tokio::test]
async fn dist_http_csource_notification_mime_defaults_json_and_ld_json_via_accept() {
    let topology = topology::single().await.expect("single-broker topology");
    let json_receiver = NotificationAdapter::start().await;
    let ld_receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let json_sub = topology.id("Subscription:csnotif-mime-json");
    let ld_sub = topology.id("Subscription:csnotif-mime-ld");
    let reg_id = topology.id("registration:csnotif-mime");

    assert_status(
        &create_csource_subscription(
            &topology,
            &csource_subscription(&json_sub, &vtype, &json!({ "uri": json_receiver.endpoint_url(), "accept": "application/json" })),
        )
        .await,
        201,
    );
    assert_status(
        &create_csource_subscription(
            &topology,
            &csource_subscription(&ld_sub, &vtype, &json!({ "uri": ld_receiver.endpoint_url(), "accept": "application/ld+json" })),
        )
        .await,
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&typed_registration(&reg_id, &vtype))
            .await
            .expect("matching registration"),
        201,
    );

    let json_notification = json_receiver.wait_for_subscription_notification(&json_sub, Duration::from_secs(10)).await;
    let json_type = json_notification
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(json_type.contains("application/json"), "the default MIME is application/json, got {json_type}");

    let ld_notification = ld_receiver.wait_for_subscription_notification(&ld_sub, Duration::from_secs(10)).await;
    let ld_type = ld_notification
        .headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    assert!(
        ld_type.contains("application/ld+json"),
        "endpoint.accept selects application/ld+json, got {ld_type}"
    );

    json_receiver.shutdown().await;
    ld_receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A JSON (non-LD) notification target carries the JSON-LD `@context` in a `Link`
/// header (clause 6.3.9). Confirms REQ-HTTP-6.
#[tokio::test]
async fn dist_http_csource_notification_json_target_carries_link_context() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let sub_id = topology.id("Subscription:csnotif-link");
    let reg_id = topology.id("registration:csnotif-link");

    assert_status(
        &create_csource_subscription(
            &topology,
            &csource_subscription(&sub_id, &vtype, &json!({ "uri": receiver.endpoint_url(), "accept": "application/json" })),
        )
        .await,
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&typed_registration(&reg_id, &vtype))
            .await
            .expect("matching registration"),
        201,
    );

    let notification = receiver.wait_for_subscription_notification(&sub_id, Duration::from_secs(10)).await;
    assert!(
        notification.headers.get("link").is_some(),
        "a JSON notification target carries a Link context header"
    );

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Each `receiverInfo` entry becomes one custom HTTP header on the notification
/// (clause 6.3.9). Confirms REQ-HTTP-7.
#[tokio::test]
async fn dist_http_csource_notification_receiver_info_becomes_custom_headers() {
    let topology = topology::single().await.expect("single-broker topology");
    let receiver = NotificationAdapter::start().await;
    let vtype = topology.id("Vehicle");
    let sub_id = topology.id("Subscription:csnotif-receiverinfo");
    let reg_id = topology.id("registration:csnotif-receiverinfo");

    let endpoint = json!({
        "uri": receiver.endpoint_url(),
        "accept": "application/json",
        "receiverInfo": [ { "key": "X-Custom-Token", "value": "secret-value" } ]
    });
    assert_status(
        &create_csource_subscription(&topology, &csource_subscription(&sub_id, &vtype, &endpoint)).await,
        201,
    );
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_registration(&typed_registration(&reg_id, &vtype))
            .await
            .expect("matching registration"),
        201,
    );

    let notification = receiver.wait_for_subscription_notification(&sub_id, Duration::from_secs(10)).await;
    let custom = notification.headers.get("x-custom-token").and_then(|value| value.to_str().ok());
    assert_eq!(custom, Some("secret-value"), "a receiverInfo entry becomes a custom header");

    receiver.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
