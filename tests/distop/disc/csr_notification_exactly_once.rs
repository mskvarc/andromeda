//! Each change of a matching registration is notified to a context source
//! subscription once, and with the trigger reason what the subscriber was
//! already told calls for (clauses 5.3.3 and 5.11.7), whichever broker sharing
//! the tenant database evaluates it and however it overlaps the subscription's
//! initial notification.

use crate::{
    disc::{
        PEER_ENDPOINT,
        csr_discovery::register,
        csr_subscription::{self, CsrSubscriptionBuilder},
    },
    notification_adapter::NotificationAdapter,
    oracle::assert_status,
    registration::CsrBuilder,
    topology::{self, BrokerRole},
};
use andromeda_test_fixtures::context::COMPOUND_CONTEXT;
use futures::future::join_all;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::time::{Instant, sleep_until};

/// The wait budget for an expected notification.
const PRESENT: Duration = Duration::from_secs(10);

/// How long an unexpected notification is waited for.
const ABSENT: Duration = Duration::from_secs(4);

/// How many subscription and registration pairs race at once, so the broker
/// evaluates some creations while initial notifications are still pending.
const RACING_PAIRS: usize = 24;

/// The subscription's `throttling`, in seconds: long enough that a
/// registration created right after a notification is certainly held back.
const THROTTLING_SECONDS: u64 = 10;

/// How long after a notification was received the next change is made so the
/// throttling no longer holds it back: the throttling and a one-second margin.
/// The notification's slot was claimed before it was sent, so the margin is
/// measured from a later instant than the slot.
const THROTTLE_CLEARANCE: Duration = Duration::from_secs(THROTTLING_SECONDS + 1);

/// A registration created the moment its subscription is acknowledged is
/// reported once, as `newlyMatching`, whether the subscription's initial
/// notification or the registration's creation carries it; its next update is
/// then `updated` (clause 5.3.3). Many pairs race at once so the broker is
/// still working through initial notifications when the creations arrive.
#[tokio::test]
async fn csr_notification_a_registration_created_right_after_its_subscription_is_reported_once() {
    let topology = topology::single().await.expect("single-broker topology");
    let adapter = NotificationAdapter::start().await;
    let pairs: Vec<(String, String, String)> = (0..RACING_PAIRS)
        .map(|index| {
            (
                topology.id(&format!("RacingType{index}")),
                topology.id(&format!("subscription:racing-{index}")),
                topology.id(&format!("registration:racing-{index}")),
            )
        })
        .collect();

    join_all(pairs.iter().map(|(entity_type, subscription_id, registration_id)| {
        let topology = &topology;
        let endpoint = adapter.endpoint_url();
        async move {
            csr_subscription::create_and_assert(topology, &CsrSubscriptionBuilder::new(subscription_id, &endpoint, entity_type).build()).await;
            register(topology, &CsrBuilder::new(registration_id, PEER_ENDPOINT).entity_type(entity_type).build()).await;
        }
    }))
    .await;

    for (_, subscription_id, _) in &pairs {
        let first = adapter.wait_for_subscription_notification(subscription_id, PRESENT).await;
        assert_eq!(
            first.body["triggerReason"].as_str(),
            Some("newlyMatching"),
            "the first notification of {subscription_id}"
        );
    }
    for (_, _, registration_id) in &pairs {
        let updated = topology
            .broker(BrokerRole::A)
            .expect("topology broker A")
            .client()
            .update_registration(
                registration_id,
                &json!({ "endpoint": "http://broker-b:1056/updated" }),
                Some(COMPOUND_CONTEXT.as_str()),
            )
            .await
            .expect("update registration");
        assert_status(&updated, 204);
    }
    for (_, subscription_id, _) in &pairs {
        let next = adapter.wait_for_subscription_notification(subscription_id, PRESENT).await;
        assert_eq!(
            next.body["triggerReason"].as_str(),
            Some("updated"),
            "the notification after {subscription_id}'s first follows the update, not a repeat of the creation"
        );
    }
    // One shared quiet window: every notification received during it is
    // buffered, so each later check only inspects what was already received.
    for (index, (_, subscription_id, _)) in pairs.iter().enumerate() {
        let window = if index == 0 { ABSENT } else { Duration::ZERO };
        adapter.wait_for_no_subscription_notification(subscription_id, window).await;
    }
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}

