//! The notifications a Temporal API write owes when it changes current state.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.3.1: "The Core API manages the current state
//! of Entities", and Context Information Subscription (clauses 5.8.1 to 5.8.6)
//! is part of it, so a subscription watches current state. Clause 5.8.1.4
//! notifies "whenever there is a change in the watched Attribute nodes", and
//! clause 5.8.6 defines that change on current state alone: "An Attribute is
//! considered to change when any of the members (including children) in its
//! corresponding JSON-LD node is updated with a value different than the
//! existing one". Nothing in either clause asks which API made the change.
//!
//! A Temporal API create of an unknown identifier (clause 5.6.11) brings the
//! Entity into current state, and an append of a newer observation (clauses
//! 5.6.11 and 5.6.12, recency per clause 4.5.5.3) advances it; both change
//! current state and owe a notification. A backfill of older observations,
//! and every operation that only edits the Temporal Evolution (clauses 5.6.13
//! to 5.6.16), leave current state as it was and owe nothing.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::{CapturedNotification, NotificationAdapter};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// How long an owed notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(15);

/// How long the absence of a notification is watched for.
const QUIET: Duration = Duration::from_secs(3);

/// One Property observation of `value` at `observed_at`.
fn observation(value: i64, observed_at: &str) -> Value {
    json!({"type": "Property", "value": value, "observedAt": observed_at})
}

/// Create a subscription to `entity_type` with the given extra members and
/// notification members, returning its identifier.
async fn subscribe(scope: &TestEnvironment, adapter: &NotificationAdapter, entity_type: &str, members: &Value, notification: &Value) -> String {
    let id = scope.id(&format!("Subscription:{entity_type}"));
    let mut body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "notification": {"endpoint": {"uri": adapter.endpoint_url(), "accept": "application/json"}},
        "@context": CORE_CONTEXT
    });
    for (key, value) in members.as_object().expect("the extra members are an object") {
        body[key] = value.clone();
    }
    for (key, value) in notification.as_object().expect("the notification members are an object") {
        body["notification"][key] = value.clone();
    }
    let created = scope.client().create_subscription(&body).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    id
}

/// The one notification a write owes `subscription`, asserting that no second
/// one follows it.
async fn exactly_one(adapter: &NotificationAdapter, subscription: &str) -> CapturedNotification {
    let owed = adapter.wait_for_subscription_notification(subscription, ARRIVAL).await;
    adapter.wait_for_no_subscription_notification(subscription, QUIET).await;
    owed
}

/// The one entity a notification carries.
fn notified_entity(notification: &CapturedNotification) -> &Value {
    let data = notification.body["data"].as_array().expect("a notification carries data");
    assert_eq!(data.len(), 1, "one entity per notification: {}", notification.body);
    &data[0]
}

/// POST /temporal/entities, asserting clause 6.20.3.1's status.
async fn post_temporal(client: &TestClient, body: &Value, expected: StatusCode) {
    let response = client.create_temporal_entity(body).await.expect("create temporal entity request");
    assert_eq!(response.status, expected, "{:?}", response.body);
}

