//! The `ngsildConformance` member a Subscription declares, which the broker
//! accepted and then ignored entirely.
//!
//! ETSI GS CIM 009 v1.9.1 Table 5.2.12-1 gives a Subscription an optional
//! `ngsildConformance`, "a semantically versioned string in the form
//! major.minor, which conforms to a version of the NGSI-LD specification", and
//! says that "if provided the notification shall undergo a backwards
//! compatibility operation as defined by clause 4.3.6.8 and be amended to
//! conform to the supplied version". Clause 5.8.6 repeats it as the last step
//! before "A Notification shall be sent", after the `notification.format`
//! rendering and after `pick`, `omit` and `join`.
//!
//! Before this suite existed the member did not survive a create: it was not a
//! column, not a field of the resource, and not read by any notification path,
//! so a client that asked for a 1.0-shaped payload was answered `201 Created`
//! and then sent a 1.9-shaped one. A malformed value was accepted just as
//! silently.
//!
//! Every test that asserts an amendment carries a control subscription that
//! declares no `ngsildConformance` and is otherwise identical, so an assertion
//! can only pass when the two notifications genuinely differ.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use andromeda_test_mocks::{notification_server::NotificationServer, server_config::MockServerConfig};
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Duration};
use tokio::time::sleep;
use urlencoding::encode;

/// The problem type clause 5.5.4 gives data that is not syntactically correct
/// according to the API data type definitions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// How long the compiled-subscription cache is given to observe a create.
const CACHE_SETTLE: Duration = Duration::from_millis(600);

/// How long a notification that will arrive is waited for.
const NOTIFICATION_WAIT: Duration = Duration::from_secs(15);

/// An `expiresAt` far enough ahead that the expiry sweep never reaches it.
const FAR_FUTURE: &str = "2099-01-01T00:00:00Z";

/// Bind an isolated notification receiver that accepts every delivery.
async fn notification_receiver() -> NotificationServer {
    NotificationServer::start(MockServerConfig::from_environment().expect("valid callback configuration"))
        .await
        .expect("notification receiver must start")
}

/// Assert a refusal is the `BadRequestData` of clauses 5.5.3 and 5.5.4.
fn assert_bad_request_data(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
}

/// Create a subscription, assert it was accepted, and let the matching cache
/// observe it.
async fn create_subscription(client: &TestClient, body: &Value) {
    let response = client.create_subscription(body).await.expect("create subscription request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
}

/// Retrieve one subscription and decode it.
async fn retrieve_subscription(client: &TestClient, id: &str) -> Value {
    let response = client.retrieve_subscription(id, None).await.expect("retrieve subscription request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response.json_value().expect("a retrieved subscription is JSON")
}

/// Delete every subscription a test created.
///
/// Called before the assertions rather than after them, so a subscription is
/// gone from the shared broker whether the test passes or fails.
async fn delete_subscriptions(client: &TestClient, ids: &[&str]) {
    for id in ids {
        let response = client.delete_subscription(id).await.expect("delete subscription request");
        assert!(
            response.status == StatusCode::NO_CONTENT || response.status == StatusCode::NOT_FOUND,
            "deleting {id} must succeed: {} {:?}",
            response.status,
            response.body
        );
    }
}

/// A subscription selecting exactly one entity, notified to `endpoint`.
fn subscription_for(id: &str, entity_id: &str, entity_type: &str, endpoint: &str, context: &Value) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"id": entity_id, "type": entity_type}],
        "notification": {"endpoint": {"uri": endpoint}},
        "@context": context
    })
}

