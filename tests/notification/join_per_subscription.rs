//! The Linked Entities of a `join` follow the Relationships each
//! subscription's own selection returned (clauses 5.7.1.5 and 5.8.6), even
//! when several subscriptions are notified of one change.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{
    helpers::{
        client,
        create_entity,
        create_subscription,
        set_endpoint_uri,
        subscription_with_endpoint,
        with_dataset_id,
        with_join,
        with_notification_attributes,
    },
    notification_adapter::NotificationAdapter as NotificationReceiver,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

const DATASET: &str = "urn:ngsi-ld:Dataset:r10-alternative";

/// A City with nothing but its identity.
fn city(city_id: &str) -> Value {
    json!({"id": city_id, "type": "City", "@context": [CORE_CONTEXT]})
}

/// Create `subscription` with its endpoint at `receiver`.
async fn subscribe(http_client: &reqwest::Client, scope: &TestEnvironment, receiver: &NotificationReceiver, mut subscription: Value) {
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, _) = create_subscription(http_client, scope, &subscription).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
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

/// Two flat joins notified of one creation: the subscription listing
/// `locatedAt` receives the City, the one listing `name` returns no
/// Relationship and so receives the Building alone -- whichever of the two
/// resolves first.
#[tokio::test]
async fn notification_flat_join_follows_only_the_relationships_each_subscription_selects() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let city_id = scope.id("City:r10_01");
    let (status, _, _) = create_entity(&http_client, &city(&city_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let located_id = scope.id("Subscription:r10_01_located");
    let named_id = scope.id("Subscription:r10_01_named");
    for (subscription_id, listed) in [(&located_id, "locatedAt"), (&named_id, "name")] {
        let subscription = with_notification_attributes(
            with_join(subscription_with_endpoint(subscription_id, &receiver.endpoint_url()), "flat", 1),
            &[listed],
        );
        subscribe(&http_client, &scope, &receiver, subscription).await;
    }
    receiver.clear_notifications().await;

    let entity_id = scope.id("Building:r10_01");
    let building = json!({
        "id": entity_id,
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "locatedAt": {"type": "Relationship", "object": city_id},
        "@context": [CORE_CONTEXT]
    });
    let (status, _, _) = create_entity(&http_client, &building).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let located = receiver.wait_for_subscription_notification(&located_id, Duration::from_secs(5)).await;
    let named = receiver.wait_for_subscription_notification(&named_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(carried_ids(&located.body), vec![entity_id.as_str(), city_id.as_str()]);
    pretty_assertions::assert_eq!(carried_ids(&named.body), vec![entity_id.as_str()]);
    scope.finish().await.expect("cleanup failed");
}

/// Two flat joins selecting different `datasetId` instances of one
/// Relationship: each receives the City its own instance names, and only it.
#[tokio::test]
async fn notification_flat_join_follows_only_the_dataset_instances_each_subscription_selects() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let default_city = scope.id("City:r10_02_default");
    let other_city = scope.id("City:r10_02_other");
    for city_id in [&default_city, &other_city] {
        let (status, _, _) = create_entity(&http_client, &city(city_id)).await;
        pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    }

    let default_id = scope.id("Subscription:r10_02_default");
    let other_id = scope.id("Subscription:r10_02_other");
    for (subscription_id, dataset) in [(&default_id, "@none"), (&other_id, DATASET)] {
        let subscription = with_dataset_id(
            with_join(subscription_with_endpoint(subscription_id, &receiver.endpoint_url()), "flat", 1),
            &[dataset],
        );
        subscribe(&http_client, &scope, &receiver, subscription).await;
    }
    receiver.clear_notifications().await;

    let entity_id = scope.id("Building:r10_02");
    let building = json!({
        "id": entity_id,
        "type": "Building",
        "locatedAt": [
            {"type": "Relationship", "object": default_city},
            {"type": "Relationship", "object": other_city, "datasetId": DATASET}
        ],
        "@context": [CORE_CONTEXT]
    });
    let (status, _, _) = create_entity(&http_client, &building).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let default = receiver.wait_for_subscription_notification(&default_id, Duration::from_secs(5)).await;
    let other = receiver.wait_for_subscription_notification(&other_id, Duration::from_secs(5)).await;
    pretty_assertions::assert_eq!(carried_ids(&default.body), vec![entity_id.as_str(), default_city.as_str()]);
    pretty_assertions::assert_eq!(carried_ids(&other.body), vec![entity_id.as_str(), other_city.as_str()]);
    scope.finish().await.expect("cleanup failed");
}
