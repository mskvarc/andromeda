//! An identifier carrying non-ASCII characters survives a round trip verbatim.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.6.2 NOTE 2 extends the term "URI" to cover
//! an Internationalized Resource Identifier (IETF RFC 3987), whose characters
//! run beyond ASCII. A Relationship whose object is `urn:ngsi-ld:Ciudad:PARIS`
//! with a non-ASCII vowel, or an entity so named, is therefore a valid NGSI-LD
//! identifier and must be given back character for character.
//!
//! The identifier is held verbatim as an IRI rather than parsed as a URN and
//! then a URL: the URN parser refuses a non-ASCII namespace-specific string, and
//! a URL parser's cannot-be-a-base encoder would rewrite every non-ASCII octet as
//! a percent escape, so `Paris` with an accented vowel would become `Par%C3%ADs`.
//! Every representation carries the character the client wrote.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::time::Duration;

/// How long a notification that is owed is waited for.
const ARRIVAL: Duration = Duration::from_secs(10);

/// The non-ASCII identifier under test: a Spanish city name whose accented
/// vowel is the character a URL encoder would corrupt.
const NON_ASCII_OBJECT: &str = "urn:ngsi-ld:Ciudad:Par\u{00ed}s";

/// The datasetId distinguishing the non-ASCII instance in the multi-instance
/// fixture. Kept ASCII so the test isolates the Relationship object.
const NON_ASCII_DATASET: &str = "urn:ngsi-ld:Dataset:spanish-name";

/// A Building whose Relationship points at the non-ASCII identifier, plus a
/// plain Property the notification path can change to trigger delivery.
fn building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "locatedAt": {"type": "Relationship", "object": NON_ASCII_OBJECT},
        "category": {"type": "Property", "value": "office"},
        "@context": [CORE_CONTEXT]
    })
}

/// A Building with two `locatedAt` Relationship instances distinguished by
/// `datasetId`, one pointing at the non-ASCII identifier, so the clause 4.5.5
/// multi-instance dataset-map rendering is exercised as well as the single one.
fn building_multi_instance(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "locatedAt": [
            {"type": "Relationship", "object": "urn:ngsi-ld:City:London", "datasetId": "urn:ngsi-ld:Dataset:ascii"},
            {"type": "Relationship", "object": NON_ASCII_OBJECT, "datasetId": NON_ASCII_DATASET}
        ],
        "category": {"type": "Property", "value": "office"},
        "@context": [CORE_CONTEXT]
    })
}

/// A subscription on `entity_id`, notified in `format` over `endpoint`.
fn subscription(id: &str, entity_id: &str, endpoint: &str, format: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"id": entity_id, "type": "Building"}],
        "notification": {"format": format, "endpoint": {"uri": endpoint, "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    })
}

/// Create an entity and assert it was taken.
async fn create(client: &TestClient, body: &Value) {
    let created = client.post_entity(body).send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);
}

/// Create a subscription and assert it was taken.
async fn subscribe(client: &TestClient, body: &Value) {
    let created = client.create_subscription(body).await.expect("create subscription");
    assert_eq!(created.status, StatusCode::CREATED, "create subscription: {:?}", created.body);
}

/// Change the Property, which triggers a notification without touching the
/// Relationship whose spelling is under test.
async fn touch(client: &TestClient, entity_id: &str) {
    let updated = client
        .append_entity_attrs(
            entity_id,
            &json!({"category": {"type": "Property", "value": "retail"}, "@context": [CORE_CONTEXT]}),
        )
        .await
        .expect("append attributes");
    assert!(updated.status.is_success(), "append: {:?}", updated.body);
}

/// The Relationship object as a notification carries it, in whichever
/// representation the subscription asked for: a normalized body nests it under
/// `object`, a keyValues body renders the attribute as the bare target.
fn notified_object<'a>(entity: &'a Value, format: &str) -> &'a Value {
    if format == "keyValues" {
        &entity["locatedAt"]
    } else {
        &entity["locatedAt"]["object"]
    }
}

