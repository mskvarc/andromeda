//! The two list attribute families, end to end.
//!
//! Clauses 4.5.21 and 4.5.22 give a `ListProperty` and a `ListRelationship`
//! their own value members, `valueList` and `objectList`, and prohibit the
//! `value` / `object` those specialize. Clause 4.5.0 spells a deletion of one
//! as an array holding a single NGSI-LD Null, and clause 4.5.9 collapses them
//! into `valueLists` and `objectLists` in the simplified temporal
//! representation. Every test here asserts what those clauses require.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{entity_attribute::DatasetSelection, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// The NGSI-LD Null of clause 4.5.0.
const NULL_URI: &str = "urn:ngsi-ld:null";

/// How long one notification is waited for.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(15);

/// How many deliveries are read before giving up on the one under test: an
/// entity creation and the change being tested can both match a subscription,
/// and only the second carries what is asserted.
const DELIVERIES_READ: usize = 4;

/// Create a subscription over `Building` entities delivering to `receiver`.
async fn create_subscription(scope: &TestEnvironment, receiver: &NotificationServer, local_id: &str, triggers: &[&str], show_changes: bool) {
    let subscription = json!({
        "id": scope.id(local_id),
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notificationTrigger": triggers,
        "notification": {
            "showChanges": show_changes,
            "endpoint": {
                "uri": receiver.endpoint().expect("notification endpoint must be valid"),
                "accept": "application/json"
            }
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().create_subscription(&subscription).await.expect("create subscription failed");
    assert_eq!(created.status, StatusCode::CREATED, "subscription create: {:?}", created.body);
}

/// The first delivered notification whose entity carries `member` inside
/// `attribute`, so an earlier delivery for the same subscription cannot be
/// mistaken for the change under test.
async fn notification_carrying(receiver: &NotificationServer, attribute: &str, member: &str) -> Value {
    let mut carrying = None;
    for _ in 0..DELIVERIES_READ {
        let captured = receiver.next_request(DELIVERY_TIMEOUT).await.expect("a notification must arrive");
        let body: Value = captured.json().expect("a notification body is JSON");
        if body["data"][0][attribute].get(member).is_some() {
            carrying = Some(body);
            break;
        }
    }
    assert!(carrying.is_some(), "no delivered notification carried '{member}' inside '{attribute}'");
    carrying.expect("the delivery asserted above")
}

/// Create one entity and assert it was stored.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// A `Building` carrying one `ListProperty` and one `ListRelationship`.
fn building_with_list_attributes(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "tags": {"type": "ListProperty", "valueList": ["residential", "commercial"]},
        "hasRooms": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Clause 4.5.21.2: `previousValueList` "represents the previous `ListProperty`
/// `valueList`, before the triggering change. The representation is the same as
/// that of `valueList`" — the previous ordered array itself, not the previous
/// instance that held it. The same clause prohibits `value` and
/// `previousValue` on the family, "as value is a generalization of `valueList`".
#[tokio::test]
async fn regression_show_changes_on_a_list_property_carries_the_previous_value_list_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");
    let id = scope.id("Building:list-previous-value-list");

    create_entity(scope.client(), &building_with_list_attributes(&id)).await;
    create_subscription(&scope, &receiver, "Subscription:list-previous-value-list", &["attributeUpdated"], true).await;

    let updated = scope
        .client()
        .partial_update_attr(
            &id,
            "tags",
            &json!({"type": "ListProperty", "valueList": ["industrial"], "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("partial update failed");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "partial update: {:?}", updated.body);

    let body = notification_carrying(&receiver, "tags", "previousValueList").await;
    let tags = &body["data"][0]["tags"];
    assert_eq!(
        tags["previousValueList"],
        json!(["residential", "commercial"]),
        "clause 4.5.21.2: previousValueList is the previous valueList array: {tags}"
    );
    assert!(tags.get("previousValue").is_none(), "clause 4.5.21.2 prohibits previousValue: {tags}");
    assert!(tags.get("value").is_none(), "clause 4.5.21.2 prohibits value: {tags}");

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.22.2: `previousObjectList` "represents the previous
/// `ListRelationship` `objectList`, before the triggering change. The
/// representation is the same as that of `objectList`", and `object` /
/// `previousObject` "shall never be present as object is a generalization of
/// `objectList`".
#[tokio::test]
async fn regression_show_changes_on_a_list_relationship_carries_the_previous_object_list_array() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");
    let id = scope.id("Building:list-previous-object-list");

    create_entity(scope.client(), &building_with_list_attributes(&id)).await;
    create_subscription(&scope, &receiver, "Subscription:list-previous-object-list", &["attributeUpdated"], true).await;

    let updated = scope
        .client()
        .partial_update_attr(
            &id,
            "hasRooms",
            &json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:3"], "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("partial update failed");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "partial update: {:?}", updated.body);

    let body = notification_carrying(&receiver, "hasRooms", "previousObjectList").await;
    let rooms = &body["data"][0]["hasRooms"];
    assert_eq!(
        rooms["previousObjectList"],
        json!(["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"]),
        "clause 4.5.22.2: previousObjectList is the previous objectList array: {rooms}"
    );
    assert!(rooms.get("previousObject").is_none(), "clause 4.5.22.2 prohibits previousObject: {rooms}");
    assert!(rooms.get("object").is_none(), "clause 4.5.22.2 prohibits object: {rooms}");

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.21.2: "An array consisting of a single NGSI-LD Null ... can be
/// used as the right-hand side of the `valueList` during partial update patch
/// and merge patch (see clauses 5.5.8 and 5.5.12) to indicate a deletion of the
/// `ListProperty`". Clause 4.5.0 fixes that spelling: where an array is required,
/// it is an array with one single NGSI-LD Null.
#[tokio::test]
async fn regression_a_null_value_list_partial_update_deletes_the_list_property() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:list-property-null-delete");

    create_entity(client, &building_with_list_attributes(&id)).await;

    let patched = client
        .partial_update_attr(
            &id,
            "tags",
            &json!({"type": "ListProperty", "valueList": [NULL_URI], "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("partial update failed");
    assert_eq!(
        patched.status,
        StatusCode::NO_CONTENT,
        "clause 5.5.8: deleting a ListProperty with the NGSI-LD Null answers 204: {:?}",
        patched.body
    );

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let body = retrieved.json_value().expect("the entity is JSON");
    assert!(
        body.get("tags").is_none(),
        "clause 4.5.21.2: a valueList holding one NGSI-LD Null deletes the ListProperty: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.22.2: the same single-null array on the right-hand side of
/// `objectList` "indicate[s] a deletion of the `ListRelationship`".
#[tokio::test]
async fn regression_a_null_object_list_partial_update_deletes_the_list_relationship() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:list-relationship-null-delete");

    create_entity(client, &building_with_list_attributes(&id)).await;

    let patched = client
        .partial_update_attr(
            &id,
            "hasRooms",
            &json!({"type": "ListRelationship", "objectList": [NULL_URI], "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("partial update failed");
    assert_eq!(
        patched.status,
        StatusCode::NO_CONTENT,
        "clause 5.5.8: deleting a ListRelationship with the NGSI-LD Null answers 204: {:?}",
        patched.body
    );

    let retrieved = client
        .get_entity(&id, "", "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let body = retrieved.json_value().expect("the entity is JSON");
    assert!(
        body.get("hasRooms").is_none(),
        "clause 4.5.22.2: an objectList holding one NGSI-LD Null deletes the ListRelationship: {body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 4.5.0 and 4.5.21.2: a deleted `ListProperty` is encoded in
/// notifications by an array holding one single NGSI-LD Null under its own
/// `valueList`. The `value` clause 4.5.21.2 prohibits must not appear, and the
/// list it replaces must not survive beside it.
#[tokio::test]
async fn regression_a_deleted_list_property_notification_carries_no_prohibited_value_member() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");
    let id = scope.id("Building:list-property-deleted-notification");

    create_entity(scope.client(), &building_with_list_attributes(&id)).await;
    create_subscription(&scope, &receiver, "Subscription:list-property-deleted", &["attributeDeleted"], true).await;

    let deleted = scope
        .client()
        .delete_entity_attr(&id, "tags", None, DatasetSelection::Default, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("delete attribute failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete attribute: {:?}", deleted.body);

    let body = notification_carrying(&receiver, "tags", "valueList").await;
    let tags = &body["data"][0]["tags"];
    assert_eq!(
        tags["valueList"],
        json!([NULL_URI]),
        "clause 4.5.0: a deleted ListProperty carries an array with one single NGSI-LD Null: {tags}"
    );
    assert!(tags.get("value").is_none(), "clause 4.5.21.2 prohibits value on a ListProperty: {tags}");
    assert!(tags.get("previousValue").is_none(), "clause 4.5.21.2 prohibits previousValue: {tags}");

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clauses 4.5.7, 4.5.8 and 4.5.21.2: the temporal evolution encodes a deleted
/// `ListProperty` instance the same way — the single-null array in `valueList`
/// beside its `deletedAt` — and never the prohibited `value`.
#[tokio::test]
async fn regression_a_deleted_list_property_temporal_evolution_carries_no_prohibited_value_member() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:list-property-deleted-temporal");

    create_entity(client, &building_with_list_attributes(&id)).await;
    let deleted = client
        .delete_entity_attr(&id, "tags", None, DatasetSelection::Default, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("delete attribute failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete attribute: {:?}", deleted.body);

    let evolution = client
        .get_temporal_entity(
            &id,
            "timeproperty=deletedAt&timerel=after&timeAt=1970-01-01T00:00:00Z",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("retrieve temporal entity failed");
    assert_eq!(evolution.status, StatusCode::OK, "temporal retrieve: {:?}", evolution.body);
    let body = evolution.json_value().expect("the temporal entity is JSON");
    let instances = body["tags"].as_array().expect("the temporal evolution carries tags instances");
    let tombstone = instances
        .iter()
        .find(|instance| instance.get("deletedAt").is_some())
        .expect("clause 4.5.8: the deletion is an instance carrying deletedAt");

    assert_eq!(
        tombstone["valueList"],
        json!([NULL_URI]),
        "clause 4.5.0: a deleted ListProperty renders an array with one single NGSI-LD Null: {tombstone}"
    );
    assert!(
        tombstone.get("value").is_none(),
        "clause 4.5.21.2 prohibits value on a ListProperty: {tombstone}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.9: "For each `ListProperty` ... Such JSON-LD object shall only
/// contain a member whose key shall be `valueLists` ... Each array element
/// shall be another array containing exactly two elements: the first element
/// shall be an ordered array of Property values, and the second element shall
/// correspond to the associated Temporal Property".
#[tokio::test]
async fn regression_temporal_values_render_a_list_property_as_value_lists() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:list-property-temporal-values");

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": "Building",
            "period": [
                {"type": "ListProperty", "valueList": ["First", "Second"], "observedAt": "2022-08-09T18:25:02Z"},
                {"type": "ListProperty", "valueList": ["1st", "2nd"], "observedAt": "2022-08-10T18:25:02Z"}
            ],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "temporal create: {:?}", created.body);

    let retrieved = client
        .get_temporal_entity(
            &id,
            "options=temporalValues&timerel=after&timeAt=2020-01-01T00:00:00Z",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("retrieve temporal entity failed");
    assert_eq!(retrieved.status, StatusCode::OK, "temporal retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("the temporal entity is JSON");
    let period = &body["period"];

    assert_eq!(period["type"], "ListProperty", "{period}");
    assert_eq!(
        period["valueLists"],
        json!([[["First", "Second"], "2022-08-09T18:25:02Z"], [["1st", "2nd"], "2022-08-10T18:25:02Z"]]),
        "clause 4.5.9: a ListProperty collapses into valueLists of [ordered array, Temporal Property] pairs: {period}"
    );
    assert!(
        period.get("values").is_none(),
        "clause 4.5.9 gives a ListProperty valueLists alone, not the values it specializes: {period}"
    );
    assert!(period.get("value").is_none(), "clause 4.5.21.2 prohibits value: {period}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.9: "For each `ListRelationship` ... Such JSON-LD object shall only
/// contain a member whose key shall be `objectLists` ... the first element
/// shall be an ordered array of Relationship objects (URIs) and the second
/// element shall correspond to the associated Temporal Property".
#[tokio::test]
async fn regression_temporal_values_render_a_list_relationship_as_object_lists() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:list-relationship-temporal-values");

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": "Building",
            "membersPresent": [
                {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"], "observedAt": "2022-08-09T18:25:02Z"},
                {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:Alice"], "observedAt": "2022-08-10T18:25:02Z"}
            ],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "temporal create: {:?}", created.body);

    let retrieved = client
        .get_temporal_entity(
            &id,
            "options=temporalValues&timerel=after&timeAt=2020-01-01T00:00:00Z",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("retrieve temporal entity failed");
    assert_eq!(retrieved.status, StatusCode::OK, "temporal retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("the temporal entity is JSON");
    let members = &body["membersPresent"];

    assert_eq!(members["type"], "ListRelationship", "{members}");
    assert_eq!(
        members["objectLists"],
        json!([
            [["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"], "2022-08-09T18:25:02Z"],
            [["urn:ngsi-ld:Person:Alice"], "2022-08-10T18:25:02Z"]
        ]),
        "clause 4.5.9: a ListRelationship collapses into objectLists of [ordered URIs, Temporal Property] pairs: {members}"
    );
    assert!(
        members.get("objects").is_none(),
        "clause 4.5.9 gives a ListRelationship objectLists alone, not the objects it specializes: {members}"
    );
    assert!(members.get("object").is_none(), "clause 4.5.22.2 prohibits object: {members}");

    scope.finish().await.expect("cleanup failed");
}
