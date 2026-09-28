//! One entity write owes a matching subscription one notification.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.8.6: "the notification shall be sent
//! whenever there is a change in the watched Attributes [...] The notification
//! message shall include all the subscribed Entities that changed". Table
//! 5.2.12-1: "The notification triggers listed indicate what kind of changes
//! shall trigger a notification" -- they decide whether a change is notified,
//! never how many times. A batch operation treats "the entity instances as if
//! they had all arrived in separate requests" (clause 5.5.11.0), so a batch
//! owes one notification per entity.
//!
//! The broker matched every subscription once per changed Attribute instance
//! and once more per entity-level trigger, so a subscription without
//! `watchedAttributes` received one identical notification per attribute of a
//! single create (three for `a`, `b`, `c`; 5,397 for 1,348 real `GtfsStops` of
//! four attributes each), and one listing both `entityCreated` and
//! `attributeCreated` received one more. A write that updated one attribute and
//! deleted another notified the update and the deletion separately.
//!
//! Every test counts the notifications one write produces: the owed one must
//! arrive, and no second one may follow it.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::{CapturedNotification, NotificationAdapter};
use andromeda_test_client::{entity_attribute::DatasetSelection, test_client::TestClient};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// How long an owed notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(15);

/// How long the absence of a further notification is watched for.
const QUIET: Duration = Duration::from_secs(3);

/// The NGSI-LD Null a deleted Property is notified with (clause 5.8.6).
const NULL: &str = "urn:ngsi-ld:null";

/// One Property of `value`.
fn property(value: &Value) -> Value {
    json!({"type": "Property", "value": value})
}

/// Create a subscription to `entity_type` with the given members and return
/// its identifier.
async fn subscribe(scope: &TestEnvironment, adapter: &NotificationAdapter, entity_type: &str, members: &Value) -> String {
    let id = scope.id(&format!("Subscription:{entity_type}:{}", members.to_string().len()));
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
    let created = scope.client().create_subscription(&body).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    id
}

/// The one notification the write owes `subscription`, asserting that no second
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