/// POST /temporal/entities/{id}/attrs (clause 5.6.12) with `attributes`.
async fn append_temporal(client: &TestClient, id: &str, attributes: &Value) {
    let mut body = attributes.clone();
    body["@context"] = json!(CORE_CONTEXT);
    let appended = client
        .raw(Method::POST, &format!("/ngsi-ld/v1/temporal/entities/{}/attrs", urlencoding::encode(id)))
        .header("content-type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("append temporal attributes request");
    assert_eq!(appended.status, StatusCode::NO_CONTENT, "{:?}", appended.body);
}

/// POST /entities with `v` observed at the start of 2021.
async fn create_core(client: &TestClient, id: &str, entity_type: &str) {
    let created = client
        .post_entity(&json!({"id": id, "type": entity_type, "v": observation(5, "2021-01-01T00:00:00Z"), "@context": CORE_CONTEXT}))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

/// Clause 5.6.11.4 on an unknown identifier brings the Entity into current
/// state with its most recent observation (clause 4.5.5.3), which a
/// subscription with the default triggers is notified of once, carrying the
/// Entity as it now stands.
#[tokio::test]
async fn regression_temporal_write_notification_a_temporal_create_notifies_the_created_entity_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("TwnCreate");
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({}), &json!({})).await;
    let id = scope.id(&format!("{entity_type}:1"));

    let series = json!({
        "id": id,
        "type": entity_type,
        "v": [observation(10, "2020-01-01T00:00:00Z"), observation(50, "2020-01-03T00:00:00Z"), observation(20, "2020-01-02T00:00:00Z")],
        "@context": CORE_CONTEXT
    });
    post_temporal(scope.client(), &series, StatusCode::CREATED).await;

    let notification = exactly_one(&adapter, &subscription).await;
    let entity = notified_entity(&notification);
    assert_eq!(entity["id"], json!(id));
    assert_eq!(
        entity["v"]["value"],
        json!(50),
        "the notification carries current state, not the series: {entity}"
    );

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-1: a subscription listening to `entityCreated` alone is
/// notified of the temporal create and of nothing an append does to the
/// Entity afterwards.
#[tokio::test]
async fn regression_temporal_write_notification_an_entity_created_subscription_hears_the_create_and_not_the_append() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("TwnCreatedOnly");
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({"notificationTrigger": ["entityCreated"]}), &json!({})).await;
    let id = scope.id(&format!("{entity_type}:1"));

    post_temporal(
        scope.client(),
        &json!({"id": id, "type": entity_type, "v": [observation(1, "2020-01-01T00:00:00Z")], "@context": CORE_CONTEXT}),
        StatusCode::CREATED,
    )
    .await;
    exactly_one(&adapter, &subscription).await;

    append_temporal(scope.client(), &id, &json!({"v": [observation(2, "2030-01-01T00:00:00Z")]})).await;
    adapter.wait_for_no_subscription_notification(&subscription, QUIET).await;

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.12.4 with clause 4.5.5.3: an appended observation more recent
/// than current state replaces it, an `attributeUpdated` change notified once
/// with the new value and, under `showChanges`, the value it replaced
/// (clause 5.8.6).
#[tokio::test]
async fn regression_temporal_write_notification_a_newer_append_notifies_the_update_with_its_previous_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("TwnAppend");
    let id = scope.id(&format!("{entity_type}:1"));
    create_core(scope.client(), &id, &entity_type).await;
    let subscription = subscribe(
        &scope,
        &adapter,
        &entity_type,
        &json!({"notificationTrigger": ["attributeUpdated"]}),
        &json!({"showChanges": true}),
    )
    .await;

    append_temporal(scope.client(), &id, &json!({"v": [observation(200, "2030-01-01T00:00:00Z")]})).await;

    let notification = exactly_one(&adapter, &subscription).await;
    let entity = notified_entity(&notification);
    assert_eq!(entity["v"]["value"], json!(200), "{entity}");
    assert_eq!(entity["v"]["previousValue"], json!(5), "showChanges carries the replaced instance: {entity}");

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.11.4 on an existing identifier is an append: a newer observation
/// of a new attribute is an `attributeCreated` change of current state.
#[tokio::test]
async fn regression_temporal_write_notification_an_upsert_adding_an_attribute_notifies_its_creation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("TwnUpsert");
    let id = scope.id(&format!("{entity_type}:1"));
    create_core(scope.client(), &id, &entity_type).await;
    let subscription = subscribe(
        &scope,
        &adapter,
        &entity_type,
        &json!({"notificationTrigger": ["attributeCreated"]}),
        &json!({}),
    )
    .await;

    post_temporal(
        scope.client(),
        &json!({"id": id, "type": entity_type, "w": [observation(7, "2022-01-01T00:00:00Z")], "@context": CORE_CONTEXT}),
        StatusCode::NO_CONTENT,
    )
    .await;

    let notification = exactly_one(&adapter, &subscription).await;
    let entity = notified_entity(&notification);
    assert_eq!(entity["w"]["value"], json!(7), "{entity}");

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// A backfill of observations older than current state leaves it as it was
/// (clause 4.5.5.3), through either temporal append, so it is no change under
/// clause 5.8.6 and owes no notification.
#[tokio::test]
async fn regression_temporal_write_notification_a_backfill_notifies_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("TwnBackfill");
    let id = scope.id(&format!("{entity_type}:1"));
    create_core(scope.client(), &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({}), &json!({})).await;

    append_temporal(scope.client(), &id, &json!({"v": [observation(1, "2019-01-01T00:00:00Z")]})).await;
    post_temporal(
        scope.client(),
        &json!({"id": id, "type": entity_type, "v": [observation(2, "2019-06-01T00:00:00Z")], "@context": CORE_CONTEXT}),
        StatusCode::NO_CONTENT,
    )
    .await;

    adapter.wait_for_no_subscription_notification(&subscription, QUIET).await;

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.1.4: `watchedAttributes` decides whether a change is notified,
/// whichever API made it; advancing an unwatched attribute owes nothing.
#[tokio::test]
async fn regression_temporal_write_notification_an_unwatched_attribute_append_notifies_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("TwnWatched");
    let id = scope.id(&format!("{entity_type}:1"));
    create_core(scope.client(), &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({"watchedAttributes": ["w"]}), &json!({})).await;

    append_temporal(scope.client(), &id, &json!({"v": [observation(9, "2030-01-01T00:00:00Z")]})).await;
    adapter.wait_for_no_subscription_notification(&subscription, QUIET).await;

    append_temporal(scope.client(), &id, &json!({"w": [observation(3, "2030-01-01T00:00:00Z")]})).await;
    exactly_one(&adapter, &subscription).await;

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.13 to 5.6.16 edit the Temporal Evolution alone: modifying or
/// deleting an instance, deleting an attribute's history, or deleting the
/// whole Temporal Evolution leaves current state untouched and owes no
/// notification -- in particular no `entityDeleted`.
#[tokio::test]
async fn regression_temporal_write_notification_history_only_operations_notify_nothing() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("TwnHistoryOnly");
    let id = scope.id(&format!("{entity_type}:1"));
    post_temporal(
        scope.client(),
        &json!({
            "id": id,
            "type": entity_type,
            "v": [observation(1, "2020-01-01T00:00:00Z"), observation(2, "2020-01-02T00:00:00Z")],
            "w": [observation(3, "2020-01-01T00:00:00Z")],
            "@context": CORE_CONTEXT
        }),
        StatusCode::CREATED,
    )
    .await;
    let subscription = subscribe(
        &scope,
        &adapter,
        &entity_type,
        &json!({"notificationTrigger": ["entityUpdated", "entityDeleted", "attributeCreated", "attributeUpdated", "attributeDeleted"]}),
        &json!({}),
    )
    .await;

    let history = scope
        .client()
        .get_temporal_entity(&id, "attrs=v", Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(history.status, StatusCode::OK, "{:?}", history.body);
    let body = history.json_value().expect("a temporal answer is JSON");
    let instance_id = body["v"]
        .as_array()
        .and_then(|instances| instances.iter().find(|instance| instance["value"] == json!(2)))
        .and_then(|instance| instance["instanceId"].as_str())
        .expect("the standing observation's instanceId")
        .to_owned();
    let encoded_id = urlencoding::encode(&id).into_owned();

    // Clause 5.6.14: correcting the instance current state holds.
    let modified = scope
        .client()
        .raw(
            Method::PATCH,
            &format!("/ngsi-ld/v1/temporal/entities/{encoded_id}/attrs/v/{}", urlencoding::encode(&instance_id)),
        )
        .header("content-type", "application/ld+json")
        .json(&json!({"v": [{"value": 99}], "@context": [CORE_CONTEXT]}))
        .send()
        .await
        .expect("modify attribute instance request");
    assert_eq!(modified.status, StatusCode::NO_CONTENT, "{:?}", modified.body);

    // Clause 5.6.15: deleting that instance from the Temporal Evolution.
    let deleted_instance = scope
        .client()
        .raw(
            Method::DELETE,
            &format!("/ngsi-ld/v1/temporal/entities/{encoded_id}/attrs/v/{}", urlencoding::encode(&instance_id)),
        )
        .send()
        .await
        .expect("delete attribute instance request");
    assert_eq!(deleted_instance.status, StatusCode::NO_CONTENT, "{:?}", deleted_instance.body);

    // Clause 5.6.13: deleting the history of `w`.
    let deleted_attribute = scope
        .client()
        .raw(Method::DELETE, &format!("/ngsi-ld/v1/temporal/entities/{encoded_id}/attrs/w"))
        .send()
        .await
        .expect("delete temporal attribute request");
    assert_eq!(deleted_attribute.status, StatusCode::NO_CONTENT, "{:?}", deleted_attribute.body);

    // Clause 5.6.16: deleting the whole Temporal Evolution.
    let deleted = scope.client().delete_temporal_entity(&id).await.expect("delete temporal entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    adapter.wait_for_no_subscription_notification(&subscription, QUIET).await;

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
