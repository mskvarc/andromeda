//! The two representations clause 4.5.22 gives a `ListRelationship`'s
//! `objectList`, end to end.
//!
//! Clause 4.5.22.2, Mandatory: "`objectList`: a JSON representation of an
//! ordered array of Relationship Objects each consisting of a JSON object
//! containing a single Attribute with a key called `object` and its value
//! holding the Relationship's object represented by a URI." Table 5.2.37-1
//! splits the two forms explicitly: "In the normalized form, each array element
//! holds a JSON object containing a single Attribute with a key called `object`
//! and where the value is a valid URI. In the concise form, each string in the
//! array holds a valid URI." Clause 4.5.4 gives the simplified rendering "an
//! ordered array holding the `ListRelationship`'s Objects (represented as
//! URIs)".
//!
//! The broker used to answer every representation with the concise array, so a
//! normalized retrieval, a normalized notification and a normalized temporal
//! evolution of the same list disagreed with each other and with the clause.
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

/// The two targets every list under test names, in order.
const TARGETS: [&str; 2] = ["urn:ngsi-ld:Room:1", "urn:ngsi-ld:Room:2"];

/// The target a partial update moves the list to.
const REPLACEMENT_TARGET: &str = "urn:ngsi-ld:Room:3";

/// How long one notification is waited for.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(15);

/// How many deliveries are read before giving up on the one under test: an
/// entity creation and the change being tested can both match a subscription,
/// and only the second carries what is asserted.
const DELIVERIES_READ: usize = 4;

/// The clause 4.5.22.2 normalized array for `targets`.
fn normalized(targets: &[&str]) -> Value {
    Value::Array(targets.iter().map(|target| json!({"object": target})).collect())
}

/// The clause 4.5.22.3 concise array for `targets`.
fn concise(targets: &[&str]) -> Value {
    Value::Array(targets.iter().map(|target| json!(target)).collect())
}