/// POST /entities with `a`, `b` and `c`.
async fn create_abc(client: &TestClient, id: &str, entity_type: &str) {
    let created = client
        .post_entity(&json!({
            "id": id,
            "type": entity_type,
            "a": property(&json!(1)),
            "b": property(&json!(2)),
            "c": property(&json!(3)),
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

/// A subscription with the default triggers and no `watchedAttributes` watches
/// every attribute (Table 5.2.12-1), and a create of three attributes owes it
/// one notification carrying all three.
#[tokio::test]
async fn regression_notification_per_write_a_create_of_three_attributes_notifies_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpCreate");
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({})).await;
    let id = scope.id(&format!("{entity_type}:1"));

    create_abc(scope.client(), &id, &entity_type).await;

    let notification = exactly_one(&adapter, &subscription).await;
    let entity = notified_entity(&notification);
    assert_eq!(entity["id"], json!(id));
    for name in ["a", "b", "c"] {
        assert!(entity.get(name).is_some(), "{name} is part of the one notification: {entity}");
    }

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// A subscription listing both `entityCreated` and `attributeCreated` is owed
/// one notification for one create, not one per trigger.
#[tokio::test]
async fn regression_notification_per_write_entity_and_attribute_triggers_together_notify_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpTriggers");
    let subscription = subscribe(
        &scope,
        &adapter,
        &entity_type,
        &json!({"notificationTrigger": ["entityCreated", "attributeCreated", "attributeUpdated"]}),
    )
    .await;
    let id = scope.id(&format!("{entity_type}:1"));

    create_abc(scope.client(), &id, &entity_type).await;
    exactly_one(&adapter, &subscription).await;

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.11.0: a Batch Create owes one notification per entity, and each
/// entity's several attributes still owe it only one.
#[tokio::test]
async fn regression_notification_per_write_a_batch_create_notifies_once_per_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpBatch");
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({})).await;
    let first = scope.id(&format!("{entity_type}:1"));
    let second = scope.id(&format!("{entity_type}:2"));
    let entity = |id: &str| json!({"id": id, "type": entity_type, "a": property(&json!(1)), "b": property(&json!(2)), "@context": CORE_CONTEXT});

    let created = scope
        .client()
        .batch_create(&json!([entity(&first), entity(&second)]))
        .await
        .expect("batch create request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let mut notified = vec![
        notified_entity(&adapter.wait_for_subscription_notification(&subscription, ARRIVAL).await)["id"].clone(),
        notified_entity(&adapter.wait_for_subscription_notification(&subscription, ARRIVAL).await)["id"].clone(),
    ];
    adapter.wait_for_no_subscription_notification(&subscription, QUIET).await;
    notified.sort_by_key(ToString::to_string);
    assert_eq!(notified, vec![json!(first), json!(second)]);

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.8: a Batch Upsert that creates an entity, and one that updates
/// two of its attributes, each owe one notification.
#[tokio::test]
async fn regression_notification_per_write_a_batch_upsert_create_and_update_each_notify_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpUpsert");
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({})).await;
    let id = scope.id(&format!("{entity_type}:1"));
    let entity = |a: i64, b: i64| json!([{"id": id, "type": entity_type, "a": property(&json!(a)), "b": property(&json!(b)), "@context": CORE_CONTEXT}]);

    let created = scope.client().batch_upsert(&entity(1, 2)).await.expect("batch upsert request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    exactly_one(&adapter, &subscription).await;

    let updated = scope
        .client()
        .raw(Method::POST, "/ngsi-ld/v1/entityOperations/upsert?options=update")
        .header("content-type", "application/ld+json")
        .json(&entity(10, 20))
        .send()
        .await
        .expect("batch upsert request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);
    let notification = exactly_one(&adapter, &subscription).await;
    let notified = notified_entity(&notification);
    assert_eq!(notified["a"]["value"], json!(10), "{notified}");
    assert_eq!(notified["b"]["value"], json!(20), "{notified}");

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.17: a Merge Entity changing two attributes owes one notification.
#[tokio::test]
async fn regression_notification_per_write_a_merge_of_two_attributes_notifies_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpMerge");
    let id = scope.id(&format!("{entity_type}:1"));
    create_abc(scope.client(), &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({})).await;

    let merged = scope
        .client()
        .merge_entity(
            &id,
            &json!({"a": property(&json!(10)), "d": property(&json!(4)), "@context": CORE_CONTEXT}),
            None,
        )
        .await
        .expect("merge entity request");
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "{:?}", merged.body);
    exactly_one(&adapter, &subscription).await;

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// A Replace Entity that creates `d`, updates `a` and removes `b` and `c` owes
/// a subscription listening to all three attribute triggers one notification,
/// which reports the removed attributes as NGSI-LD Null beside the changed ones
/// (clause 5.8.6).
#[tokio::test]
async fn regression_notification_per_write_a_replace_that_creates_updates_and_deletes_notifies_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpReplace");
    let id = scope.id(&format!("{entity_type}:1"));
    create_abc(scope.client(), &id, &entity_type).await;
    let subscription = subscribe(
        &scope,
        &adapter,
        &entity_type,
        &json!({"notificationTrigger": ["attributeCreated", "attributeUpdated", "attributeDeleted"]}),
    )
    .await;

    let replaced = scope
        .client()
        .replace_entity(
            &id,
            &json!({"id": id, "type": entity_type, "a": property(&json!(10)), "d": property(&json!(4)), "@context": CORE_CONTEXT}),
            "application/ld+json",
            None,
        )
        .await
        .expect("replace entity request");
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "{:?}", replaced.body);

    let notification = exactly_one(&adapter, &subscription).await;
    let entity = notified_entity(&notification);
    assert_eq!(entity["a"]["value"], json!(10), "{entity}");
    assert_eq!(entity["d"]["value"], json!(4), "{entity}");
    for removed in ["b", "c"] {
        let member = &entity[removed];
        let nulled = member.as_str() == Some(NULL) || member["value"].as_str() == Some(NULL);
        assert!(nulled, "{removed} is notified as NGSI-LD Null: {entity}");
    }

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.1.4: `watchedAttributes` decides whether a write is notified; a
/// create touching one watched attribute among three owes one notification,
/// and an update of an unwatched attribute owes none.
#[tokio::test]
async fn regression_notification_per_write_watched_attributes_gate_a_single_notification() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpWatched");
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({"watchedAttributes": ["b"]})).await;
    let id = scope.id(&format!("{entity_type}:1"));

    create_abc(scope.client(), &id, &entity_type).await;
    exactly_one(&adapter, &subscription).await;

    let updated = scope
        .client()
        .update_entity_attrs(&id, &json!({"a": property(&json!(10)), "@context": CORE_CONTEXT}))
        .await
        .expect("update attributes request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "{:?}", updated.body);
    adapter.wait_for_no_subscription_notification(&subscription, QUIET).await;

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.5: instances of one attribute under several `datasetId`s are
/// several changed instances of one write, and still one notification that
/// carries them all.
#[tokio::test]
async fn regression_notification_per_write_several_dataset_instances_notify_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpDataset");
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({})).await;
    let id = scope.id(&format!("{entity_type}:1"));

    let created = scope
        .client()
        .post_entity(&json!({
            "id": id,
            "type": entity_type,
            "speed": [
                {"type": "Property", "value": 1},
                {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:gps"},
                {"type": "Property", "value": 3, "datasetId": "urn:ngsi-ld:Dataset:radar"}
            ],
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);

    let notification = exactly_one(&adapter, &subscription).await;
    let entity = notified_entity(&notification);
    assert_eq!(entity["speed"].as_array().map(Vec::len), Some(3), "every instance is carried: {entity}");

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Deleting every `datasetId` instance of an attribute in one request owes one
/// notification, which carries each deleted instance as NGSI-LD Null with its
/// `datasetId` (clause 5.8.6: "a JSON object with all the required
/// information is provided").
#[tokio::test]
async fn regression_notification_per_write_deleting_every_dataset_instance_notifies_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpDeleteAll");
    let id = scope.id(&format!("{entity_type}:1"));
    let created = scope
        .client()
        .post_entity(&json!({
            "id": id,
            "type": entity_type,
            "keep": property(&json!(0)),
            "speed": [
                {"type": "Property", "value": 1},
                {"type": "Property", "value": 2, "datasetId": "urn:ngsi-ld:Dataset:gps"}
            ],
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let subscription = subscribe(&scope, &adapter, &entity_type, &json!({"notificationTrigger": ["attributeDeleted"]})).await;

    let deleted = scope
        .client()
        .delete_entity_attr(&id, "speed", None, DatasetSelection::All, Some(CORE_CONTEXT))
        .await
        .expect("delete attribute request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    let notification = exactly_one(&adapter, &subscription).await;
    let entity = notified_entity(&notification);
    let instances = entity["speed"].as_array().expect("two deleted instances are an array");
    assert_eq!(instances.len(), 2, "{entity}");
    assert!(instances.iter().all(|instance| instance["value"] == json!(NULL)), "{entity}");
    assert!(
        instances.iter().any(|instance| instance["datasetId"] == json!("urn:ngsi-ld:Dataset:gps")),
        "the named instance carries its datasetId: {entity}"
    );

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6: deleting an entity owes a subscription listening to both
/// `entityDeleted` and `attributeDeleted` one notification, carrying the
/// `deletedAt` of the entity alone.
#[tokio::test]
async fn regression_notification_per_write_an_entity_deletion_notifies_once() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("NpDelete");
    let id = scope.id(&format!("{entity_type}:1"));
    create_abc(scope.client(), &id, &entity_type).await;
    let subscription = subscribe(
        &scope,
        &adapter,
        &entity_type,
        &json!({"notificationTrigger": ["entityDeleted", "attributeDeleted"]}),
    )
    .await;

    let deleted = scope.client().delete_entity(&id).await.expect("delete entity request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    let notification = exactly_one(&adapter, &subscription).await;
    let entity = notified_entity(&notification);
    assert_eq!(entity["id"], json!(id));
    assert!(entity.get("deletedAt").is_some(), "{entity}");
    assert!(entity.get("a").is_none(), "an entity deletion carries deletedAt alone: {entity}");

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
