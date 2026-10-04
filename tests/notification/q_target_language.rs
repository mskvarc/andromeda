//! A subscription `q` naming a target language reads only that language's
//! value of a `LanguageProperty` (clause 4.9: "If the target element is a
//! `LanguageProperty` and a target language is specified, the target value is
//! defined as the Value associated to the matching key-value pair held within
//! the languageMap").

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{
    helpers::{client, create_entity, create_subscription, set_endpoint_uri, subscription_with_endpoint, with_query_filter, with_time_interval},
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

/// Create an entity of `entity_type` whose `name` is the language map `names`.
async fn create_named(http_client: &reqwest::Client, entity_id: &str, entity_type: &str, names: &Value) {
    let entity = json!({
        "id": entity_id,
        "type": entity_type,
        "name": {"type": "LanguageProperty", "languageMap": names},
        "@context": [CORE_CONTEXT]
    });
    let (status, _, _) = create_entity(http_client, &entity).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
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

/// Create the entity that must not match, then the one that must, and check
/// the subscription is notified of the second alone.
async fn notified_only_of(
    http_client: &reqwest::Client,
    receiver: &NotificationReceiver,
    subscription_id: &str,
    (unmatched_id, unmatched): (&str, Value),
    (matched_id, matched): (&str, Value),
    entity_type: &str,
) {
    create_named(http_client, unmatched_id, entity_type, &unmatched).await;
    create_named(http_client, matched_id, entity_type, &matched).await;
    let notification = receiver.wait_for_subscription_notification(subscription_id, NOTIFICATION_WAIT).await;
    pretty_assertions::assert_eq!(carried_ids(&notification.body), vec![matched_id]);
    receiver.wait_for_no_subscription_notification(subscription_id, SILENCE).await;
}

/// Clause 4.9 `equal`: "a match is found as the value of the key-value pair
/// corresponding to the specified natural language" -- `de: Door` is no match
/// for `name[en]=="Door"`.
#[tokio::test]
async fn notification_q_target_language_matches_only_that_language() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let subscription_id = scope.id("Subscription:r07_01");
    subscribe(
        &http_client,
        &scope,
        &receiver,
        filtered(&subscription_id, &receiver, "R07DoorA", "name[en]==\"Door\""),
    )
    .await;
    receiver.clear_notifications().await;

    let gate = scope.id("R07DoorA:gate");
    let door = scope.id("R07DoorA:door");
    notified_only_of(
        &http_client,
        &receiver,
        &subscription_id,
        (&gate, json!({"en": "Gate", "de": "Door"})),
        (&door, json!({"en": "Door", "de": "Tuer"})),
        "R07DoorA",
    )
    .await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 `unequal` with a target language, and every operator's "shall
/// contain the target element": a map without the language is no match for
/// `!=` either.
#[tokio::test]
async fn notification_q_target_language_inequality_requires_the_language() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let subscription_id = scope.id("Subscription:r07_02");
    subscribe(
        &http_client,
        &scope,
        &receiver,
        filtered(&subscription_id, &receiver, "R07DoorB", "name[fr]!=\"Porte\""),
    )
    .await;
    receiver.clear_notifications().await;

    let english = scope.id("R07DoorB:english");
    let french = scope.id("R07DoorB:french");
    notified_only_of(
        &http_client,
        &receiver,
        &subscription_id,
        (&english, json!({"en": "Door"})),
        (&french, json!({"fr": "Portail"})),
        "R07DoorB",
    )
    .await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.9 `patternOp` reads the language's value like any String target.
#[tokio::test]
async fn notification_q_target_language_pattern() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let subscription_id = scope.id("Subscription:r07_03");
    subscribe(
        &http_client,
        &scope,
        &receiver,
        filtered(&subscription_id, &receiver, "R07DoorC", "name[de]~=\"^T\""),
    )
    .await;
    receiver.clear_notifications().await;

    let house = scope.id("R07DoorC:house");
    let door = scope.id("R07DoorC:door");
    notified_only_of(
        &http_client,
        &receiver,
        &subscription_id,
        (&house, json!({"de": "Haus", "en": "Tower"})),
        (&door, json!({"de": "Tuer", "en": "Door"})),
        "R07DoorC",
    )
    .await;
    scope.finish().await.expect("cleanup failed");
}

/// A periodic notification is filtered by the same reading: only the entity
/// whose English name is `Door` is carried.
#[tokio::test]
async fn notification_periodic_q_target_language() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let gate = scope.id("R07DoorD:gate");
    let door = scope.id("R07DoorD:door");
    create_named(&http_client, &gate, "R07DoorD", &json!({"en": "Gate", "de": "Door"})).await;
    create_named(&http_client, &door, "R07DoorD", &json!({"en": "Door", "de": "Tuer"})).await;

    let subscription_id = scope.id("Subscription:r07_04");
    let subscription = with_time_interval(filtered(&subscription_id, &receiver, "R07DoorD", "name[en]==\"Door\""), 2);
    subscribe(&http_client, &scope, &receiver, subscription).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, PERIODIC_WAIT).await;
    pretty_assertions::assert_eq!(carried_ids(&notification.body), vec![door.as_str()]);
    scope.finish().await.expect("cleanup failed");
}
