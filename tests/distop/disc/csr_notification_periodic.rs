//! Periodic and change-driven Context Source Notification delivery and matching
//! filters (clause 5.11.7): time-interval versus change-driven behaviour,
//! empty-attribute match-all, watched-attribute filtering, and `RegistrationInfo`
//! trimming in the notification data. `T-LOCAL` plus the notification adapter.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::{find, register},
        csr_subscription::{self, CsrSubscriptionBuilder},
    },
    notification_adapter::NotificationAdapter,
    registration::CsrBuilder,
    topology,
};
use serde_json::json;
use std::time::Duration;

/// The wait budget for an expected notification.
const PRESENT: Duration = Duration::from_secs(12);
/// The quiet window over which an absence is confirmed.
const ABSENT: Duration = Duration::from_secs(4);

/// A time-interval subscription sends both an initial and a periodic
/// notification even without an intervening change (clause 5.11.7). Confirms
/// REQ-DISC-59, REQ-DISC-79, REQ-DISC-80.
#[tokio::test]
async fn csr_notification_time_interval_sends_initial_and_periodic_regardless_of_change() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("PeriodicType");
    let subscription_id = topology.id("subscription:periodic");
    let subscription = CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
        .time_interval(2)
        .build();
    csr_subscription::create_and_assert(&topology, &subscription).await;
    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:periodic"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;

    let _initial = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    let _periodic = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A periodic notification includes every currently matching registration
/// (clause 5.11.7). Confirms REQ-DISC-80.
#[tokio::test]
async fn csr_notification_time_interval_periodic_includes_every_currently_matching_csr() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("PeriodicAllType");
    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:periodic-all-a"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:periodic-all-b"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;

    let subscription_id = topology.id("subscription:periodic-all");
    let subscription = CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
        .time_interval(2)
        .build();
    csr_subscription::create_and_assert(&topology, &subscription).await;

    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    let data = notification.body["data"]
        .as_array()
        .unwrap_or_else(|| unreachable!("notification data is an array"));
    assert!(data.len() >= 2, "a periodic notification lists every matching registration, got {}", data.len());
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// A subscription without a time interval is change-driven: a change notifies
/// and a quiet window stays silent (clause 5.11.7). Confirms REQ-DISC-60,
/// REQ-DISC-82, REQ-DISC-78.
#[tokio::test]
async fn csr_notification_no_time_interval_is_change_driven() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("ChangeDrivenType");
    let subscription_id = topology.id("subscription:change-driven");
    csr_subscription::create_and_assert(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:change-driven"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .build(),
    )
    .await;
    let _change = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    adapter.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Empty watched or notification attributes match any registration (clause
/// 5.12). Confirms REQ-DISC-81, REQ-DISC-49, REQ-DISC-96.
#[tokio::test]
async fn csr_notification_empty_watched_or_notification_attributes_match_all() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("EmptyMatchAllType");
    let subscription_id = topology.id("subscription:empty-match-all");
    csr_subscription::create_and_assert(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:empty-match-all"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .property_names(&["speed"])
            .build(),
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "an empty-attribute subscription matches a registration with attributes"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// Watched attributes filter matching to registrations covering them: a
/// registration that does not cover the watched attribute is silent, one that
/// does notifies (clause 5.12). Confirms REQ-DISC-48.
#[tokio::test]
async fn csr_notification_watched_attributes_filter_matches_registration() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("WatchedFilterType");
    let watched = topology.id("attr:watched-filter");
    let other = topology.id("attr:other-filter");
    let subscription_id = topology.id("subscription:watched-filter");
    let subscription = CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
        .watched_attributes(&[watched.as_str()])
        .build();
    csr_subscription::create_and_assert(&topology, &subscription).await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:watched-other"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .property_names(&[other.as_str()])
            .build(),
    )
    .await;
    adapter.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;

    register(
        &topology,
        &CsrBuilder::new(&topology.id("registration:watched-covered"), PEER_ENDPOINT)
            .entity_type(&vtype)
            .property_names(&[watched.as_str()])
            .build(),
    )
    .await;
    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        notification.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "only the registration covering the watched attribute notifies"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The notification data trims each registration to only its matching
/// `RegistrationInfo` (clause 5.11.7, SHOULD). Confirms REQ-DISC-85, REQ-DISC-84.
#[tokio::test]
async fn csr_notification_returns_only_matching_registration_info() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("TrimNotifType");
    let other_type = topology.id("TrimNotifOther");
    let subscription_id = topology.id("subscription:trim-notif");
    csr_subscription::create_and_assert(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype).build(),
    )
    .await;

    let registration_id = topology.id("registration:trim-notif");
    let mut body = CsrBuilder::new(&registration_id, PEER_ENDPOINT).entity_type(&vtype).build();
    body["information"]
        .as_array_mut()
        .unwrap_or_else(|| unreachable!("information is an array"))
        .push(json!({ "entities": [ { "type": other_type } ] }));
    register(&topology, &body).await;

    let notification = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    let registration =
        find(&notification.body["data"], &registration_id).unwrap_or_else(|| unreachable!("the registration is present in the notification data"));
    let information = registration["information"]
        .as_array()
        .unwrap_or_else(|| unreachable!("information is an array"));
    assert_eq!(
        information.len(),
        1,
        "the notified registration is trimmed to its matching RegistrationInfo, got {information:?}"
    );
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
