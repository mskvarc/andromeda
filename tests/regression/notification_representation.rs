//! What a notification body carries: where its `@context` is written, and what
//! `notification.pick` and `notification.omit` leave of each Entity.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.6 has an `application/ld+json` body
//! "include a JSON-LD @context", and clause 6.3.5 forbids mixing that with the
//! `Link` header the `application/json` case uses. Andromeda wrote the member
//! once, on the Notification envelope, and stripped it from every Entity in
//! `data`. The body is two documents, not one: Table 5.3.1-1 gives the
//! Notification an `id` that is the JSON-LD `@id` and a `type` that is the
//! `@type`, and clause 4.5.1 gives every Entity the same two, so a subscriber
//! that lifts one Entity out of `data` held something it could not expand.
//! Both carry it now, with the one @context clause 5.8.6 names, "the one
//! specified in the jsonldContext field".
//!
//! Clause 5.8.6 also reduces "every Entity within the payload body ... down to
//! only contain the defined entity members listed" when `notification.pick` or
//! `notification.omit` is set -- clause 5.7.1.5's sentence for a query
//! response, which clause 4.21 governs in both places. The notification path
//! read it differently, keeping `id` and `type` on the ground that Table
//! 5.2.4-1 makes them mandatory for an Entity; the query path had the same
//! reading until clause 5.7.1.5's own words settled it, and the notification
//! now applies that one rule after compaction, where `id`, `type` and `scope`
//! exist as such.
//!
//! Clause 5.8.6 holds one member back from that rule: a notification carries
//! Entities and an Entity is only actionable if it carries its `id` (clause
//! 4.5.1), so `id` survives a `notification.pick`/`notification.omit` that
//! would otherwise drop it, while `type` and `scope` follow the projection as a
//! query response does.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

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

/// A Building carrying two attributes, so a projection has something to drop.
fn building(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Building",
        "name": {"type": "Property", "value": "Tour Eiffel"},
        "category": {"type": "Property", "value": "office"},
        "@context": [CORE_CONTEXT]
    })
}

/// A subscription on `entity_id`, notified over `accept`.
fn subscription(id: &str, entity_id: &str, endpoint: &str, accept: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"id": entity_id, "type": "Building"}],
        "notification": {"format": "normalized", "endpoint": {"uri": endpoint, "accept": accept}},
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

/// Change an attribute, which is what triggers the notification.
async fn touch(client: &TestClient, entity_id: &str) {
    let updated = client
        .append_entity_attrs(
            entity_id,
            &json!({"name": {"type": "Property", "value": "Le Tour Eiffel"}, "@context": [CORE_CONTEXT]}),
        )
        .await
        .expect("append attributes");
    assert!(updated.status.is_success(), "append: {:?}", updated.body);
}

