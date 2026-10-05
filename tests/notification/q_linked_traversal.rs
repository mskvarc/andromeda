//! A subscription `q` may cross a Relationship into the entity it names
//! (clause 4.9 `LinkedEntityRelation`, EXAMPLE 13: "makes a sub-query on
//! matching target objects"), and the subscription is notified when the linked
//! entity satisfies the sub-query.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{
    helpers::{
        client,
        create_entity,
        create_subscription,
        set_endpoint_uri,
        subscription_with_endpoint,
        update_entity_attrs,
        with_query_filter,
        with_time_interval,
    },
    notification_adapter::NotificationAdapter as NotificationReceiver,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// How long a change-driven notification is waited on.
const NOTIFICATION_WAIT: Duration = Duration::from_secs(5);
/// How long a notification that must not come is waited on.
const SILENCE: Duration = Duration::from_secs(2);
/// How long a two-second periodic subscription is waited on.
const PERIODIC_WAIT: Duration = Duration::from_secs(15);

const COMMERCIAL_HOP: &str = "isParkedIn{category}==\"commercial\"";

/// Create `entity` under the core context.
async fn create(http_client: &reqwest::Client, mut entity: Value) {
    entity["@context"] = json!([CORE_CONTEXT]);
    let (status, _, body) = create_entity(http_client, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// A parking of `parking_type` whose `category` is `category`.
fn parking(parking_id: &str, parking_type: &str, category: &str) -> Value {
    json!({"id": parking_id, "type": parking_type, "category": {"type": "Property", "value": category}})
}

/// A car of `car_type` parked in `parking_id`.
fn car(car_id: &str, car_type: &str, parking_id: &str) -> Value {
    json!({"id": car_id, "type": car_type, "isParkedIn": {"type": "Relationship", "object": parking_id}})
}

/// A subscription on `entity_type` filtering on `q`.
fn filtered(subscription_id: &str, receiver: &NotificationReceiver, entity_type: &str, q: &str) -> Value {
    let mut subscription = with_query_filter(subscription_with_endpoint(subscription_id, &receiver.endpoint_url()), q);
    subscription["entities"] = json!([{"type": entity_type}]);
    subscription
}

/// Create `subscription` with its endpoint at `receiver`.
async fn subscribe(http_client: &reqwest::Client, scope: &TestEnvironment, receiver: &NotificationReceiver, mut subscription: Value) {
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, body) = create_subscription(http_client, scope, &subscription).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// The identifiers a notification carries in its `data`.
fn carried_ids(notification: &Value) -> Vec<&str> {
    notification["data"]
        .as_array()
        .expect("a data array")
        .iter()
        .filter_map(|entity| entity["id"].as_str())
        .collect()
}

/// Wait for the one notification of `subscription_id`, check it carries
/// `expected` alone, and that no other follows.
async fn notified_only_of(receiver: &NotificationReceiver, subscription_id: &str, expected: &str) {
    let notification = receiver.wait_for_subscription_notification(subscription_id, NOTIFICATION_WAIT).await;
    pretty_assertions::assert_eq!(carried_ids(&notification.body), vec![expected]);
    receiver.wait_for_no_subscription_notification(subscription_id, SILENCE).await;
}

/// The commercial and the residential parking of one test.
async fn parkings(http_client: &reqwest::Client, scope: &TestEnvironment, case: &str) -> (String, String) {
    let commercial = scope.id(&format!("R14Parking:{case}_commercial"));
    let residential = scope.id(&format!("R14Parking:{case}_residential"));
    create(http_client, parking(&commercial, "R14Parking", "commercial")).await;
    create(http_client, parking(&residential, "R14Parking", "residential")).await;
    (commercial, residential)
}

/// Clause 4.9 EXAMPLE 13 over a subscription: only the car parked in the
/// commercial parking is notified.
#[tokio::test]
async fn notification_q_hop_matches_on_the_linked_entity() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (commercial, residential) = parkings(&http_client, &scope, "r14_01").await;
    let subscription_id = scope.id("Subscription:r14_01");
    subscribe(
        &http_client,
        &scope,
        &receiver,
        filtered(&subscription_id, &receiver, "R14CarA", COMMERCIAL_HOP),
    )
    .await;
    receiver.clear_notifications().await;

    let elsewhere = scope.id("R14CarA:elsewhere");
    let parked = scope.id("R14CarA:parked");
    create(&http_client, car(&elsewhere, "R14CarA", &residential)).await;
    create(&http_client, car(&parked, "R14CarA", &commercial)).await;
    notified_only_of(&receiver, &subscription_id, &parked).await;
    scope.finish().await.expect("cleanup failed");
}

/// The hop reads the Relationship as the change left it: a car moved into the
/// commercial parking is notified then, and not before.
#[tokio::test]
async fn notification_q_hop_follows_a_relationship_moved_to_another_entity() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (commercial, residential) = parkings(&http_client, &scope, "r14_02").await;
    let subscription_id = scope.id("Subscription:r14_02");
    subscribe(
        &http_client,
        &scope,
        &receiver,
        filtered(&subscription_id, &receiver, "R14CarB", COMMERCIAL_HOP),
    )
    .await;
    receiver.clear_notifications().await;

    let moving = scope.id("R14CarB:moving");
    create(&http_client, car(&moving, "R14CarB", &residential)).await;
    receiver.wait_for_no_subscription_notification(&subscription_id, SILENCE).await;
    let fragment = json!({"isParkedIn": {"type": "Relationship", "object": commercial}, "@context": [CORE_CONTEXT]});
    let (status, body) = update_entity_attrs(&http_client, &moving, &fragment).await;
    pretty_assertions::assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    notified_only_of(&receiver, &subscription_id, &moving).await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 EXAMPLE 14: the entity-type hint restricts the hop to linked
/// entities of the listed types.
#[tokio::test]
async fn notification_q_hop_type_guard() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (commercial, _) = parkings(&http_client, &scope, "r14_03").await;
    let garage = scope.id("R14Garage:r14_03");
    create(&http_client, parking(&garage, "R14Garage", "commercial")).await;
    let subscription_id = scope.id("Subscription:r14_03");
    let guarded = "isParkedIn{R14Garage,R14Depot:category}==\"commercial\"";
    subscribe(&http_client, &scope, &receiver, filtered(&subscription_id, &receiver, "R14CarC", guarded)).await;
    receiver.clear_notifications().await;

    let in_parking = scope.id("R14CarC:in_parking");
    let in_garage = scope.id("R14CarC:in_garage");
    create(&http_client, car(&in_parking, "R14CarC", &commercial)).await;
    create(&http_client, car(&in_garage, "R14CarC", &garage)).await;
    notified_only_of(&receiver, &subscription_id, &in_garage).await;
    scope.finish().await.expect("cleanup failed");
}

