//! A `deleteAll` deletion notifies every deleted Attribute instance.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.6.5.4: "If the deleteAll flag is set, remove
//! all target Attribute instances from the target Entity". Clause 5.8.6 notifies
//! a deleted Attribute as NGSI-LD Null "unless more information is required. The
//! latter is the case, if: a datasetId needs to be provided", and keeps "only the
//! Attribute instances [...] that match the datasetId member in Subscription".
//! Clause 4.5.5 reads an instance without `datasetId` as the default instance and
//! returns several instances "as arrays of Attributes, instead of a single
//! Attribute element".
//!
//! One nullified object for the whole Attribute would claim that only the
//! default instance was deleted, and a Subscription `datasetId` would filter it
//! away. The notification therefore carries each deleted instance, nullified,
//! with its own `datasetId`, and a Subscription `datasetId` narrows those
//! instances like any others.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::{CapturedNotification, NotificationAdapter};
use andromeda_test_client::entity_attribute::DatasetSelection;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// The core context every payload here is written against.
const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

/// How long the owed notification is waited for.
const ARRIVAL: Duration = Duration::from_secs(15);

/// How long the absence of a further notification is watched for.
const QUIET: Duration = Duration::from_secs(3);

/// The NGSI-LD Null a deleted instance is notified with (clause 5.8.6).
const NULL: &str = "urn:ngsi-ld:null";

/// The first named dataset of the fixture.
const FRENCH: &str = "urn:ngsi-ld:Dataset:french";

/// The second named dataset of the fixture.
const GERMAN: &str = "urn:ngsi-ld:Dataset:german";