/// The entity every amendment test watches: one carrier for each row of Tables
/// 4.3.6.8-1, -2 and -3.
fn conformance_entity(entity_id: &str, room_id: &str, building_id: &str, context: &Value) -> Value {
    json!({
        "id": entity_id,
        // Table 4.3.6.8-1 note 1: multiple entity types exist from 1.3 onwards.
        "type": ["Vehicle", "Car"],
        // Table 4.3.6.8-1: `scope` was introduced in 1.4.
        "scope": ["/conformance"],
        // Table 4.3.6.8-1: entity `expiresAt` was introduced in 1.9.
        "expiresAt": FAR_FUTURE,
        // Table 4.3.6.8-2: `observedAt`, `unitCode` (1.3) and `valueType` (1.9).
        "speed": {"type": "Property", "value": 20, "observedAt": "2025-01-01T00:00:00Z", "unitCode": "CEL", "valueType": "Number"},
        // Table 4.3.6.8-1: the LanguageProperty was introduced in 1.4.
        "name": {"type": "LanguageProperty", "languageMap": {"en": "Tower", "de": "Turm"}},
        // Table 4.3.6.8-1: VocabProperty, JsonProperty, ListProperty and
        // ListRelationship were all introduced in 1.8.
        "category": {"type": "VocabProperty", "vocab": "commercial"},
        "config": {"type": "JsonProperty", "json": {"level": 3}},
        "tags": {"type": "ListProperty", "valueList": ["a", "b"]},
        "refRooms": {"type": "ListRelationship", "objectList": [room_id, format!("{room_id}-second")]},
        // Table 4.3.6.8-3: `objectType` (1.8) and `observedAt` (1.3).
        "refBuilding": {"type": "Relationship", "object": building_id, "objectType": "Building", "observedAt": "2025-01-01T00:00:00Z"},
        // Table 4.3.6.8-1 note 2: several instances of one attribute, told
        // apart by the `datasetId` introduced in 1.3.
        "temperature": [
            {"type": "Property", "value": 25, "datasetId": "urn:ngsi-ld:Dataset:one"},
            {"type": "Property", "value": 30}
        ],
        "@context": context
    })
}

/// Create the watched entity and assert it was accepted.
async fn create_entity(client: &TestClient, entity: &Value) {
    let response = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "{:?}", response.body);
}

/// Append one attribute so the entity changes and its subscriptions are
/// evaluated (clause 5.8.6, `attributeCreated`).
async fn touch_entity(client: &TestClient, entity_id: &str, marker: &str, context: &Value) {
    let fragment = json!({"note": {"type": "Property", "value": marker}, "@context": context});
    let response = client.append_entity_attrs(entity_id, &fragment).await.expect("append attribute request");
    assert!(
        response.status == StatusCode::NO_CONTENT || response.status == StatusCode::CREATED,
        "appending an attribute must succeed: {} {:?}",
        response.status,
        response.body
    );
}

/// Collect one notified entity per subscription named in `expected`.
///
/// Notifications from several subscriptions interleave, so they are read until
/// every expected subscription has been seen or the deadline passes; the map
/// the caller receives says which arrived.
async fn notified_entities_by_subscription(receiver: &NotificationServer, expected: usize) -> BTreeMap<String, Value> {
    let mut collected: BTreeMap<String, Value> = BTreeMap::new();
    while collected.len() < expected {
        let Ok(captured) = receiver.next_request(NOTIFICATION_WAIT).await else {
            break;
        };
        let Ok(body) = captured.json::<Value>() else {
            continue;
        };
        let Some(subscription_id) = body["subscriptionId"].as_str().map(str::to_owned) else {
            continue;
        };
        let Some(entity) = body["data"].as_array().and_then(|data| data.first()) else {
            continue;
        };
        // clone: the assertion outlives the captured request body
        collected.entry(subscription_id).or_insert_with(|| entity.clone());
    }
    collected
}