/// The identifiers of the registrations a context source notification carries.
fn notified_registrations(notification: &Value) -> Vec<&str> {
    notification["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|registration| registration["id"].as_str())
        .collect()
}

/// What a subscriber was told is recorded in the tenant database, shared by
/// every broker, and a registration change is notified against that record
/// (clause 5.3.3: `newlyMatching` for a registration the subscriber has not
/// been told of, `updated` for one "that was part of a previous
/// notification").
///
/// A subscription created on the front broker throttles its notifications. The
/// replica reports a first registration as `newlyMatching`; a second, created
/// on the replica at once, is held back by the throttling, so the subscriber
/// never hears of it. Once the throttling has passed, the front broker updates
/// the second registration -- which it must report as `newlyMatching`, since
/// the subscriber was never told of it -- and then the first, which it must
/// report as `updated`, since the replica told the subscriber of it.
///
/// Deciding the reason from the registration's state before and after the
/// change reports the second update as `updated`; a record kept by each broker
/// for itself reports the first as `newlyMatching`. Only a record shared
/// through the tenant database reports both correctly.
#[tokio::test]
async fn csr_notification_the_front_broker_reports_registration_updates_against_what_the_replica_notified() {
    let topology = topology::replica_pair().await.expect("replica-pair topology");
    let adapter = NotificationAdapter::start().await;
    let vtype = topology.id("ReplicaType");
    let subscription_id = topology.id("subscription:replica");
    let notified_id = topology.id("registration:replica-notified");
    let held_back_id = topology.id("registration:replica-held-back");
    csr_subscription::create_and_assert(
        &topology,
        &CsrSubscriptionBuilder::new(&subscription_id, &adapter.endpoint_url(), &vtype)
            .throttling(THROTTLING_SECONDS)
            .build(),
    )
    .await;
    let front = topology.broker(BrokerRole::A).expect("topology broker A").client();
    let replica = topology.broker(BrokerRole::AReplica).expect("topology broker A replica").client();

    let created = replica
        .create_registration(&CsrBuilder::new(&notified_id, PEER_ENDPOINT).entity_type(&vtype).build())
        .await
        .expect("create the first registration on the replica");
    assert_status(&created, 201);
    let on_creation = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    let throttle_cleared = Instant::now() + THROTTLE_CLEARANCE;
    assert_eq!(
        on_creation.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the replica reports the first registration"
    );
    assert_eq!(notified_registrations(&on_creation.body), vec![notified_id.as_str()]);

    let held_back = replica
        .create_registration(&CsrBuilder::new(&held_back_id, PEER_ENDPOINT).entity_type(&vtype).build())
        .await
        .expect("create the second registration on the replica");
    assert_status(&held_back, 201);
    adapter.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;

    sleep_until(throttle_cleared).await;
    let updated = front
        .update_registration(
            &held_back_id,
            &json!({ "endpoint": "http://broker-b:1056/updated" }),
            Some(COMPOUND_CONTEXT.as_str()),
        )
        .await
        .expect("update the held-back registration on the front broker");
    assert_status(&updated, 204);
    let on_held_back_update = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    let throttle_cleared = Instant::now() + THROTTLE_CLEARANCE;
    assert_eq!(
        on_held_back_update.body["triggerReason"].as_str(),
        Some("newlyMatching"),
        "the subscriber was never told of the held-back registration"
    );
    assert_eq!(notified_registrations(&on_held_back_update.body), vec![held_back_id.as_str()]);

    sleep_until(throttle_cleared).await;
    let updated = front
        .update_registration(
            &notified_id,
            &json!({ "endpoint": "http://broker-b:1056/updated" }),
            Some(COMPOUND_CONTEXT.as_str()),
        )
        .await
        .expect("update the notified registration on the front broker");
    assert_status(&updated, 204);
    let on_notified_update = adapter.wait_for_subscription_notification(&subscription_id, PRESENT).await;
    assert_eq!(
        on_notified_update.body["triggerReason"].as_str(),
        Some("updated"),
        "the replica told the subscriber of the first registration"
    );
    assert_eq!(notified_registrations(&on_notified_update.body), vec![notified_id.as_str()]);

    adapter.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;
    adapter.shutdown().await;
    topology.finish().await.expect("topology cleanup");
}
