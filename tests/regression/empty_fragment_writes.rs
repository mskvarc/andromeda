//! An empty Fragment on the fragment writes of the entity API.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.4 makes an NGSI-LD Fragment an IETF RFC 7396
//! JSON Merge Patch, and under that RFC the empty patch returns its target
//! unchanged. Clauses 5.5.8 and 5.5.12 iterate over the members of the Fragment,
//! so a Fragment that names none does nothing, and none of the clauses 5.6.2.4,
//! 5.6.3.4, 5.6.4.4 and 5.6.17.4 lists an empty Fragment among its errors. The
//! broker therefore answers `{}` with the operation's success status (204) and
//! changes, notifies and records nothing; a missing entity or attribute is still
//! the `ResourceNotFound` those clauses raise. The batch operations that run
//! those fragment writes per entity (clauses 5.6.8.4, 5.6.9.4 and 5.6.20.4)
//! follow it for an entity whose fragment names no member.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

/// The window the temporal reads of this module ask for. Clause 4.11 makes
/// `observedAt` the default `timeproperty`, which no instance of a Core-created
/// entity carries, so the window names the `modifiedAt` the broker stamps on
/// every instance.
const WINDOW: &str = "timeproperty=modifiedAt&timerel=after&timeAt=2000-01-01T00:00:00Z";

/// How long the absence of a notification is observed.
const QUIET: Duration = Duration::from_secs(2);

/// The problem type clauses 5.6.2.4 to 5.6.17.4 raise for a missing entity or
/// attribute.
const RESOURCE_NOT_FOUND: &str = "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound";

/// The fragment write operations that take an Entity Fragment.
#[derive(Clone, Copy, Debug)]
enum EntityFragmentWrite {
    /// Clause 5.6.2, `PATCH /entities/{id}/attrs`.
    UpdateAttributes,
    /// Clause 5.6.3, `POST /entities/{id}/attrs`.
    AppendAttributes,
    /// Clause 5.6.17, `PATCH /entities/{id}`.
    MergeEntity,
}

impl EntityFragmentWrite {
    /// Every entity-level fragment write.
    const ALL: [Self; 3] = [Self::UpdateAttributes, Self::AppendAttributes, Self::MergeEntity];

    /// The HTTP method and path of the operation against `id`.
    fn request(self, id: &str) -> (Method, String) {
        let encoded = urlencoding::encode(id);
        match self {
            Self::UpdateAttributes => (Method::PATCH, format!("/ngsi-ld/v1/entities/{encoded}/attrs")),
            Self::AppendAttributes => (Method::POST, format!("/ngsi-ld/v1/entities/{encoded}/attrs")),
            Self::MergeEntity => (Method::PATCH, format!("/ngsi-ld/v1/entities/{encoded}")),
        }
    }
}

/// The batch operations that run a fragment write for each entity.
#[derive(Clone, Copy, Debug)]
enum BatchFragmentWrite {
    /// Clause 5.6.9, `POST /entityOperations/update`.
    Update,
    /// Clause 5.6.9 with `options=noOverwrite`.
    NoOverwriteUpdate,
    /// Clause 5.6.20, `POST /entityOperations/merge`.
    Merge,
    /// Clause 5.6.8 with `options=update`.
    UpsertUpdate,
}

impl BatchFragmentWrite {
    /// Every batch fragment write.
    const ALL: [Self; 4] = [Self::Update, Self::NoOverwriteUpdate, Self::Merge, Self::UpsertUpdate];

    /// The path of the operation.
    const fn path(self) -> &'static str {
        match self {
            Self::Update => "/ngsi-ld/v1/entityOperations/update",
            Self::NoOverwriteUpdate => "/ngsi-ld/v1/entityOperations/update?options=noOverwrite",
            Self::Merge => "/ngsi-ld/v1/entityOperations/merge",
            Self::UpsertUpdate => "/ngsi-ld/v1/entityOperations/upsert?options=update",
        }
    }
}

/// Send the array `entities` to a batch operation as `application/ld+json`.
async fn batch_write(client: &TestClient, operation: BatchFragmentWrite, entities: &Value) -> TestResponse {
    send(client, Method::POST, operation.path(), "application/ld+json", &entities.to_string()).await
}

/// Send `body` as `content_type` with the given method and path.
async fn send(client: &TestClient, method: Method, path: &str, content_type: &str, body: &str) -> TestResponse {
    client
        .raw(method, path)
        .header("content-type", content_type)
        .body(body.as_bytes().to_vec())
        .send()
        .await
        .expect("fragment write request")
}

/// Partial Attribute Update (clause 5.6.4) of `attribute` with the empty
/// `application/json` fragment.
async fn empty_partial_update(client: &TestClient, id: &str, attribute: &str) -> TestResponse {
    let path = format!("/ngsi-ld/v1/entities/{}/attrs/{attribute}", urlencoding::encode(id));
    send(client, Method::PATCH, &path, "application/json", "{}").await
}

