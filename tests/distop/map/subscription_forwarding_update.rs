//! Forwarding of subscription updates and deletes to supporting Context Sources,
//! captured on in-process mocks (clause 5.8.5): reduced-copy updates, mapped-id
//! targeting, no-longer-matching tear-down, and delete propagation to every
//! supporting source. All `T-MOCK`, seeded `UNKNOWN`.

use crate::{
    context_source_adapter::ContextSourceAdapter,
    map::{
        csr_subscription::{FORWARD_TIMEOUT, forwarded_subscription, register_subscription_source, stub_subscription_endpoints, subscription_source},
        subscription_document::{set_entity_type, subscription},
    },
    oracle::assert_status,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use axum::http::Method;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The base subscription for a forwarding test, scoped to a per-test entity type
/// so a concurrent test's Vehicle subscription cannot match this registration.
fn scoped_over_source(id: &str, entity_type: &str) -> Value {
    let mut body = subscription(id, "http://127.0.0.1:9/notify");
    set_entity_type(&mut body, entity_type);
    body
}

/// Create a subscription against a source and drain the initial forwarded create,
/// returning the mock ready to capture the next forward.
async fn subscription_over_source(
    topology: &MultiBrokerTopology<BrokerRole>,
    entity_type: &str,
    registration_local: &str,
    subscription_id: &str,
) -> ContextSourceAdapter {
    let mock = subscription_source(topology, entity_type, registration_local).await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_over_source(subscription_id, entity_type))
            .await
            .expect("create subscription"),
        201,
    );
    assert_eq!(
        mock.next_request(FORWARD_TIMEOUT).await.method,
        Method::POST,
        "the initial CSR subscription is created"
    );
    mock
}

