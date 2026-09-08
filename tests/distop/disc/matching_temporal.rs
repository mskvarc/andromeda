//! Subscription-side temporal, geo, context-source and scope gating of the
//! matching algorithm (clause 5.12), captured as `ContextSourceNotification`s
//! through the in-process notification adapter. All `T-LOCAL` plus the adapter.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{register, register_interval, register_located},
        csr_subscription::{self, CsrSubscriptionBuilder},
    },
    notification_adapter::NotificationAdapter,
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_harness::multi_broker::MultiBrokerTopology;
use serde_json::json;
use std::time::Duration;

/// The wait budget for an expected notification.
const PRESENT: Duration = Duration::from_secs(10);
/// The quiet window over which an absence is confirmed.
const ABSENT: Duration = Duration::from_secs(4);
/// The start of the interval every temporal registration in this module carries.
const INTERVAL_START: &str = "2020-01-01T00:00:00Z";
/// The end of that interval.
const INTERVAL_END: &str = "2021-01-01T00:00:00Z";

/// Create a Context Source Subscription and assert the `201` acknowledgement.
async fn create_subscription(topology: &MultiBrokerTopology<BrokerRole>, body: &serde_json::Value) {
    let created = csr_subscription::create(topology.broker(BrokerRole::A).client(), body).await;
    assert_status(&created, 201);
}