/// Create the vehicle every case writes over.
async fn seed(client: &TestClient, id: &str) {
    let created = client
        .post_entity(&json!({
            "id": id,
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 50},
            "brandName": {"type": "Property", "value": "Mercedes"},
            "@context": [CORE_CONTEXT]
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);
}

/// The entity as stored, system attributes included.
async fn stored(client: &TestClient, id: &str) -> Value {
    let retrieved = client
        .get_entity(id, "options=sysAttrs", "application/ld+json", Some(CORE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    retrieved.json_value().expect("a retrieved entity is JSON")
}

/// The Temporal Evolution of `id`.
async fn evolution(client: &TestClient, id: &str) -> Value {
    let retrieved = client
        .get_temporal_entity(id, WINDOW, Some(CORE_CONTEXT))
        .await
        .expect("retrieve temporal evolution request");
    assert_eq!(retrieved.status, StatusCode::OK, "temporal retrieve: {:?}", retrieved.body);
    retrieved.json_value().expect("a temporal evolution is JSON")
}

/// Assert `response` is the `ResourceNotFound` refusal.
fn assert_not_found(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::NOT_FOUND, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(RESOURCE_NOT_FOUND), "{scenario}: {problem}");
}

/// Clauses 5.4 and 5.6.4: an empty fragment on Partial Attribute Update answers
/// 204 and leaves the entity, `modifiedAt` included, as it was, and no
/// subscriber is notified.
#[tokio::test]
async fn regression_an_empty_attribute_fragment_is_a_no_op_on_partial_attribute_update() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let id = scope.id("Vehicle:empty-partial");
    let subscription_id = scope.id("Subscription:empty-partial");
    seed(client, &id).await;
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription_id,
            "type": "Subscription",
            "entities": [{"id": id, "type": "Vehicle"}],
            "notification": {"format": "normalized", "endpoint": {"uri": receiver.endpoint_url(), "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);
    receiver.clear_notifications().await;
    let before = stored(client, &id).await;

    let answer = empty_partial_update(client, &id, "speed").await;

    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{:?}", answer.body);
    assert_eq!(stored(client, &id).await, before);
    receiver.wait_for_no_subscription_notification(&subscription_id, QUIET).await;

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    receiver.shutdown().await;
    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.4 with clause 6.3.5: the same request as `application/ld+json`
/// carrying only its `@context` is an empty fragment too.
#[tokio::test]
async fn regression_an_attribute_fragment_holding_only_a_context_is_a_no_op_on_partial_attribute_update() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:context-only-partial");
    seed(client, &id).await;
    let before = stored(client, &id).await;

    let path = format!("/ngsi-ld/v1/entities/{}/attrs/speed", urlencoding::encode(&id));
    let body = json!({"@context": [CORE_CONTEXT]}).to_string();
    let answer = send(client, Method::PATCH, &path, "application/ld+json", &body).await;

    assert_eq!(answer.status, StatusCode::NO_CONTENT, "{:?}", answer.body);
    assert_eq!(stored(client, &id).await, before);

    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.2.4, 5.6.3.4 and 5.6.17.4: an empty entity fragment is a no-op
/// on Update Attributes, Append Attributes and Merge Entity, and no subscriber
/// is notified.
#[tokio::test]
async fn regression_an_empty_entity_fragment_is_a_no_op_on_every_entity_fragment_write() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let id = scope.id("Vehicle:empty-entity-fragment");
    let subscription_id = scope.id("Subscription:empty-entity-fragment");
    seed(client, &id).await;
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription_id,
            "type": "Subscription",
            "entities": [{"id": id, "type": "Vehicle"}],
            "notification": {"format": "normalized", "endpoint": {"uri": receiver.endpoint_url(), "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);
    receiver.clear_notifications().await;
    let before = stored(client, &id).await;

    for operation in EntityFragmentWrite::ALL {
        let (method, path) = operation.request(&id);
        let answer = send(client, method, &path, "application/json", "{}").await;
        assert_eq!(answer.status, StatusCode::NO_CONTENT, "{operation:?}: {:?}", answer.body);
        assert_eq!(stored(client, &id).await, before, "{operation:?}");
    }
    receiver.wait_for_no_subscription_notification(&subscription_id, QUIET).await;

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    receiver.shutdown().await;
    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.11 and 4.5.7: no empty fragment records a Temporal Evolution
/// instance.
#[tokio::test]
async fn regression_an_empty_fragment_records_no_temporal_evolution() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:empty-temporal");
    seed(client, &id).await;
    let before = evolution(client, &id).await;

    assert_eq!(empty_partial_update(client, &id, "speed").await.status, StatusCode::NO_CONTENT);
    for operation in EntityFragmentWrite::ALL {
        let (method, path) = operation.request(&id);
        let answer = send(client, method, &path, "application/json", "{}").await;
        assert_eq!(answer.status, StatusCode::NO_CONTENT, "{operation:?}: {:?}", answer.body);
    }

    let after = evolution(client, &id).await;
    assert_eq!(after, before);
    for attribute in ["speed", "brandName"] {
        let instances = after[attribute].as_array().expect("the evolution of an attribute is an array of instances");
        assert_eq!(instances.len(), 1, "{attribute} holds only the instance the creation recorded: {after}");
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.2.4, 5.6.3.4, 5.6.4.4 and 5.6.17.4: an unknown entity is still a
/// `ResourceNotFound` when the fragment is empty.
#[tokio::test]
async fn regression_an_empty_fragment_still_answers_not_found_for_an_unknown_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let missing = scope.id("Vehicle:empty-missing");

    assert_not_found(&empty_partial_update(client, &missing, "speed").await, "partial attribute update");
    for operation in EntityFragmentWrite::ALL {
        let (method, path) = operation.request(&missing);
        assert_not_found(&send(client, method, &path, "application/json", "{}").await, &format!("{operation:?}"));
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.8.4 (update mode), 5.6.9.4 and 5.6.20.4: an entity of a batch
/// whose fragment names no member beside its identity is a success that changes,
/// notifies and records nothing, on every batch operation that runs a fragment
/// write per entity.
#[tokio::test]
async fn regression_an_empty_batch_fragment_is_a_no_op_on_every_batch_fragment_write() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let id = scope.id("Vehicle:empty-batch-fragment");
    let subscription_id = scope.id("Subscription:empty-batch-fragment");
    seed(client, &id).await;
    let subscribed = client
        .create_subscription(&json!({
            "id": subscription_id,
            "type": "Subscription",
            "entities": [{"id": id, "type": "Vehicle"}],
            "notification": {"format": "normalized", "endpoint": {"uri": receiver.endpoint_url(), "accept": "application/json"}},
            "@context": [CORE_CONTEXT]
        }))
        .await
        .expect("create subscription request");
    assert_eq!(subscribed.status, StatusCode::CREATED, "{:?}", subscribed.body);
    receiver.clear_notifications().await;
    let before = stored(client, &id).await;
    let evolution_before = evolution(client, &id).await;

    for operation in BatchFragmentWrite::ALL {
        let answer = batch_write(client, operation, &json!([{"id": id, "type": "Vehicle", "@context": [CORE_CONTEXT]}])).await;
        assert_eq!(answer.status, StatusCode::NO_CONTENT, "{operation:?}: {:?}", answer.body);
        assert_eq!(stored(client, &id).await, before, "{operation:?}");
    }
    receiver.wait_for_no_subscription_notification(&subscription_id, QUIET).await;
    assert_eq!(evolution(client, &id).await, evolution_before);

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    receiver.shutdown().await;
    scope.finish().await.expect("environment cleanup");
}

/// Clauses 5.6.9.4 and 5.6.20.4: a batch fragment that does change a value
/// still writes, so the skipped write is decided by the change and not by the
/// operation.
#[tokio::test]
async fn regression_a_batch_fragment_that_changes_a_value_still_moves_the_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:changing-batch-fragment");
    seed(client, &id).await;

    for (operation, speed) in [(BatchFragmentWrite::Update, 60), (BatchFragmentWrite::Merge, 70)] {
        let before = stored(client, &id).await;
        let fragment = json!([{"id": id, "type": "Vehicle", "speed": {"type": "Property", "value": speed}, "@context": [CORE_CONTEXT]}]);

        let answer = batch_write(client, operation, &fragment).await;

        assert_eq!(answer.status, StatusCode::NO_CONTENT, "{operation:?}: {:?}", answer.body);
        let after = stored(client, &id).await;
        assert_eq!(after["speed"]["value"], json!(speed), "{operation:?}: {after}");
        assert!(
            after["modifiedAt"] != before["modifiedAt"],
            "{operation:?}: modifiedAt moves with a real change: {after}"
        );
    }

    scope.finish().await.expect("environment cleanup");
}

/// Clause 5.6.4.4: an attribute the entity does not hold is still a
/// `ResourceNotFound` when the fragment is empty.
#[tokio::test]
async fn regression_an_empty_attribute_fragment_still_answers_not_found_for_an_absent_attribute() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Vehicle:empty-absent-attribute");
    seed(client, &id).await;

    assert_not_found(&empty_partial_update(client, &id, "fuel").await, "absent attribute");

    scope.finish().await.expect("environment cleanup");
}