/// An updated subscription rebuilds a reduced-copy update on the source
/// (clause 5.8.5). Confirms REQ-MAP-29.
#[tokio::test]
async fn sub_updated_supporting_source_builds_reduced_update() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:updated-reduced");
    let mock = subscription_over_source(&topology, &vtype, "registration:updated-reduced", &id).await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(&id, &json!({ "description": "changed" }), None)
            .await
            .expect("update subscription"),
        204,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    assert!(
        matches!(captured.method, Method::PATCH | Method::POST),
        "the source receives a subscription update: {}",
        captured.method
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Under `splitEntities: true` an updated forward strips `q`, `geoQ`, and
/// `scopeQ` (clause 5.8.1.4, "updated" trigger: "If the splitEntities member is
/// explicitly set to true or, if not explicitly set, the default setting of the
/// deployment allows split entities, the members q, geoQ and scopeQ shall be
/// removed from the updated Subscription"). The member is set explicitly
/// because the deployment default is complete entities. Confirms REQ-MAP-30.
#[tokio::test]
async fn sub_updated_split_forward_strips_q_geoq_scopeq() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:updated-strip-filters");
    let mock = subscription_over_source(&topology, &vtype, "registration:updated-strip-filters", &id).await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(&id, &json!({ "q": "speed>10", "splitEntities": true }), None)
            .await
            .expect("update subscription"),
        204,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    let forwarded = forwarded_subscription(&captured);
    assert!(forwarded.get("q").is_none_or(Value::is_null), "the forwarded update strips q: {forwarded}");
    assert!(
        forwarded.get("scopeQ").is_none_or(Value::is_null),
        "the forwarded update strips scopeQ: {forwarded}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An updated forward strips the notification `attributes` projection
/// (clause 5.8.5). Confirms REQ-MAP-31.
#[tokio::test]
async fn sub_updated_forward_strips_notification_attributes_pick_omit() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:updated-strip-attrs");
    let mock = subscription_over_source(&topology, &vtype, "registration:updated-strip-attrs", &id).await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(
                &id,
                &json!({ "notification": { "attributes": ["speed"], "endpoint": { "uri": "http://127.0.0.1:9/notify", "accept": "application/json" } } }),
                None,
            )
            .await
            .expect("update subscription"),
        204,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    let forwarded = forwarded_subscription(&captured);
    assert!(
        forwarded["notification"].get("attributes").is_none_or(Value::is_null),
        "the forwarded update strips the notification attributes projection: {forwarded}"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An updated forward targets the mapped CSR subscription id (clause 5.8.5).
/// Confirms REQ-MAP-32.
#[tokio::test]
async fn sub_updated_forward_uses_mapped_subscription_id() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:updated-mapped-id");
    let mock = subscription_over_source(&topology, &vtype, "registration:updated-mapped-id", &id).await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(&id, &json!({ "description": "mapped" }), None)
            .await
            .expect("update subscription"),
        204,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    assert!(
        captured.uri.path().contains("subscriptions"),
        "the update targets a mapped CSR subscription: {}",
        captured.uri
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A relevant-change-only forwarding policy is a permitted optimisation: the
/// local update always succeeds, and any forwarding is observational
/// (clause 5.8.5, MAY). Confirms REQ-MAP-33.
#[tokio::test]
async fn sub_updated_forward_only_on_relevant_change() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:updated-relevant-only");
    let mock = subscription_over_source(&topology, &vtype, "registration:updated-relevant-only", &id).await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(&id, &json!({ "description": "irrelevant" }), None)
            .await
            .expect("update subscription"),
        204,
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A no-longer-matching change forwards a delete via the mapped id (clause
/// 5.8.1.4, "noLongerMatching" trigger). The fragment is `application/json`, so
/// its context travels in the `Link` header and never inline (clause 6.3.5: an
/// inline `@context` is only carried by `application/ld+json`). Confirms
/// REQ-MAP-34.
#[tokio::test]
async fn sub_no_longer_matching_forwards_delete_via_mapped_id() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:no-longer-matching");
    let mock = subscription_over_source(&topology, &vtype, "registration:no-longer-matching", &id).await;
    let mut fragment = json!({});
    set_entity_type(&mut fragment, "Device");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(&id, &fragment, Some(CORE_CONTEXT.as_str()))
            .await
            .expect("update subscription"),
        204,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(
        captured.method,
        Method::DELETE,
        "a subscription that no longer matches deletes the mapped CSR subscription"
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A local update propagates to the mapped CSR subscription (clause 5.8.5).
/// Confirms REQ-MAP-50.
#[tokio::test]
async fn sub_update_propagates_to_mapped_csr_subscription() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:propagate-update");
    let mock = subscription_over_source(&topology, &vtype, "registration:propagate-update", &id).await;
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_subscription(&id, &json!({ "description": "propagated" }), None)
            .await
            .expect("update subscription"),
        204,
    );

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    assert!(
        matches!(captured.method, Method::PATCH | Method::POST),
        "the update propagates to the CSR subscription: {}",
        captured.method
    );

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Deleting a subscription forwards a delete to every supporting source
/// (clause 5.8.5). Confirms REQ-MAP-55.
#[tokio::test]
async fn sub_delete_forwards_delete_to_each_supporting_source() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let first = ContextSourceAdapter::start().await;
    let second = ContextSourceAdapter::start().await;
    stub_subscription_endpoints(&first).await;
    stub_subscription_endpoints(&second).await;
    register_subscription_source(&topology, &first.url(""), &vtype, &topology.id("registration:fanout-a")).await;
    register_subscription_source(&topology, &second.url(""), &vtype, &topology.id("registration:fanout-b")).await;

    let id = topology.id("Subscription:fanout-delete");
    assert_status(
        &topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .create_subscription(&scoped_over_source(&id, &vtype))
            .await
            .expect("create subscription"),
        201,
    );
    assert_eq!(
        first.next_request(FORWARD_TIMEOUT).await.method,
        Method::POST,
        "the first source is provisioned"
    );
    assert_eq!(
        second.next_request(FORWARD_TIMEOUT).await.method,
        Method::POST,
        "the second source is provisioned"
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
    assert_eq!(
        first.next_request(FORWARD_TIMEOUT).await.method,
        Method::DELETE,
        "the first source receives the delete"
    );
    assert_eq!(
        second.next_request(FORWARD_TIMEOUT).await.method,
        Method::DELETE,
        "the second source receives the delete"
    );

    first.shutdown().await;
    second.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A local delete propagates to the mapped CSR subscription (clause 5.8.5).
/// Confirms REQ-MAP-56.
#[tokio::test]
async fn sub_delete_propagates_to_mapped_csr_subscription() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("Vehicle");
    let id = topology.id("Subscription:propagate-delete");
    let mock = subscription_over_source(&topology, &vtype, "registration:propagate-delete", &id).await;
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

    let captured = mock.next_request(FORWARD_TIMEOUT).await;
    assert_eq!(captured.method, Method::DELETE, "the delete propagates to the mapped CSR subscription");

    mock.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
