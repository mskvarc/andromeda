//! Matching algorithm, attribute part and the subscription-side attribute union
//! (clause 5.12). The query rows run on `T-LOCAL` through the discovery surface;
//! the subscription rows capture a `ContextSourceNotification` through the
//! in-process notification adapter.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{contains, query, register},
        csr_subscription::{self, CsrSubscriptionBuilder},
    },
    notification_adapter::NotificationAdapter,
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use pretty_assertions::assert_eq;
use std::time::Duration;
use urlencoding::encode;

/// The wait budget for a change-driven Context Source Notification.
const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(10);

/// With no attribute names in the query the attribute part is unconstrained and
/// a registration with registered attributes still matches (clause 5.12).
/// Confirms REQ-DISC-107.
#[tokio::test]
async fn matching_attribute_no_names_specified_all_match() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("NoNamesType");
    let id = topology.id("registration:no-names");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).property_names(&["speed"]).build(),
    )
    .await;

    let body = query(&topology, &format!("type={}", encode(&vtype))).await;
    assert!(
        contains(&body, &id),
        "a query with no attribute names matches a registration that registers attributes"
    );
    topology.finish().await.expect("topology cleanup");
}

/// A registration with an empty registered attribute combination (entity-only)
/// matches any requested attribute name (clause 5.12). Confirms REQ-DISC-108.
#[tokio::test]
async fn matching_attribute_empty_registered_combination_entity_only_match() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("EntityOnlyType");
    let id = topology.id("registration:entity-only");
    register(&topology, &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).build()).await;

    let body = query(&topology, &format!("type={}&attrs=speed", encode(&vtype))).await;
    assert!(contains(&body, &id), "an entity-only registration matches any requested attribute name");
    topology.finish().await.expect("topology cleanup");
}

/// The attribute part matches when at least one requested name matches a
/// registered name, and excludes when none do (clause 5.12). Confirms
/// REQ-DISC-109.
#[tokio::test]
async fn matching_attribute_at_least_one_name_matches_registered() {
    let topology = topology::single().await.expect("single-broker topology");
    let vtype = topology.id("OneNameType");
    let id = topology.id("registration:one-name");
    register(
        &topology,
        &CsrBuilder::new(&id, PEER_ENDPOINT).entity_type(&vtype).property_names(&["speed"]).build(),
    )
    .await;

    let one_matches = query(&topology, &format!("type={}&attrs=speed,brandName", encode(&vtype))).await;
    assert!(contains(&one_matches, &id), "at least one matching attribute name keeps the registration");

    let none_match = query(&topology, &format!("type={}&attrs=brandName,fuelLevel", encode(&vtype))).await;
    assert!(!contains(&none_match, &id), "no matching attribute name excludes the registration");
    topology.finish().await.expect("topology cleanup");
}

/// A subscription with empty notification attributes matches any registration,
/// so a newly matching registration drives a notification (clause 5.11.7).
/// Confirms REQ-DISC-96, REQ-DISC-49, REQ-DISC-81.
#[tokio::test]
async fn matching_subscription_empty_notification_attributes_all_match() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("EmptyAttrsSubType");
    let subscription_id = topology.id("subscription:empty-attrs");
    let subscription = CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build();
    let created = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription).await;
    assert_status(&created, 201);

    let registration_id = topology.id("registration:empty-attrs");
    register(
        &topology,
        &CsrBuilder::new(&registration_id, PEER_ENDPOINT)
            .entity_type(&vtype)
            .property_names(&["speed"])
            .build(),
    )
    .await;

    let notification = adapter.wait_for_subscription_notification(&subscription_id, NOTIFICATION_TIMEOUT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the newly matching registration triggers a notification"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A subscription matches on the union of its watched attributes and its
/// notification attributes: a registration covering an attribute drawn only
/// from the notification set still matches (clause 5.12). Confirms REQ-DISC-97,
/// REQ-DISC-48, REQ-DISC-95.
#[tokio::test]
async fn matching_subscription_union_of_watched_and_notification_attributes() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("UnionSubType");
    let watched = topology.id("attr:watched");
    let notified = topology.id("attr:notified");
    let subscription_id = topology.id("subscription:union");
    let subscription = CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
        .watched_attributes(&[watched.as_str()])
        .notification_attributes(&[notified.as_str()])
        .build();
    let created = csr_subscription::create(topology.broker(BrokerRole::A).expect("topology broker A").client(), &subscription).await;
    assert_status(&created, 201);

    let registration_id = topology.id("registration:union");
    register(
        &topology,
        &CsrBuilder::new(&registration_id, PEER_ENDPOINT)
            .entity_type(&vtype)
            .property_names(&[notified.as_str()])
            .build(),
    )
    .await;

    let notification = adapter.wait_for_subscription_notification(&subscription_id, NOTIFICATION_TIMEOUT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "an attribute drawn from the notification set is part of the matching union"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