/// Drive one notification format end to end and assert the non-ASCII target is
/// carried verbatim.
async fn assert_notification_keeps_identifier(format: &str) {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id(&format!("Building:non-ascii-{format}"));
    let subscription_id = scope.id(&format!("Subscription:non-ascii-{format}"));

    create(client, &building(&entity_id)).await;
    subscribe(client, &subscription(&subscription_id, &entity_id, &receiver.endpoint_url(), format)).await;
    receiver.clear_notifications().await;

    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    let entity = &notification.body["data"][0];
    assert_eq!(entity["id"], json!(entity_id), "{}", notification.body);
    assert_eq!(
        notified_object(entity, format),
        &json!(NON_ASCII_OBJECT),
        "the {format} notification keeps the non-ASCII target verbatim: {}",
        notification.body
    );

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.2 NOTE 2: a normalized notification keeps the non-ASCII target
/// verbatim.
#[tokio::test]
async fn regression_non_ascii_identifier_a_normalized_notification_keeps_it_verbatim() {
    assert_notification_keeps_identifier("normalized").await;
}

/// Clause 4.6.2 NOTE 2: a concise notification, which carries no `type`
/// discriminator, keeps the non-ASCII target verbatim.
#[tokio::test]
async fn regression_non_ascii_identifier_a_concise_notification_keeps_it_verbatim() {
    assert_notification_keeps_identifier("concise").await;
}

/// Clause 4.6.2 NOTE 2: a keyValues notification renders the Relationship as a
/// bare target and keeps it verbatim.
#[tokio::test]
async fn regression_non_ascii_identifier_a_keyvalues_notification_keeps_it_verbatim() {
    assert_notification_keeps_identifier("keyValues").await;
}

/// Clause 4.6.2 NOTE 2: a plain retrieval of the entity carries the non-ASCII
/// Relationship target verbatim, in every representation.
#[tokio::test]
async fn regression_non_ascii_identifier_a_plain_retrieval_keeps_it_verbatim() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_id = scope.id("Building:non-ascii-retrieval");

    create(client, &building(&entity_id)).await;

    let normalized = client
        .get_entity(&entity_id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve normalized");
    assert_eq!(normalized.status, StatusCode::OK, "{:?}", normalized.body);
    let normalized_body = normalized.json_value().expect("a normalized retrieval answers JSON");
    assert_eq!(normalized_body["locatedAt"]["object"], json!(NON_ASCII_OBJECT), "normalized: {normalized_body}");

    let concise = client
        .get_entity(&entity_id, "options=concise", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve concise");
    assert_eq!(concise.status, StatusCode::OK, "{:?}", concise.body);
    let concise_body = concise.json_value().expect("a concise retrieval answers JSON");
    assert_eq!(concise_body["locatedAt"]["object"], json!(NON_ASCII_OBJECT), "concise: {concise_body}");

    let key_values = client
        .get_entity(&entity_id, "options=keyValues", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve keyValues");
    assert_eq!(key_values.status, StatusCode::OK, "{:?}", key_values.body);
    let key_values_body = key_values.json_value().expect("a keyValues retrieval answers JSON");
    assert_eq!(key_values_body["locatedAt"], json!(NON_ASCII_OBJECT), "keyValues: {key_values_body}");

    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.2 NOTE 2 with clause 4.5.5: a keyValues notification of a
/// multi-instance Relationship keeps the non-ASCII target verbatim in the
/// dataset-map rendering, where `urn:ngsi-ld:Ciudad:Par\u{00ed}s` must not come back
/// double-encoded as
/// `urn:ngsi-ld:Ciudad:ParÃ­s`.
#[tokio::test]
async fn regression_non_ascii_identifier_a_multi_instance_keyvalues_notification_keeps_it_verbatim() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:non-ascii-multi-notify");
    let subscription_id = scope.id("Subscription:non-ascii-multi-notify");

    create(client, &building_multi_instance(&entity_id)).await;
    subscribe(client, &subscription(&subscription_id, &entity_id, &receiver.endpoint_url(), "keyValues")).await;
    receiver.clear_notifications().await;

    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    let entity = &notification.body["data"][0];
    assert_eq!(entity["id"], json!(entity_id), "{}", notification.body);
    assert_eq!(
        entity["locatedAt"]["dataset"][NON_ASCII_DATASET],
        json!(NON_ASCII_OBJECT),
        "the keyValues notification keeps the non-ASCII multi-instance target verbatim: {}",
        notification.body
    );

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.6.2 NOTE 2 with clause 4.5.5: a multi-instance Relationship keeps
/// the non-ASCII target verbatim in the dataset-map rendering too. The
/// single-instance path holds the object as a typed URI, but the dataset-map
/// path must not corrupt a non-ASCII target: `urn:ngsi-ld:Ciudad:Par\u{00ed}s`
/// must not come back as the double-encoded `urn:ngsi-ld:Ciudad:ParÃ­s`.
#[tokio::test]
async fn regression_non_ascii_identifier_a_multi_instance_retrieval_keeps_it_verbatim() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let entity_id = scope.id("Building:non-ascii-multi");

    create(client, &building_multi_instance(&entity_id)).await;

    let normalized = client
        .get_entity(&entity_id, "", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve normalized");
    assert_eq!(normalized.status, StatusCode::OK, "{:?}", normalized.body);
    let normalized_body = normalized.json_value().expect("a normalized retrieval answers JSON");
    let spanish = normalized_body["locatedAt"]
        .as_array()
        .expect("a multi-instance Relationship is an array")
        .iter()
        .find(|instance| instance["datasetId"] == json!(NON_ASCII_DATASET))
        .expect("the non-ASCII dataset instance is present");
    assert_eq!(spanish["object"], json!(NON_ASCII_OBJECT), "normalized multi-instance: {normalized_body}");

    let key_values = client
        .get_entity(&entity_id, "options=keyValues", "application/json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve keyValues");
    assert_eq!(key_values.status, StatusCode::OK, "{:?}", key_values.body);
    let key_values_body = key_values.json_value().expect("a keyValues retrieval answers JSON");
    assert_eq!(
        key_values_body["locatedAt"]["dataset"][NON_ASCII_DATASET],
        json!(NON_ASCII_OBJECT),
        "keyValues dataset-map multi-instance: {key_values_body}"
    );

    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}
