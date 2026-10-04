//! What a periodic (`timeInterval`) notification carries (clause 5.8.6): the
//! subscription's `datasetId` instances, its `join` Linked Entities, its
//! `sysAttrs` and its `pick` / `omit`, exactly as a change-driven one does.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::{
    helpers::{
        building_multi_instances_entity,
        client,
        create_entity,
        create_subscription,
        set_endpoint_uri,
        subscription_with_endpoint,
        with_dataset_id,
        with_join,
        with_notification_attributes,
        with_sys_attrs,
        with_time_interval,
    },
    notification_adapter::NotificationAdapter as NotificationReceiver,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// How long a two-second periodic subscription is waited on.
const PERIODIC_WAIT: Duration = Duration::from_secs(15);

/// Create a City and a Building located at it, both named by `case`.
async fn create_located_building(http_client: &reqwest::Client, scope: &TestEnvironment, case: &str) -> (String, String) {
    let city_id = scope.id(&format!("City:{case}"));
    let (status, _, _) = create_entity(http_client, &json!({"id": city_id, "type": "City", "@context": [CORE_CONTEXT]})).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    let entity_id = scope.id(&format!("Building:{case}"));
    let building = json!({
        "id": entity_id,
        "type": "Building",
        "name": {"type": "Property", "value": "Eiffel Tower"},
        "locatedAt": {"type": "Relationship", "object": city_id},
        "@context": [CORE_CONTEXT]
    });
    let (status, _, _) = create_entity(http_client, &building).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    (entity_id, city_id)
}

/// Create the periodic `subscription` with its endpoint at `receiver`, and wait
/// for its first notification.
async fn first_periodic_notification(
    http_client: &reqwest::Client,
    scope: &TestEnvironment,
    receiver: &NotificationReceiver,
    subscription_id: &str,
    mut subscription: Value,
) -> Value {
    set_endpoint_uri(&mut subscription, &receiver.endpoint_url());
    let (status, _, _) = create_subscription(http_client, scope, &subscription).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);
    receiver.wait_for_subscription_notification(subscription_id, PERIODIC_WAIT).await.body
}

/// A periodic subscription on Buildings notifying every two seconds.
fn periodic(subscription_id: &str, receiver: &NotificationReceiver) -> Value {
    with_time_interval(subscription_with_endpoint(subscription_id, &receiver.endpoint_url()), 2)
}

/// Clause 5.8.6, periodic notifications: "For each entity in the
/// notification, only the Attribute instances are to be included that match
/// the datasetId member in Subscription."
#[tokio::test]
async fn notification_periodic_carries_only_the_selected_dataset_instances() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let entity_id = scope.id("Building:r61_01");
    let (status, _, _) = create_entity(&http_client, &building_multi_instances_entity(&entity_id)).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED);

    let subscription_id = scope.id("Subscription:r61_01");
    let subscription = with_dataset_id(periodic(&subscription_id, &receiver), &["urn:ngsi-ld:Dataset:french-name"]);
    let notification = first_periodic_notification(&http_client, &scope, &receiver, &subscription_id, subscription).await;

    let entity = &notification["data"][0];
    pretty_assertions::assert_eq!(entity["id"], json!(entity_id));
    pretty_assertions::assert_eq!(
        entity["name"],
        json!({"type": "Property", "value": "Tour Eiffel", "datasetId": "urn:ngsi-ld:Dataset:french-name"})
    );
    pretty_assertions::assert_eq!(
        entity["locatedAt"],
        json!({"type": "Relationship", "object": "urn:ngsi-ld:Ville:Paris", "datasetId": "urn:ngsi-ld:Dataset:french-name"})
    );
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6 with clause 4.5.23.3: a periodic `join=flat` writes the
/// Linked Entity beside the notified one.
#[tokio::test]
async fn notification_periodic_flat_join_carries_the_linked_entity() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, city_id) = create_located_building(&http_client, &scope, "r62_01").await;

    let subscription_id = scope.id("Subscription:r62_01");
    let subscription = with_join(periodic(&subscription_id, &receiver), "flat", 1);
    let notification = first_periodic_notification(&http_client, &scope, &receiver, &subscription_id, subscription).await;

    let ids: Vec<&str> = notification["data"]
        .as_array()
        .expect("a data array")
        .iter()
        .filter_map(|entity| entity["id"].as_str())
        .collect();
    pretty_assertions::assert_eq!(ids, vec![entity_id.as_str(), city_id.as_str()]);
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6 with clause 4.5.23.2: a periodic `join=inline` nests the
/// Linked Entity under the Relationship naming it.
#[tokio::test]
async fn notification_periodic_inline_join_nests_the_linked_entity() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, city_id) = create_located_building(&http_client, &scope, "r62_02").await;

    let subscription_id = scope.id("Subscription:r62_02");
    let subscription = with_join(periodic(&subscription_id, &receiver), "inline", 1);
    let notification = first_periodic_notification(&http_client, &scope, &receiver, &subscription_id, subscription).await;

    let data = notification["data"].as_array().expect("a data array");
    pretty_assertions::assert_eq!(data.len(), 1, "{data:?}");
    pretty_assertions::assert_eq!(data[0]["id"], json!(entity_id));
    pretty_assertions::assert_eq!(data[0]["locatedAt"]["entity"]["id"], json!(city_id));
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.14.1-1: `sysAttrs` gives a periodic notification the entity's
/// `createdAt` and `modifiedAt` whatever Attributes it lists.
#[tokio::test]
async fn notification_periodic_listed_attributes_with_sys_attrs_carry_the_entity_system_times() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, _) = create_located_building(&http_client, &scope, "r09_06").await;

    let subscription_id = scope.id("Subscription:r09_06");
    let subscription = with_sys_attrs(with_notification_attributes(periodic(&subscription_id, &receiver), &["name"]), true);
    let notification = first_periodic_notification(&http_client, &scope, &receiver, &subscription_id, subscription).await;

    let entity = &notification["data"][0];
    pretty_assertions::assert_eq!(entity["id"], json!(entity_id));
    for present in ["createdAt", "modifiedAt", "name"] {
        assert!(entity.get(present).is_some(), "{present} is provided: {entity}");
    }
    assert!(entity.get("locatedAt").is_none(), "{entity}");
    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.8.6 and 4.21: `notification.omit` removes `type` from every
/// entity of a periodic notification, as it does from a change-driven one.
#[tokio::test]
async fn notification_periodic_omit_of_type_removes_the_type() {
    let http_client = client();
    let scope = TestEnvironment::from_suite("notification").await.expect("isolated test environment");
    let receiver = NotificationReceiver::start().await;
    let (entity_id, _) = create_located_building(&http_client, &scope, "r64_01").await;

    let subscription_id = scope.id("Subscription:r64_01");
    let mut subscription = periodic(&subscription_id, &receiver);
    subscription["notification"]["omit"] = json!(["type"]);
    let notification = first_periodic_notification(&http_client, &scope, &receiver, &subscription_id, subscription).await;

    let entity = &notification["data"][0];
    pretty_assertions::assert_eq!(entity["id"], json!(entity_id));
    assert!(entity.get("type").is_none(), "{entity}");
    assert!(entity.get("name").is_some(), "{entity}");
    scope.finish().await.expect("cleanup failed");
}