/// A subscription without a temporal query matches latest-info registrations
/// only: an interval-bearing registration is silent, a latest one notifies
/// (clause 5.12). Confirms REQ-DISC-52, REQ-DISC-43.
#[tokio::test]
async fn matching_sub_no_temporaliq_matches_latest_info_only() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("NoTemporalSubType");
    let subscription_id = topology.id("subscription:no-temporal");
    create_subscription(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;

    register_interval(
        &topology,
        &topology.id("registration:no-temporal-interval"),
        &vtype,
        "observationInterval",
        INTERVAL_START,
        INTERVAL_END,
    )
    .await;
    adapter.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:no-temporal-latest"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the latest-info registration notifies"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A subscription with a temporal query matches interval-bearing registrations
/// only: a latest registration is silent, an interval one notifies (clause
/// 5.12). Confirms REQ-DISC-53, REQ-DISC-44.
#[tokio::test]
async fn matching_sub_temporaliq_matches_interval_csrs_only() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("TemporalSubType");
    let subscription_id = topology.id("subscription:temporal");
    let temporal_q = json!({ "timerel": "after", "timeAt": "2020-06-01T00:00:00Z", "timeproperty": "observedAt" });
    create_subscription(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
            .temporal_q(temporal_q)
            .build(),
    )
    .await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:temporal-latest"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    adapter.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;

    register_interval(
        &topology,
        &topology.id("registration:temporal-interval"),
        &vtype,
        "observationInterval",
        INTERVAL_START,
        INTERVAL_END,
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the interval-bearing registration notifies"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// An `observedAt` temporal query notifies when a registration's
/// `observationInterval` overlaps the queried instant (clause 5.12). Confirms
/// REQ-DISC-54.
#[tokio::test]
async fn matching_sub_observedat_matches_observation_interval() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("ObsSubType");
    let subscription_id = topology.id("subscription:observed");
    let temporal_q = json!({ "timerel": "after", "timeAt": "2020-06-01T00:00:00Z", "timeproperty": "observedAt" });
    create_subscription(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
            .temporal_q(temporal_q)
            .build(),
    )
    .await;

    register_interval(
        &topology,
        &topology.id("registration:observed"),
        &vtype,
        "observationInterval",
        INTERVAL_START,
        INTERVAL_END,
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the observation-interval registration notifies"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A management time-property query notifies against a `managementInterval`
/// (clause 5.12). Confirms REQ-DISC-55.
#[tokio::test]
async fn matching_sub_management_timeproperties_match_management_interval() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("MgmtSubType");
    let subscription_id = topology.id("subscription:management");
    let temporal_q = json!({ "timerel": "after", "timeAt": "2020-06-01T00:00:00Z", "timeproperty": "createdAt" });
    create_subscription(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
            .temporal_q(temporal_q)
            .build(),
    )
    .await;

    register_interval(
        &topology,
        &topology.id("registration:management"),
        &vtype,
        "managementInterval",
        INTERVAL_START,
        INTERVAL_END,
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the management-interval registration notifies"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// When the interval relevant to the queried time-property is absent the
/// registration does not notify: a `createdAt` query against an
/// observation-only registration is silent (clause 5.12). Confirms REQ-DISC-56.
#[tokio::test]
async fn matching_sub_relevant_interval_absent_no_match() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("AbsentSubType");
    let subscription_id = topology.id("subscription:absent");
    let temporal_q = json!({ "timerel": "after", "timeAt": "2020-06-01T00:00:00Z", "timeproperty": "createdAt" });
    create_subscription(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
            .temporal_q(temporal_q)
            .build(),
    )
    .await;

    register_interval(
        &topology,
        &topology.id("registration:absent"),
        &vtype,
        "observationInterval",
        INTERVAL_START,
        INTERVAL_END,
    )
    .await;
    adapter.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The `before` relation notifies when the interval falls within the queried
/// endpoint (clause 5.12). Confirms REQ-DISC-57.
#[tokio::test]
async fn matching_sub_before_after_within_or_at_endpoint() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("BeforeSubType");
    let subscription_id = topology.id("subscription:before");
    let temporal_q = json!({ "timerel": "before", "timeAt": "2020-06-01T00:00:00Z", "timeproperty": "observedAt" });
    create_subscription(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
            .temporal_q(temporal_q)
            .build(),
    )
    .await;

    register_interval(
        &topology,
        &topology.id("registration:before"),
        &vtype,
        "observationInterval",
        INTERVAL_START,
        INTERVAL_END,
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "a before query inside the interval notifies"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The `between` relation notifies when its window overlaps the interval
/// (clause 5.12). Confirms REQ-DISC-58.
#[tokio::test]
async fn matching_sub_between_matches_on_overlap() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("BetweenSubType");
    let subscription_id = topology.id("subscription:between");
    let temporal_q = json!({ "timerel": "between", "timeAt": "2020-06-01T00:00:00Z", "endTimeAt": "2020-09-01T00:00:00Z", "timeproperty": "observedAt" });
    create_subscription(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
            .temporal_q(temporal_q)
            .build(),
    )
    .await;

    register_interval(
        &topology,
        &topology.id("registration:between"),
        &vtype,
        "observationInterval",
        INTERVAL_START,
        INTERVAL_END,
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "a between window overlapping the interval notifies"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A geo-query notifies against a registration's `location` GeoProperty
/// (clause 5.12). Confirms REQ-DISC-50, REQ-DISC-51.
#[tokio::test]
async fn matching_sub_geoq_matches_geoproperty_and_default_location() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("GeoSubType");
    let subscription_id = topology.id("subscription:geo");
    let geo_q = json!({ "georel": "near;maxDistance==3000", "geometry": "Point", "coordinates": [13.404, 52.520], "geoproperty": "location" });
    create_subscription(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
            .geo_q(geo_q)
            .build(),
    )
    .await;

    register_located(&topology, &topology.id("registration:geo"), &vtype, [13.404, 52.520]).await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "a nearby location notifies the geo-gated subscription"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The Context Source Filter and scope query both gate matching: a registration
/// satisfying the subscription's csf and scopeQ notifies (clause 5.12). The
/// `csf` targets a Context Source Property of the registration (clause 4.9:
/// "target is the name of a Context Source Property member of the
/// CSourceRegistration"), so the fixture carries `tier` as a top-level member,
/// not as a `contextSourceInfo` header pair. Confirms REQ-DISC-61, REQ-DISC-62.
#[tokio::test]
async fn matching_sub_csf_and_scope_gate_matching() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("CsfScopeSubType");
    let subscription_id = topology.id("subscription:csf-scope");
    let subscription = CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
        .csf("tier==gold")
        .scope_q("/Madrid/#")
        .build();
    create_subscription(&topology, &subscription).await;

    let registration_id = topology.id("registration:csf-scope");
    let mut body = CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_type(&vtype).build();
    body["tier"] = json!("gold");
    body["scope"] = json!(["/Madrid/Gardens"]);
    register(&topology, &body).await;

    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "a registration satisfying csf and scopeQ notifies"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