/// A `Building` whose `hasRooms` names [`TARGETS`] in `written`.
fn building_with_object_list(id: &str, written: &Value) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "hasRooms": {"type": "ListRelationship", "objectList": written},
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// Create one entity and assert it was stored.
async fn create_entity(client: &TestClient, entity: &Value) {
    let created = client.post_entity(entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// Retrieve one entity in the representation `query` names.
async fn retrieve(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity failed");
    assert_eq!(response.status, StatusCode::OK, "retrieve {query}: {:?}", response.body);
    response.json_value().expect("the entity is JSON")
}

/// Create a subscription over `Building` entities delivering `format` to
/// `receiver`, with the clause 5.8.6 `showChanges` option set.
async fn create_show_changes_subscription(scope: &TestEnvironment, receiver: &NotificationServer, local_id: &str, format: &str) {
    let subscription = json!({
        "id": scope.id(local_id),
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notificationTrigger": ["attributeUpdated"],
        "notification": {
            "showChanges": true,
            "format": format,
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

/// Move `hasRooms` to [`REPLACEMENT_TARGET`], which is the change every
/// `showChanges` test observes.
async fn replace_the_list(client: &TestClient, id: &str) {
    let updated = client
        .partial_update_attr(
            id,
            "hasRooms",
            &json!({"type": "ListRelationship", "objectList": [REPLACEMENT_TARGET], "@context": [TEST_SUITE_CONTEXT]}),
        )
        .await
        .expect("partial update failed");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "partial update: {:?}", updated.body);
}

/// Assert the members clause 4.5.22.2 and clause 4.5.22.3 both prohibit are
/// absent: "`object` and `previousObject`: shall never be present as object is
/// a generalization of `objectList`".
fn assert_prohibited_members_absent(attribute: &Value, scenario: &str) {
    assert!(attribute.get("object").is_none(), "{scenario}: clause 4.5.22.2 prohibits object: {attribute}");
    assert!(
        attribute.get("previousObject").is_none(),
        "{scenario}: clause 4.5.22.2 prohibits previousObject: {attribute}"
    );
}

/// Table 5.2.37-1: "In the normalized form, each array element holds a JSON
/// object containing a single Attribute with a key called `object` and where
/// the value is a valid URI. In the concise form, each string in the array
/// holds a valid URI." Clause 4.5.4 gives the simplified rendering "an ordered
/// array holding the `ListRelationship`'s Objects (represented as URIs)". The
/// three renderings are therefore not interchangeable, and the normalized one
/// is the one the broker answered with the concise array.
#[tokio::test]
async fn regression_list_relationship_representation_each_format_renders_its_own_object_list_form() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:object-list-per-format");

    create_entity(client, &building_with_object_list(&id, &concise(&TARGETS))).await;

    let normalized_body = retrieve(client, &id, "").await;
    let rooms = &normalized_body["hasRooms"];
    assert_eq!(rooms["type"], json!("ListRelationship"), "clause 4.5.22.2 makes type Mandatory: {rooms}");
    assert_eq!(
        rooms["objectList"],
        normalized(&TARGETS),
        "clause 4.5.22.2: each normalized element is an object keyed `object`: {rooms}"
    );
    assert_prohibited_members_absent(rooms, "normalized");

    let concise_body = retrieve(client, &id, "options=concise").await;
    let rooms = &concise_body["hasRooms"];
    assert_eq!(
        rooms["objectList"],
        concise(&TARGETS),
        "Table 5.2.37-1: in the concise form each string in the array holds a valid URI: {rooms}"
    );
    assert_prohibited_members_absent(rooms, "concise");

    let key_values_body = retrieve(client, &id, "options=keyValues").await;
    assert_eq!(
        key_values_body["hasRooms"],
        concise(&TARGETS),
        "clause 4.5.4: the simplified rendering is an ordered array of the Objects as URIs: {key_values_body}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.22.3 lets a Context Producer write the concise array and clause
/// 4.5.22.2 the normalized one, and Table 5.2.37-1 makes them the same ordered
/// array of targets. Which spelling was written therefore cannot decide which
/// spelling is read back: both round-trip to the representation the request
/// asked for.
#[tokio::test]
async fn regression_list_relationship_representation_both_written_forms_round_trip_to_the_same_renderings() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let written_concise = scope.id("Building:object-list-written-concise");
    let written_normalized = scope.id("Building:object-list-written-normalized");

    create_entity(client, &building_with_object_list(&written_concise, &concise(&TARGETS))).await;
    create_entity(client, &building_with_object_list(&written_normalized, &normalized(&TARGETS))).await;

    for (query, expected) in [("", normalized(&TARGETS)), ("options=concise", concise(&TARGETS))] {
        let from_concise = retrieve(client, &written_concise, query).await;
        let from_normalized = retrieve(client, &written_normalized, query).await;
        assert_eq!(
            from_concise["hasRooms"]["objectList"], expected,
            "Table 5.2.37-1: the concise write renders the requested form under '{query}': {from_concise}"
        );
        assert_eq!(
            from_normalized["hasRooms"]["objectList"], expected,
            "Table 5.2.37-1: the normalized write renders the requested form under '{query}': {from_normalized}"
        );
    }

    let from_concise = retrieve(client, &written_concise, "options=keyValues").await;
    let from_normalized = retrieve(client, &written_normalized, "options=keyValues").await;
    assert_eq!(
        from_concise["hasRooms"], from_normalized["hasRooms"],
        "clause 4.5.4 renders both writes identically: {from_concise} vs {from_normalized}"
    );
    assert_eq!(from_normalized["hasRooms"], concise(&TARGETS), "clause 4.5.4: bare URIs: {from_normalized}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.22.2, Output Only: "`previousObjectList`: only provided in the
/// case of notifications and only if the `showChanges` option is explicitly
/// requested. It represents the previous `ListRelationship` `objectList`,
/// before the triggering change. The representation is the same as that of
/// `objectList`" — so in a normalized notification both members hold the
/// `object`-keyed Relationship Objects of the same clause.
#[tokio::test]
async fn regression_list_relationship_representation_a_normalized_notification_carries_object_keyed_members() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");
    let id = scope.id("Building:object-list-normalized-notification");

    create_entity(scope.client(), &building_with_object_list(&id, &concise(&TARGETS))).await;
    create_show_changes_subscription(&scope, &receiver, "Subscription:object-list-normalized", "normalized").await;
    replace_the_list(scope.client(), &id).await;

    let body = notification_carrying(&receiver, "hasRooms", "previousObjectList").await;
    let rooms = &body["data"][0]["hasRooms"];
    assert_eq!(
        rooms["objectList"],
        normalized(&[REPLACEMENT_TARGET]),
        "clause 4.5.22.2: a normalized notification's objectList holds Relationship Objects: {rooms}"
    );
    assert_eq!(
        rooms["previousObjectList"],
        normalized(&TARGETS),
        "clause 4.5.22.2: previousObjectList has the same representation as objectList: {rooms}"
    );
    assert_prohibited_members_absent(rooms, "normalized notification");

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.37-1, concise form: "each string in the array holds a valid URI".
/// Clause 4.5.22.3 gives `previousObjectList` the same representation as
/// `objectList`, so a concise notification carries both as bare URI arrays.
#[tokio::test]
async fn regression_list_relationship_representation_a_concise_notification_carries_bare_uris() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let receiver = NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification server must start");
    let id = scope.id("Building:object-list-concise-notification");

    create_entity(scope.client(), &building_with_object_list(&id, &concise(&TARGETS))).await;
    create_show_changes_subscription(&scope, &receiver, "Subscription:object-list-concise", "concise").await;
    replace_the_list(scope.client(), &id).await;

    let body = notification_carrying(&receiver, "hasRooms", "previousObjectList").await;
    let rooms = &body["data"][0]["hasRooms"];
    assert_eq!(
        rooms["objectList"],
        concise(&[REPLACEMENT_TARGET]),
        "Table 5.2.37-1: a concise notification's objectList holds bare URIs: {rooms}"
    );
    assert_eq!(
        rooms["previousObjectList"],
        concise(&TARGETS),
        "clause 4.5.22.3: previousObjectList has the same representation as objectList: {rooms}"
    );
    assert_prohibited_members_absent(rooms, "concise notification");

    receiver.shutdown().await.expect("notification server shutdown failed");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.22.2 governs the temporal evolution as well: a temporal
/// retrieval answers in the normalized representation, so each `objectList`
/// element is "a JSON object containing a single Attribute with a key called
/// `object`". The current-state fix must not disturb the path that already
/// answered this way.
#[tokio::test]
async fn regression_list_relationship_representation_a_temporal_evolution_keeps_object_keyed_members() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:object-list-temporal");

    let created = client
        .create_temporal_entity(&json!({
            "id": &id,
            "type": "Building",
            "hasRooms": [
                {"type": "ListRelationship", "objectList": concise(&TARGETS), "observedAt": "2022-08-09T18:25:02Z"}
            ],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create temporal entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "temporal create: {:?}", created.body);

    let retrieved = client
        .get_temporal_entity(&id, "timerel=after&timeAt=2020-01-01T00:00:00Z", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve temporal entity failed");
    assert_eq!(retrieved.status, StatusCode::OK, "temporal retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("the temporal entity is JSON");
    let instances = body["hasRooms"].as_array().expect("the temporal evolution carries hasRooms instances");
    let observation = instances
        .iter()
        .find(|instance| instance.get("observedAt").is_some())
        .expect("the recorded observation");
    assert_eq!(
        observation["objectList"],
        normalized(&TARGETS),
        "clause 4.5.22.2: a temporal evolution answers in the normalized form: {observation}"
    );
    assert_prohibited_members_absent(observation, "temporal evolution");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.22.2, the sentence that follows the `objectList` definition: "An
/// array consisting of a single NGSI-LD Null … can be used as the right-hand
/// side of the `objectList` … as well as in notifications and in temporal
/// evolutions (for encoding a deleted `ListRelationship`)."
///
/// The deletion encoding is the one `objectList` array whose element is not a
/// Relationship Object, so it must stay a bare `urn:ngsi-ld:null` rather than
/// being wrapped as `{"object": …}` by the normalized rendering this module
/// otherwise pins. The wrapping and the guard now live in one function, and
/// this is the end-to-end proof of the guard: without it the tombstone reads
/// back as `[{"object": "urn:ngsi-ld:null"}]`, which no clause admits.
///
/// The tombstone carries no `observedAt`, so it is reachable only through
/// `timeproperty=deletedAt` (clause 4.8) -- querying the default time property
/// selects the observation and never the deletion.
#[tokio::test]
async fn regression_list_relationship_representation_a_deleted_list_is_encoded_as_a_bare_null() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:object-list-deleted");

    create_entity(client, &building_with_object_list(&id, &concise(&TARGETS))).await;

    let deleted = client
        .delete_entity_attr(&id, "hasRooms", None, DatasetSelection::All, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("delete the list");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete: {:?}", deleted.body);

    let retrieved = client
        .get_temporal_entity(
            &id,
            "timerel=after&timeAt=2020-01-01T00:00:00Z&timeproperty=deletedAt",
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("retrieve the temporal evolution");
    assert_eq!(retrieved.status, StatusCode::OK, "temporal retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("the temporal entity is JSON");
    let instances = body["hasRooms"].as_array().expect("the temporal evolution carries hasRooms instances");
    let tombstone = instances
        .iter()
        .find(|instance| instance.get("deletedAt").is_some())
        .expect("the recorded deletion");

    assert_eq!(
        tombstone["objectList"],
        json!(["urn:ngsi-ld:null"]),
        "clause 4.5.22.2: a deleted ListRelationship is a bare NGSI-LD Null, not a Relationship Object: {tombstone}"
    );
    assert_prohibited_members_absent(tombstone, "deletion tombstone");

    scope.finish().await.expect("cleanup failed");
}
