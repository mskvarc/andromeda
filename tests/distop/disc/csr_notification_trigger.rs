//! Context Source Notification structure and trigger reasons (clauses 5.3.2,
//! 5.3.3, 5.11.7): the required notification fields and the newlyMatching /
//! updated / noLongerMatching transitions. `T-LOCAL` plus the notification
//! adapter.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::register,
        csr_subscription::{self, CsrSubscriptionBuilder},
    },
    notification_adapter::NotificationAdapter,
    oracle::assert_status,
    registration::CsrBuilder,
    topology::DistributedTopology,
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use serde_json::json;
use std::time::Duration;

/// The wait budget for an expected notification.
const PRESENT: Duration = Duration::from_secs(10);

/// A subscription over a unique entity type targeting the adapter endpoint.
fn subscription(id: &str, endpoint: &str, entity_type: &str) -> serde_json::Value {
    CsrSubscriptionBuilder::new(id, endpoint, entity_type).build()
}

/// The notification carries every required Context Source Notification field: a
/// URI id, the `ContextSourceNotification` type, the subscription id, a
/// timestamp, a trigger reason, and a non-empty data array (clause 5.3.2).
/// Confirms REQ-DISC-1, REQ-DISC-2, REQ-DISC-3, REQ-DISC-4, REQ-DISC-5,
/// REQ-DISC-6, REQ-DISC-86.
#[tokio::test]
async fn csr_notification_has_required_csource_notification_structure() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("StructureType");
    let subscription_id = topology.id("subscription:structure");
    csr_subscription::create_and_assert(&topology, &subscription(&subscription_id, &adapter.endpoint_url(), &vtype)).await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:structure"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    let body = notification.body;
    assert!(body["id"].as_str().is_some_and(|id| id.contains(':')), "the notification id is a URI: {body}");
    assert_eq!(
        body["type"].as_str(),
        Some("ContextSourceNotification"),
        "the notification type is ContextSourceNotification"
    );
    assert_eq!(
        body["subscriptionId"].as_str(),
        Some(subscription_id.as_str()),
        "the notification carries the subscription id"
    );
    assert!(body["notifiedAt"].is_string(), "the notification carries a notifiedAt timestamp");
    assert!(body["triggerReason"].is_string(), "the notification carries a triggerReason");
    assert!(
        body["data"].as_array().is_some_and(|data| !data.is_empty()),
        "the notification data is a non-empty array of registrations"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The initial notification for a subscription over an already-registered
/// context source is `newlyMatching` (clause 5.11.7). Confirms REQ-DISC-7.
#[tokio::test]
async fn csr_notification_initial_notification_is_newly_matching() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("InitialType");
    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:initial"), PEER_ENDPOINT).entity_type(&vtype).build(),
    )
    .await;

    let subscription_id = topology.id("subscription:initial");
    csr_subscription::create_and_assert(&topology, &subscription(&subscription_id, &adapter.endpoint_url(), &vtype)).await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the initial notification is newlyMatching"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Registering a new matching context source drives a `newlyMatching`
/// notification (clause 5.11.7). Confirms REQ-DISC-8, REQ-DISC-83, REQ-DISC-84.
#[tokio::test]
async fn csr_notification_new_matching_csr_registration_is_newly_matching() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("NewMatchType");
    let subscription_id = topology.id("subscription:new-match");
    csr_subscription::create_and_assert(&topology, &subscription(&subscription_id, &adapter.endpoint_url(), &vtype)).await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:new-match"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "a newly registered matching CSR is newlyMatching"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A previously non-matching registration updated so it matches drives a
/// `newlyMatching` notification (clause 5.11.7). Confirms REQ-DISC-9.
#[tokio::test]
async fn csr_notification_non_matching_csr_updated_to_match_is_newly_matching() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("BecomeMatchType");
    let other_type = topology.id("BecomeMatchOther");
    let registration_id = topology.id("registration:become-match");
    register(&topology, &CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_type(&other_type).build()).await;

    let subscription_id = topology.id("subscription:become-match");
    csr_subscription::create_and_assert(&topology, &subscription(&subscription_id, &adapter.endpoint_url(), &vtype)).await;
    adapter.wait_for_no_subscription_notification(&subscription_id, Duration::from_secs(4)).await;

    let fragment = json!({ "information": [ { "entities": [ { "type": vtype } ] } ] });
    let updated = topology
        .front()
        .client()
        .update_registration(&registration_id, &fragment, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("update registration");
    assert_status(&updated, 204);
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "a CSR updated into matching is newlyMatching"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A matching registration updated while still matching drives an `updated`
/// notification (clause 5.11.7). Confirms REQ-DISC-10, REQ-DISC-83.
#[tokio::test]
async fn csr_notification_matching_csr_updated_still_matching_is_updated() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("StillMatchType");
    let subscription_id = topology.id("subscription:still-match");
    csr_subscription::create_and_assert(&topology, &subscription(&subscription_id, &adapter.endpoint_url(), &vtype)).await;

    let registration_id = topology.id("registration:still-match");
    register(&topology, &CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_type(&vtype).build()).await;
    let _initial = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;

    let fragment = json!({ "endpoint": "http://broker-b:1056/updated" });
    let updated = topology
        .front()
        .client()
        .update_registration(&registration_id, &fragment, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("update registration");
    assert_status(&updated, 204);
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("updated"),
        "a still-matching update is updated"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A matching registration updated so it no longer matches drives a
/// `noLongerMatching` notification (clause 5.11.7). Confirms REQ-DISC-11.
#[tokio::test]
async fn csr_notification_update_makes_csr_no_longer_match_is_no_longer_matching() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("NoLongerType");
    let other_type = topology.id("NoLongerOther");
    let subscription_id = topology.id("subscription:no-longer");
    csr_subscription::create_and_assert(&topology, &subscription(&subscription_id, &adapter.endpoint_url(), &vtype)).await;

    let registration_id = topology.id("registration:no-longer");
    register(&topology, &CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_type(&vtype).build()).await;
    let _initial = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;

    let fragment = json!({ "information": [ { "entities": [ { "type": other_type } ] } ] });
    let updated = topology
        .front()
        .client()
        .update_registration(&registration_id, &fragment, Some(COMPOUND_CONTEXT.as_str()))
        .await
        .expect("update registration");
    assert_status(&updated, 204);
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("noLongerMatching"),
        "a CSR updated out of matching is noLongerMatching"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Deleting a matching registration drives a `noLongerMatching` notification
/// (clause 5.11.7). Confirms REQ-DISC-12, REQ-DISC-83.
#[tokio::test]
async fn csr_notification_matching_csr_deletion_is_no_longer_matching() {
    let topology = DistributedTopology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("DeletionType");
    let subscription_id = topology.id("subscription:deletion");
    csr_subscription::create_and_assert(&topology, &subscription(&subscription_id, &adapter.endpoint_url(), &vtype)).await;

    let registration_id = topology.id("registration:deletion");
    register(&topology, &CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_type(&vtype).build()).await;
    let _initial = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;

    let deleted = topology
        .front()
        .client()
        .delete_registration(&registration_id)
        .await
        .expect("delete registration");
    assert_status(&deleted, 204);
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("noLongerMatching"),
        "a deleted matching CSR is noLongerMatching"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