/// Create the fixture entity: `label` with a default, a French and a German
/// instance, and `hosts` with a default and a French instance.
async fn create_fixture(scope: &TestEnvironment, id: &str, entity_type: &str) {
    let created = scope
        .client()
        .post_entity(&json!({
            "id": id,
            "type": entity_type,
            "label": [
                {"type": "Property", "value": "Main"},
                {"type": "Property", "value": "Principal", "datasetId": FRENCH},
                {"type": "Property", "value": "Haupt", "datasetId": GERMAN}
            ],
            "hosts": [
                {"type": "Relationship", "object": "urn:ngsi-ld:Room:1"},
                {"type": "Relationship", "object": "urn:ngsi-ld:Room:2", "datasetId": FRENCH}
            ],
            "@context": CORE_CONTEXT
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
}

/// Subscribe to `attributeDeleted` of `watched` on `entity_type`, with the
/// extra Subscription members in `members` and the extra `NotificationParams`
/// members in `notification`, and return the subscription identifier.
async fn subscribe(scope: &TestEnvironment, adapter: &NotificationAdapter, entity_type: &str, watched: &str, members: &Value, notification: &Value) -> String {
    let id = scope.id(&format!("Subscription:{entity_type}"));
    let mut body = json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": entity_type}],
        "watchedAttributes": [watched],
        "notificationTrigger": ["attributeDeleted"],
        "notification": {"format": "normalized", "endpoint": {"uri": adapter.endpoint_url(), "accept": "application/json"}},
        "@context": CORE_CONTEXT
    });
    for (key, value) in members.as_object().expect("the Subscription members are an object") {
        body[key] = value.clone();
    }
    for (key, value) in notification.as_object().expect("the NotificationParams members are an object") {
        body["notification"][key] = value.clone();
    }
    let created = scope.client().create_subscription(&body).await.expect("create subscription request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    id
}

/// DELETE every instance of `attribute` with `deleteAll=true`.
async fn delete_all(scope: &TestEnvironment, id: &str, attribute: &str) {
    let deleted = scope
        .client()
        .delete_entity_attr(id, attribute, None, DatasetSelection::All, Some(CORE_CONTEXT))
        .await
        .expect("delete attribute request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);
}

/// The one entity of the one notification the deletion owes `subscription`,
/// asserting that no second notification follows it.
async fn notified_entity(adapter: &NotificationAdapter, subscription: &str) -> Value {
    let notification: CapturedNotification = adapter.wait_for_subscription_notification(subscription, ARRIVAL).await;
    adapter.wait_for_no_subscription_notification(subscription, QUIET).await;
    let data = notification.body["data"].as_array().expect("a notification carries data");
    assert_eq!(data.len(), 1, "one entity per notification: {}", notification.body);
    data[0].clone()
}

/// The `datasetId` of every instance of `member`, sorted, `None` for the
/// default instance.
fn dataset_ids(member: &Value) -> Vec<Option<&str>> {
    let mut ids: Vec<Option<&str>> = member
        .as_array()
        .expect("several instances are an array (clause 4.5.5)")
        .iter()
        .map(|instance| instance.get("datasetId").and_then(Value::as_str))
        .collect();
    ids.sort_unstable();
    ids
}

/// Every deleted instance is notified, nullified, the named ones with their
/// `datasetId`, and the Attribute the deletion did not touch keeps its
/// instances.
#[tokio::test]
async fn regression_delete_all_notification_every_deleted_instance_is_nullified_with_its_dataset_id() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("DaEvery");
    let id = scope.id(&format!("{entity_type}:1"));
    create_fixture(&scope, &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, "label", &json!({}), &json!({})).await;

    delete_all(&scope, &id, "label").await;

    let entity = notified_entity(&adapter, &subscription).await;
    assert_eq!(entity["id"], json!(id));
    assert_eq!(dataset_ids(&entity["label"]), vec![None, Some(FRENCH), Some(GERMAN)], "{entity}");
    let label = entity["label"].as_array().expect("three deleted instances");
    assert!(
        label.iter().all(|instance| instance["value"] == json!(NULL)),
        "every instance is NGSI-LD Null: {entity}"
    );
    assert!(label.iter().all(|instance| instance["type"] == json!("Property")), "{entity}");
    assert_eq!(
        dataset_ids(&entity["hosts"]),
        vec![None, Some(FRENCH)],
        "the untouched Attribute is notified whole: {entity}"
    );

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// A Subscription `datasetId` selects among the deleted instances as among any
/// others (clause 5.8.6), so it is told about the French instance alone, as a
/// single Attribute element (clause 4.5.5).
#[tokio::test]
async fn regression_delete_all_notification_a_subscription_dataset_id_keeps_only_that_deleted_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("DaNamed");
    let id = scope.id(&format!("{entity_type}:1"));
    create_fixture(&scope, &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, "label", &json!({"datasetId": [FRENCH]}), &json!({})).await;

    delete_all(&scope, &id, "label").await;

    let entity = notified_entity(&adapter, &subscription).await;
    assert_eq!(
        entity["label"],
        json!({"type": "Property", "value": NULL, "datasetId": FRENCH}),
        "only the French instance: {entity}"
    );

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// `@none` in the Subscription `datasetId` selects the default instance
/// (Table 5.2.12-1), which is notified without a `datasetId`.
#[tokio::test]
async fn regression_delete_all_notification_the_none_dataset_id_keeps_only_the_default_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("DaDefault");
    let id = scope.id(&format!("{entity_type}:1"));
    create_fixture(&scope, &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, "label", &json!({"datasetId": ["@none"]}), &json!({})).await;

    delete_all(&scope, &id, "label").await;

    let entity = notified_entity(&adapter, &subscription).await;
    assert_eq!(
        entity["label"],
        json!({"type": "Property", "value": NULL}),
        "only the default instance: {entity}"
    );

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// With `notification.sysAttrs` every deleted instance carries the system
/// generated sub-attributes, `deletedAt` among them (clauses 5.8.6 and 4.8).
#[tokio::test]
async fn regression_delete_all_notification_sys_attrs_stamp_every_deleted_instance_with_deleted_at() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("DaSysAttrs");
    let id = scope.id(&format!("{entity_type}:1"));
    create_fixture(&scope, &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, "label", &json!({}), &json!({"sysAttrs": true})).await;

    delete_all(&scope, &id, "label").await;

    let entity = notified_entity(&adapter, &subscription).await;
    assert_eq!(dataset_ids(&entity["label"]), vec![None, Some(FRENCH), Some(GERMAN)], "{entity}");
    let label = entity["label"].as_array().expect("three deleted instances");
    assert!(label.iter().all(|instance| instance["value"] == json!(NULL)), "{entity}");
    assert!(
        label.iter().all(|instance| instance["deletedAt"].is_string()),
        "every instance carries deletedAt: {entity}"
    );

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// With `notification.showChanges` every deleted instance carries its own
/// previous value (clause 5.8.6), never another instance's.
#[tokio::test]
async fn regression_delete_all_notification_show_changes_carries_each_instance_previous_value() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("DaShowChanges");
    let id = scope.id(&format!("{entity_type}:1"));
    create_fixture(&scope, &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, "label", &json!({}), &json!({"showChanges": true})).await;

    delete_all(&scope, &id, "label").await;

    let entity = notified_entity(&adapter, &subscription).await;
    let mut previous: Vec<(Option<&str>, &Value)> = entity["label"]
        .as_array()
        .expect("three deleted instances")
        .iter()
        .map(|instance| (instance.get("datasetId").and_then(Value::as_str), &instance["previousValue"]))
        .collect();
    previous.sort_unstable_by_key(|(dataset, _)| *dataset);
    assert_eq!(
        previous,
        vec![(None, &json!("Main")), (Some(FRENCH), &json!("Principal")), (Some(GERMAN), &json!("Haupt"))],
        "{entity}"
    );

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}

/// A Relationship's deleted instances are nullified in their `object`
/// (clause 5.8.6: "the value or the object is set to the URI
/// 'urn:ngsi-ld:null' respectively"), each keeping its `datasetId`.
#[tokio::test]
async fn regression_delete_all_notification_deleted_relationship_instances_are_nullified_in_their_object() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let adapter = NotificationAdapter::start().await;
    let entity_type = scope.entity_type("DaRelationship");
    let id = scope.id(&format!("{entity_type}:1"));
    create_fixture(&scope, &id, &entity_type).await;
    let subscription = subscribe(&scope, &adapter, &entity_type, "hosts", &json!({}), &json!({})).await;

    delete_all(&scope, &id, "hosts").await;

    let entity = notified_entity(&adapter, &subscription).await;
    assert_eq!(dataset_ids(&entity["hosts"]), vec![None, Some(FRENCH)], "{entity}");
    let hosts = entity["hosts"].as_array().expect("two deleted instances");
    assert!(
        hosts
            .iter()
            .all(|instance| instance["type"] == json!("Relationship") && instance["object"] == json!(NULL)),
        "every instance is nullified in its object: {entity}"
    );
    assert!(
        hosts.iter().all(|instance| instance.get("value").is_none()),
        "a Relationship gains no value: {entity}"
    );

    adapter.shutdown().await;
    scope.finish().await.expect("cleanup failed");
}