/// Hops chain: the car is notified when the district its parking lies in is
/// in the north.
#[tokio::test]
async fn notification_q_two_hops() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let mut parkings_by_zone = Vec::new();
    for zone in ["north", "south"] {
        let district = scope.id(&format!("R14District:r14_04_{zone}"));
        create(
            &http_client,
            json!({"id": district, "type": "R14District", "zone": {"type": "Property", "value": zone}}),
        )
        .await;
        let parking_id = scope.id(&format!("R14Parking:r14_04_{zone}"));
        create(
            &http_client,
            json!({"id": parking_id, "type": "R14Parking", "locatedIn": {"type": "Relationship", "object": district}}),
        )
        .await;
        parkings_by_zone.push(parking_id);
    }
    let subscription_id = scope.id("Subscription:r14_04");
    let two_hops = "isParkedIn{locatedIn{zone}}==\"north\"";
    subscribe(&http_client, &scope, &receiver, filtered(&subscription_id, &receiver, "R14CarD", two_hops)).await;
    receiver.clear_notifications().await;

    let south = scope.id("R14CarD:south");
    let north = scope.id("R14CarD:north");
    create(&http_client, car(&south, "R14CarD", &parkings_by_zone[1])).await;
    create(&http_client, car(&north, "R14CarD", &parkings_by_zone[0])).await;
    notified_only_of(&receiver, &subscription_id, &north).await;
    scope.finish().await.expect("cleanup failed");
}

/// A periodic notification carries only the cars the hop admits.
#[tokio::test]
async fn notification_periodic_q_hop() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (commercial, residential) = parkings(&http_client, &scope, "r14_05").await;
    let elsewhere = scope.id("R14CarE:elsewhere");
    let parked = scope.id("R14CarE:parked");
    create(&http_client, car(&elsewhere, "R14CarE", &residential)).await;
    create(&http_client, car(&parked, "R14CarE", &commercial)).await;

    let subscription_id = scope.id("Subscription:r14_05");
    let subscription = with_time_interval(filtered(&subscription_id, &receiver, "R14CarE", COMMERCIAL_HOP), 2);
    subscribe(&http_client, &scope, &receiver, subscription).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, PERIODIC_WAIT).await;
    pretty_assertions::assert_eq!(carried_ids(&notification.body), vec![parked.as_str()]);
    scope.finish().await.expect("cleanup failed");
}