/// Table 5.2.12-1 defines `ngsildConformance` as a member of the Subscription
/// datatype, so it survives a create, is echoed by a retrieve, and is replaced
/// by an update that names it. Clause 5.8.3 updates a Subscription with "a
/// fragment of the Subscription", so a fragment that does not mention the
/// member leaves the stored version alone.
#[tokio::test]
async fn regression_ngsild_conformance_round_trips_through_create_retrieve_and_update() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);

    let subscription_id = scope.id("Subscription:conformance-round-trip");
    let mut body = subscription_for(
        &subscription_id,
        &scope.id("Vehicle:conformance-round-trip"),
        "Vehicle",
        "http://127.0.0.1:9/notify",
        &context,
    );
    body["ngsildConformance"] = json!("1.4");
    create_subscription(client, &body).await;

    let created = retrieve_subscription(client, &subscription_id).await;

    let updated = client
        .update_subscription(&subscription_id, &json!({"ngsildConformance": "1.0"}), Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("update subscription request");
    let update_status = updated.status;
    let update_body = updated.body.clone(); // clone: the diagnostic outlives the response
    let replaced = retrieve_subscription(client, &subscription_id).await;

    let untouched_update = client
        .update_subscription(&subscription_id, &json!({"description": "unrelated"}), Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("update subscription request");
    let untouched_status = untouched_update.status;
    let untouched = retrieve_subscription(client, &subscription_id).await;

    delete_subscriptions(client, &[&subscription_id]).await;

    assert_eq!(created["ngsildConformance"], json!("1.4"), "{created}");
    assert_eq!(update_status, StatusCode::NO_CONTENT, "{update_body:?}");
    assert_eq!(replaced["ngsildConformance"], json!("1.0"), "{replaced}");
    assert_eq!(untouched_status, StatusCode::NO_CONTENT);
    assert_eq!(
        untouched["ngsildConformance"],
        json!("1.0"),
        "a fragment that names no ngsildConformance leaves the stored one alone: {untouched}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.12-1 restricts `ngsildConformance` to "a semantically versioned
/// string in the form major.minor". Anything else is data that is not
/// syntactically correct according to the API data type definition, which
/// clause 5.5.4 answers with `BadRequestData` -- not with a `201 Created` and a
/// silently dropped member.
#[tokio::test]
async fn regression_ngsild_conformance_a_malformed_version_is_bad_request_data() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);

    for (value, scenario) in [
        (json!("1"), "a bare major component is not major.minor"),
        (json!("1.2.3"), "a patch component is not major.minor"),
        (json!("abc"), "a non-numeric token is not major.minor"),
        (json!(""), "the empty string is not major.minor"),
        (json!("v1.4"), "a version prefix is not major.minor"),
        (json!(1.4), "Table 5.2.12-1 types the member as a String"),
        (json!(null), "Table 5.2.12-1 types the member as a String"),
    ] {
        let mut body = subscription_for(
            &scope.id("Subscription:conformance-refused"),
            &scope.id("Vehicle:conformance-refused"),
            "Vehicle",
            "http://127.0.0.1:9/notify",
            &context,
        );
        body["ngsildConformance"] = value;

        let response = client.create_subscription(&body).await.expect("create subscription request");

        assert_bad_request_data(&response, scenario);
    }

    // The same restriction applies to the fragment of clause 5.8.3, which is
    // the same datatype.
    let subscription_id = scope.id("Subscription:conformance-refused-update");
    let body = subscription_for(
        &subscription_id,
        &scope.id("Vehicle:conformance-refused-update"),
        "Vehicle",
        "http://127.0.0.1:9/notify",
        &context,
    );
    create_subscription(client, &body).await;
    let refused = client
        .update_subscription(&subscription_id, &json!({"ngsildConformance": "1.2.3"}), Some(TEST_SUITE_CONTEXT.as_str()))
        .await
        .expect("update subscription request");

    delete_subscriptions(client, &[&subscription_id]).await;

    assert_bad_request_data(&refused, "an update fragment carries the same restriction");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6 and Tables 4.3.6.8-1 to -3 for a 1.0 target: every member and
/// datatype introduced after 1.0 is removed or reformatted. The control
/// subscription differs only in declaring no `ngsildConformance`, so each
/// assertion below names a difference the amendment made.
#[tokio::test]
async fn regression_ngsild_conformance_a_1_0_target_receives_the_1_0_shape() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = notification_receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let context = json!([TEST_SUITE_CONTEXT]);

    let entity_id = scope.id("Vehicle:conformance-1-0");
    create_entity(
        client,
        &conformance_entity(&entity_id, &scope.id("Room:conformance-1-0"), &scope.id("Building:conformance-1-0"), &context),
    )
    .await;

    let amended_id = scope.id("Subscription:conformance-1-0-amended");
    let mut amended_body = subscription_for(&amended_id, &entity_id, "Vehicle", &endpoint, &context);
    amended_body["ngsildConformance"] = json!("1.0");
    create_subscription(client, &amended_body).await;

    let control_id = scope.id("Subscription:conformance-1-0-control");
    let control_body = subscription_for(&control_id, &entity_id, "Vehicle", &endpoint, &context);
    create_subscription(client, &control_body).await;
    sleep(CACHE_SETTLE).await;

    receiver.drain_requests().await;
    touch_entity(client, &entity_id, "conformance-1-0", &context).await;
    let notified = notified_entities_by_subscription(&receiver, 2).await;

    delete_subscriptions(client, &[&amended_id, &control_id]).await;
    receiver.shutdown().await.expect("notification receiver shutdown failed");

    let control = notified.get(&control_id).expect("the control subscription is notified");
    let amended = notified.get(&amended_id).expect("the amended subscription is notified");

    // Table 4.3.6.8-1 note 1: a multi-valued `type` exists from 1.3 onwards.
    assert_eq!(control["type"], json!(["Vehicle", "Car"]), "{control}");
    assert_eq!(amended["type"], json!("Vehicle"), "{amended}");

    // Table 4.3.6.8-1: `scope` was introduced in 1.4 and entity `expiresAt` in 1.9.
    assert_eq!(control["scope"], json!("/conformance"), "{control}");
    assert!(amended.get("scope").is_none(), "{amended}");
    assert!(control["expiresAt"].is_string(), "{control}");
    assert!(amended.get("expiresAt").is_none(), "{amended}");

    // Table 4.3.6.8-2: `observedAt`, `unitCode` and `valueType`.
    assert!(control["speed"]["observedAt"].is_string(), "{control}");
    assert_eq!(control["speed"]["unitCode"], json!("CEL"), "{control}");
    assert_eq!(amended["speed"], json!({"type": "Property", "value": 20}), "{amended}");

    // Table 4.3.6.8-1: the LanguageProperty is reformatted as a Property.
    assert_eq!(control["name"]["type"], json!("LanguageProperty"), "{control}");
    assert_eq!(
        amended["name"],
        json!({"type": "Property", "value": {"en": "Tower", "de": "Turm"}}),
        "{amended}"
    );

    // Table 4.3.6.8-1: the three 1.8 Property subclasses are reformatted as Properties.
    assert_eq!(amended["category"], json!({"type": "Property", "value": "commercial"}), "{amended}");
    assert_eq!(amended["config"], json!({"type": "Property", "value": {"level": 3}}), "{amended}");
    assert_eq!(amended["tags"], json!({"type": "Property", "value": ["a", "b"]}), "{amended}");

    // Table 4.3.6.8-1: a ListRelationship is reformatted as a Relationship.
    assert_eq!(control["refRooms"]["type"], json!("ListRelationship"), "{control}");
    assert_eq!(amended["refRooms"]["type"], json!("Relationship"), "{amended}");
    assert_eq!(amended["refRooms"]["object"], json!(scope.id("Room:conformance-1-0")), "{amended}");
    assert!(amended["refRooms"].get("objectList").is_none(), "{amended}");

    // Table 4.3.6.8-3: `objectType` and `observedAt`.
    assert_eq!(control["refBuilding"]["objectType"], json!("Building"), "{control}");
    assert_eq!(
        amended["refBuilding"],
        json!({"type": "Relationship", "object": scope.id("Building:conformance-1-0")}),
        "{amended}"
    );

    // Table 4.3.6.8-1 note 2: several instances become the default instance alone.
    assert!(control["temperature"].is_array(), "{control}");
    assert_eq!(amended["temperature"], json!({"type": "Property", "value": 30}), "{amended}");

    scope.finish().await.expect("cleanup failed");
}

/// Every fallback of Tables 4.3.6.8-1 to -3 names the version its member or
/// datatype was introduced at, and applies below that version and not at or
/// above it. Three targets in one entity's lifetime therefore receive three
/// different payloads.
#[tokio::test]
async fn regression_ngsild_conformance_each_target_applies_only_the_rows_below_it() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = notification_receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let context = json!([TEST_SUITE_CONTEXT]);

    let entity_id = scope.id("Vehicle:conformance-thresholds");
    create_entity(
        client,
        &conformance_entity(
            &entity_id,
            &scope.id("Room:conformance-thresholds"),
            &scope.id("Building:conformance-thresholds"),
            &context,
        ),
    )
    .await;

    let ids: Vec<String> = ["1.3", "1.4", "1.8"]
        .iter()
        .map(|version| scope.id(&format!("Subscription:conformance-{}", version.replace('.', "-"))))
        .collect();
    for (id, version) in ids.iter().zip(["1.3", "1.4", "1.8"]) {
        let mut body = subscription_for(id, &entity_id, "Vehicle", &endpoint, &context);
        body["ngsildConformance"] = json!(version);
        create_subscription(client, &body).await;
    }
    sleep(CACHE_SETTLE).await;

    receiver.drain_requests().await;
    touch_entity(client, &entity_id, "conformance-thresholds", &context).await;
    let notified = notified_entities_by_subscription(&receiver, ids.len()).await;

    let borrowed: Vec<&str> = ids.iter().map(String::as_str).collect();
    delete_subscriptions(client, &borrowed).await;
    receiver.shutdown().await.expect("notification receiver shutdown failed");

    let at_1_3 = notified.get(&ids[0]).expect("the 1.3 subscription is notified");
    let at_1_4 = notified.get(&ids[1]).expect("the 1.4 subscription is notified");
    let at_1_8 = notified.get(&ids[2]).expect("the 1.8 subscription is notified");

    // 1.3 knows the multi-valued `type` and the `datasetId` that separates
    // instances, and knows neither `scope` (1.4) nor the LanguageProperty (1.4).
    assert_eq!(at_1_3["type"], json!(["Vehicle", "Car"]), "{at_1_3}");
    assert!(at_1_3["temperature"].is_array(), "{at_1_3}");
    assert_eq!(at_1_3["speed"]["observedAt"], json!("2025-01-01T00:00:00Z"), "{at_1_3}");
    assert_eq!(at_1_3["speed"]["unitCode"], json!("CEL"), "{at_1_3}");
    assert!(at_1_3.get("scope").is_none(), "{at_1_3}");
    assert_eq!(at_1_3["name"]["type"], json!("Property"), "{at_1_3}");

    // 1.4 knows `scope` and the LanguageProperty, and knows none of the four
    // datatypes introduced in 1.8 nor the `objectType` of the same version.
    assert_eq!(at_1_4["scope"], json!("/conformance"), "{at_1_4}");
    assert_eq!(at_1_4["name"]["type"], json!("LanguageProperty"), "{at_1_4}");
    assert_eq!(at_1_4["category"]["type"], json!("Property"), "{at_1_4}");
    assert_eq!(at_1_4["config"]["type"], json!("Property"), "{at_1_4}");
    assert_eq!(at_1_4["tags"]["type"], json!("Property"), "{at_1_4}");
    assert_eq!(at_1_4["refRooms"]["type"], json!("Relationship"), "{at_1_4}");
    assert!(at_1_4["refBuilding"].get("objectType").is_none(), "{at_1_4}");

    // 1.8 knows every datatype and loses only what 1.9 introduced: the entity
    // `expiresAt` and the Property `valueType`.
    assert_eq!(at_1_8["category"]["type"], json!("VocabProperty"), "{at_1_8}");
    assert_eq!(at_1_8["config"]["type"], json!("JsonProperty"), "{at_1_8}");
    assert_eq!(at_1_8["tags"]["type"], json!("ListProperty"), "{at_1_8}");
    assert_eq!(at_1_8["refRooms"]["type"], json!("ListRelationship"), "{at_1_8}");
    assert_eq!(at_1_8["refBuilding"]["objectType"], json!("Building"), "{at_1_8}");
    assert!(at_1_8.get("expiresAt").is_none(), "{at_1_8}");
    assert!(at_1_8["speed"].get("valueType").is_none(), "{at_1_8}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6 orders the amendment after the `notification.format`
/// rendering, so a subscription asking for `concise` is amended in the concise
/// representation. Clause 4.5.2.3 makes an attribute's `type` optional there --
/// "if missing, `Property` can be inferred by the presence of the `value`
/// attribute" -- which is what the amendment has to read the family from, and
/// the same clause is what puts a Property with nothing but a value back into
/// its bare form.
#[tokio::test]
async fn regression_ngsild_conformance_a_concise_notification_is_amended_in_its_own_representation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = notification_receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let context = json!([TEST_SUITE_CONTEXT]);

    let entity_id = scope.id("Vehicle:conformance-concise");
    create_entity(
        client,
        &conformance_entity(
            &entity_id,
            &scope.id("Room:conformance-concise"),
            &scope.id("Building:conformance-concise"),
            &context,
        ),
    )
    .await;

    let amended_id = scope.id("Subscription:conformance-concise-amended");
    let mut amended_body = subscription_for(&amended_id, &entity_id, "Vehicle", &endpoint, &context);
    amended_body["notification"]["format"] = json!("concise");
    amended_body["ngsildConformance"] = json!("1.0");
    create_subscription(client, &amended_body).await;

    let control_id = scope.id("Subscription:conformance-concise-control");
    let mut control_body = subscription_for(&control_id, &entity_id, "Vehicle", &endpoint, &context);
    control_body["notification"]["format"] = json!("concise");
    create_subscription(client, &control_body).await;
    sleep(CACHE_SETTLE).await;

    receiver.drain_requests().await;
    touch_entity(client, &entity_id, "conformance-concise", &context).await;
    let notified = notified_entities_by_subscription(&receiver, 2).await;

    delete_subscriptions(client, &[&amended_id, &control_id]).await;
    receiver.shutdown().await.expect("notification receiver shutdown failed");

    let control = notified.get(&control_id).expect("the control subscription is notified");
    let amended = notified.get(&amended_id).expect("the amended subscription is notified");

    // The control shows the concise representation the amendment has to read:
    // no `type` member anywhere, and the metadata Table 4.3.6.8-2 removes.
    assert_eq!(control["speed"]["unitCode"], json!("CEL"), "{control}");
    assert!(control["speed"].get("type").is_none(), "{control}");
    assert_eq!(control["name"], json!({"languageMap": {"en": "Tower", "de": "Turm"}}), "{control}");

    assert_eq!(amended["type"], json!("Vehicle"), "{amended}");
    assert!(amended.get("scope").is_none(), "{amended}");
    assert!(amended.get("expiresAt").is_none(), "{amended}");
    // Clause 4.5.2.3: what is left of the Property is its bare value.
    assert_eq!(amended["speed"], json!(20), "{amended}");
    // The LanguageProperty is reformatted as a Property, whose concise form is
    // the bare language map.
    assert_eq!(amended["name"], json!({"en": "Tower", "de": "Turm"}), "{amended}");
    assert_eq!(amended["category"], json!("commercial"), "{amended}");
    // Clause 4.5.5 would read a bare array as several instances, so the
    // reformatted ListProperty keeps the object form clause 4.5.2.3 gives a
    // Property that carries one.
    assert_eq!(amended["tags"], json!({"value": ["a", "b"]}), "{amended}");
    assert_eq!(amended["refBuilding"], json!({"object": scope.id("Building:conformance-concise")}), "{amended}");
    assert_eq!(amended["refRooms"], json!({"object": scope.id("Room:conformance-concise")}), "{amended}");
    assert_eq!(amended["temperature"], json!(30), "{amended}");

    scope.finish().await.expect("cleanup failed");
}

/// The same, for the clause 4.5.4 simplified representation a subscription
/// selects with `format: keyValues`. A simplified attribute is its bare value,
/// so the only fallbacks that can apply are the ones clause 4.5.4 leaves a
/// carrier for: the one-member wrappers of a `LanguageProperty`, a
/// `JsonProperty` and a `VocabProperty`, and the `dataset` map that holds the
/// several instances of Table 4.3.6.8-1 note 2.
#[tokio::test]
async fn regression_ngsild_conformance_a_key_values_notification_is_amended_in_its_own_representation() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = notification_receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let context = json!([TEST_SUITE_CONTEXT]);

    let entity_id = scope.id("Vehicle:conformance-key-values");
    create_entity(
        client,
        &conformance_entity(
            &entity_id,
            &scope.id("Room:conformance-key-values"),
            &scope.id("Building:conformance-key-values"),
            &context,
        ),
    )
    .await;

    let amended_id = scope.id("Subscription:conformance-key-values-amended");
    let mut amended_body = subscription_for(&amended_id, &entity_id, "Vehicle", &endpoint, &context);
    amended_body["notification"]["format"] = json!("keyValues");
    amended_body["ngsildConformance"] = json!("1.0");
    create_subscription(client, &amended_body).await;

    let control_id = scope.id("Subscription:conformance-key-values-control");
    let mut control_body = subscription_for(&control_id, &entity_id, "Vehicle", &endpoint, &context);
    control_body["notification"]["format"] = json!("keyValues");
    create_subscription(client, &control_body).await;
    sleep(CACHE_SETTLE).await;

    receiver.drain_requests().await;
    touch_entity(client, &entity_id, "conformance-key-values", &context).await;
    let notified = notified_entities_by_subscription(&receiver, 2).await;

    delete_subscriptions(client, &[&amended_id, &control_id]).await;
    receiver.shutdown().await.expect("notification receiver shutdown failed");

    let control = notified.get(&control_id).expect("the control subscription is notified");
    let amended = notified.get(&amended_id).expect("the amended subscription is notified");

    // The control shows the clause 4.5.4 shapes the amendment has to read.
    assert_eq!(control["name"], json!({"languageMap": {"en": "Tower", "de": "Turm"}}), "{control}");
    assert_eq!(control["category"], json!({"vocab": "commercial"}), "{control}");
    assert_eq!(control["config"], json!({"json": {"level": 3}}), "{control}");
    assert_eq!(
        control["temperature"],
        json!({"dataset": {"urn:ngsi-ld:Dataset:one": 25, "@none": 30}}),
        "{control}"
    );

    assert_eq!(amended["type"], json!("Vehicle"), "{amended}");
    assert!(amended.get("scope").is_none(), "{amended}");
    assert!(amended.get("expiresAt").is_none(), "{amended}");
    assert_eq!(amended["name"], json!({"en": "Tower", "de": "Turm"}), "{amended}");
    assert_eq!(amended["category"], json!("commercial"), "{amended}");
    assert_eq!(amended["config"], json!({"level": 3}), "{amended}");
    // Table 4.3.6.8-1 note 2: the default instance, which clause 4.5.4 keys
    // under `@none`, and as the bare value a 1.0 attribute is.
    assert_eq!(amended["temperature"], json!(30), "{amended}");
    // The bare values of every other family are untouched: they carry nothing
    // Tables 4.3.6.8-2 and -3 name.
    assert_eq!(amended["speed"], json!(20), "{amended}");

    scope.finish().await.expect("cleanup failed");
}