/// Clause 6.3.6: the `application/ld+json` body includes a JSON-LD `@context`,
/// and clause 4.5.1 makes each Entity in `data` a document that carries it too.
#[tokio::test]
async fn regression_notification_context_a_jsonld_notification_writes_it_on_every_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:jsonld-context");
    let subscription_id = scope.id("Subscription:jsonld-context");

    create(client, &building(&entity_id)).await;
    subscribe(
        client,
        &subscription(&subscription_id, &entity_id, &receiver.endpoint_url(), "application/ld+json"),
    )
    .await;
    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert_eq!(notification.body["@context"], json!(CORE_CONTEXT), "the envelope: {}", notification.body);
    assert_eq!(
        notification.body["data"][0]["@context"],
        json!(CORE_CONTEXT),
        "and the Entity it carries: {}",
        notification.body
    );
    assert!(
        notification.headers.get("link").is_none(),
        "clause 6.3.5 forbids mixing the body context with a Link header"
    );

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.3.5's other half: an `application/json` notification carries the
/// `@context` in the `Link` header and nowhere in the body, envelope and
/// Entity alike.
#[tokio::test]
async fn regression_notification_context_a_json_notification_writes_it_in_no_part_of_the_body() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:json-context");
    let subscription_id = scope.id("Subscription:json-context");

    create(client, &building(&entity_id)).await;
    subscribe(
        client,
        &subscription(&subscription_id, &entity_id, &receiver.endpoint_url(), "application/json"),
    )
    .await;
    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    assert!(notification.body.get("@context").is_none(), "the envelope: {}", notification.body);
    assert!(
        notification.body["data"][0].get("@context").is_none(),
        "and the Entity it carries: {}",
        notification.body
    );
    assert!(notification.headers.get("link").is_some(), "clause 6.3.6 puts it in the Link header instead");

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6: a `notification.pick` reduces every Entity in the payload to
/// the members it lists, `type` included when it is not one of them.
#[tokio::test]
async fn regression_notification_projection_a_pick_drops_the_type_it_does_not_name() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:pick-members");
    let subscription_id = scope.id("Subscription:pick-members");

    create(client, &building(&entity_id)).await;
    let mut body = subscription(&subscription_id, &entity_id, &receiver.endpoint_url(), "application/json");
    body["notification"]["pick"] = json!(["id", "name"]);
    subscribe(client, &body).await;
    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    let notified = &notification.body["data"][0];
    assert_eq!(notified["id"], json!(entity_id), "{notified}");
    assert!(notified.get("name").is_some(), "the picked attribute stays: {notified}");
    assert!(notified.get("type").is_none(), "`type` was not picked: {notified}");
    assert!(notified.get("category").is_none(), "nor was the other attribute: {notified}");

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.8.6 with clause 4.5.1: a notified Entity must carry its `id` to be
/// actionable, so a `notification.pick` naming only attributes still leaves
/// `id` in place, even though the same projection drops it from a query
/// response (clause 5.7.1.5). `type`, not force-kept, follows the projection.
#[tokio::test]
async fn regression_notification_projection_a_pick_naming_only_attributes_keeps_id() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:pick-only-attributes");
    let subscription_id = scope.id("Subscription:pick-only-attributes");

    create(client, &building(&entity_id)).await;
    let mut body = subscription(&subscription_id, &entity_id, &receiver.endpoint_url(), "application/json");
    body["notification"]["pick"] = json!(["name"]);
    subscribe(client, &body).await;
    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    let notified = &notification.body["data"][0];
    assert_eq!(
        notified["id"],
        json!(entity_id),
        "the id is force-kept so the notification is actionable: {notified}"
    );
    assert!(notified.get("name").is_some(), "the picked attribute stays: {notified}");
    assert!(notified.get("type").is_none(), "`type` was not picked and is not force-kept: {notified}");
    assert!(notified.get("category").is_none(), "the attribute the pick did not name is gone: {notified}");

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}

/// The `omit` half of the same sentence: what it names goes, `type` included.
#[tokio::test]
async fn regression_notification_projection_an_omit_drops_the_type_it_names() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("Building:omit-members");
    let subscription_id = scope.id("Subscription:omit-members");

    create(client, &building(&entity_id)).await;
    let mut body = subscription(&subscription_id, &entity_id, &receiver.endpoint_url(), "application/json");
    body["notification"]["omit"] = json!(["type", "name"]);
    subscribe(client, &body).await;
    touch(client, &entity_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, ARRIVAL).await;
    let notified = &notification.body["data"][0];
    assert_eq!(notified["id"], json!(entity_id), "{notified}");
    assert!(notified.get("type").is_none(), "`type` was omitted: {notified}");
    assert!(notified.get("name").is_none(), "and so was the attribute: {notified}");
    assert!(notified.get("category").is_some(), "what was not omitted stays: {notified}");

    client.delete_subscription(&subscription_id).await.expect("delete subscription");
    client.delete_entity(&entity_id).await.expect("delete entity");
    scope.finish().await.expect("cleanup failed");
}