/// A subscription that declares no `ngsildConformance` undergoes no
/// backwards-compatibility operation at all: Table 5.2.12-1 makes the member
/// optional and gives it no default, so the notification is the payload this
/// broker's own version produces.
#[tokio::test]
async fn regression_ngsild_conformance_an_undeclared_version_leaves_the_notification_unchanged() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = notification_receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let context = json!([TEST_SUITE_CONTEXT]);

    let entity_id = scope.id("Vehicle:conformance-absent");
    create_entity(
        client,
        &conformance_entity(
            &entity_id,
            &scope.id("Room:conformance-absent"),
            &scope.id("Building:conformance-absent"),
            &context,
        ),
    )
    .await;

    let subscription_id = scope.id("Subscription:conformance-absent");
    let body = subscription_for(&subscription_id, &entity_id, "Vehicle", &endpoint, &context);
    create_subscription(client, &body).await;
    sleep(CACHE_SETTLE).await;

    receiver.drain_requests().await;
    touch_entity(client, &entity_id, "conformance-absent", &context).await;
    let notified = notified_entities_by_subscription(&receiver, 1).await;

    delete_subscriptions(client, &[&subscription_id]).await;
    receiver.shutdown().await.expect("notification receiver shutdown failed");

    let entity = notified.get(&subscription_id).expect("the subscription is notified");

    assert_eq!(entity["type"], json!(["Vehicle", "Car"]), "{entity}");
    assert_eq!(entity["scope"], json!("/conformance"), "{entity}");
    assert!(entity["expiresAt"].is_string(), "{entity}");
    assert_eq!(entity["name"]["type"], json!("LanguageProperty"), "{entity}");
    assert_eq!(entity["category"]["type"], json!("VocabProperty"), "{entity}");
    assert_eq!(entity["refRooms"]["type"], json!("ListRelationship"), "{entity}");
    assert_eq!(entity["refBuilding"]["objectType"], json!("Building"), "{entity}");
    assert!(entity["temperature"].is_array(), "{entity}");
    assert_eq!(entity["speed"]["unitCode"], json!("CEL"), "{entity}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6 gives the amendment to "a Notification", without distinguishing
/// the periodic notification a `timeInterval` subscription sends from the
/// change-driven one: both are sent by the same "A Notification shall be sent
/// as follows" list, whose last step before the send is the amendment.
#[tokio::test]
async fn regression_ngsild_conformance_a_periodic_notification_is_amended_too() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = notification_receiver().await;
    let endpoint = receiver.endpoint_url().expect("notification endpoint URL");
    let context = json!([TEST_SUITE_CONTEXT]);

    let entity_id = scope.id("Vehicle:conformance-periodic");
    create_entity(
        client,
        &conformance_entity(
            &entity_id,
            &scope.id("Room:conformance-periodic"),
            &scope.id("Building:conformance-periodic"),
            &context,
        ),
    )
    .await;

    let amended_id = scope.id("Subscription:conformance-periodic-amended");
    let mut amended_body = subscription_for(&amended_id, &entity_id, "Vehicle", &endpoint, &context);
    amended_body["timeInterval"] = json!(1);
    amended_body["ngsildConformance"] = json!("1.0");
    create_subscription(client, &amended_body).await;

    let control_id = scope.id("Subscription:conformance-periodic-control");
    let mut control_body = subscription_for(&control_id, &entity_id, "Vehicle", &endpoint, &context);
    control_body["timeInterval"] = json!(1);
    create_subscription(client, &control_body).await;

    let notified = notified_entities_by_subscription(&receiver, 2).await;

    delete_subscriptions(client, &[&amended_id, &control_id]).await;
    receiver.shutdown().await.expect("notification receiver shutdown failed");

    let control = notified.get(&control_id).expect("the control subscription is notified periodically");
    let amended = notified.get(&amended_id).expect("the amended subscription is notified periodically");

    assert_eq!(control["type"], json!(["Vehicle", "Car"]), "{control}");
    assert_eq!(control["name"]["type"], json!("LanguageProperty"), "{control}");

    assert_eq!(amended["type"], json!("Vehicle"), "{amended}");
    assert!(amended.get("scope").is_none(), "{amended}");
    assert_eq!(amended["name"]["type"], json!("Property"), "{amended}");
    assert!(amended["speed"].get("unitCode").is_none(), "{amended}");
    assert_eq!(amended["refRooms"]["type"], json!("Relationship"), "{amended}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.11.2.3 makes the payload of a Context Source Registration
/// Subscription the same Subscription datatype, so `ngsildConformance` is
/// admitted and echoed there too. Clause 5.11.7 replaces the clause 5.8.6 "A
/// csource notification shall be sent as follows" list with its own, which
/// names no amendment, and a `CsourceNotification`'s `data` carries Context
/// Source Registrations rather than the Entity data Tables 4.3.6.8-1 to -3
/// describe, so the member has no effect on what such a subscription is sent.
#[tokio::test]
async fn regression_ngsild_conformance_round_trips_through_a_context_source_subscription() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let context = json!([TEST_SUITE_CONTEXT]);

    let subscription_id = scope.id("Subscription:conformance-csource");
    let body = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify"}},
        "ngsildConformance": "1.4",
        "@context": context
    });

    let created = client
        .raw(Method::POST, "csourceSubscriptions")
        .header("content-type", "application/ld+json")
        .json(&body)
        .send()
        .await
        .expect("create context source subscription request");
    let create_status = created.status;
    let create_body = created.body.clone(); // clone: the diagnostic outlives the response

    let path = format!("csourceSubscriptions/{}", encode(&subscription_id));
    let retrieved = client
        .raw(Method::GET, &path)
        .send()
        .await
        .expect("retrieve context source subscription request");
    let retrieve_status = retrieved.status;
    let document = retrieved.json_value().unwrap_or(Value::Null);

    let deleted = client
        .raw(Method::DELETE, &path)
        .send()
        .await
        .expect("delete context source subscription request");

    assert_eq!(create_status, StatusCode::CREATED, "{create_body:?}");
    assert_eq!(retrieve_status, StatusCode::OK, "{document}");
    assert_eq!(document["ngsildConformance"], json!("1.4"), "{document}");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{:?}", deleted.body);

    scope.finish().await.expect("cleanup failed");
}
