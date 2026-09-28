//! A broker in live delivery mode still drains the deliveries its tenant
//! database queued.
//!
//! The queue lives in the tenant database, not in the broker: a durable broker
//! sharing that database, or this broker before it was switched to live
//! delivery, may have left deliveries that are still due, and the tenant's
//! pending work reports them due to whichever broker wakes the tenant. A live
//! delivery broker must therefore run the notification-delivery drain too, or
//! those deliveries are never sent and keep the tenant due forever.
//!
//! These verifications run against the configured broker, which the base
//! stack runs in the default live delivery mode, and seed the queued rows the
//! way a durable commit would have left them. Clause 5.8.6: a queued
//! notification is sent to the Subscription's `endpoint.uri`; a queued row that
//! can never be sent is retired with its reason instead of being leased again
//! at every lease expiry.

use crate::notification_adapter::NotificationAdapter;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{Instant, sleep};
use uuid::Uuid;

/// Entity type the verifications notify about, spelled as an absolute IRI so
/// the stored snapshot is the expanded form whatever context is in scope.
pub const PROBE_TYPE: &str = "https://andromeda.test/durable-queue/LiveDrainProbe";

/// Attribute the undeliverable deletion names, as an absolute IRI.
const PROBE_ATTRIBUTE: &str = "https://andromeda.test/durable-queue/probeName";

/// How long a queued delivery may take to be claimed: the drain's idle
/// fallback backs off to its 30 s ceiling, and the tenant's pacer may rest it
/// until its next obligation read.
pub const DRAIN_DEADLINE: Duration = Duration::from_secs(90);

/// How long to wait for an acknowledgement recorded after its notification.
pub const ACKNOWLEDGEMENT_DEADLINE: Duration = Duration::from_secs(15);

/// Create the probe Entity, then a Subscription on it notifying `endpoint` of
/// its creation, and return the Subscription's surrogate key.
///
/// The Entity exists before the Subscription does, so the broker notifies
/// nothing of its own: whatever reaches the endpoint came from the queue.
///
/// `pool` is the database of the tenant `client` addresses.
pub async fn subscribe_to_probe(client: &TestClient, pool: &PgPool, entity_id: &str, subscription_id: &str, endpoint: &str) -> i64 {
    create_probe(client, entity_id).await;
    subscribe_to_created_probe(client, pool, entity_id, subscription_id, endpoint).await
}

/// Create the probe Entity under the tenant `client` addresses.
pub async fn create_probe(client: &TestClient, entity_id: &str) {
    let entity = json!({"id": entity_id, "type": PROBE_TYPE, "@context": [TEST_SUITE_CONTEXT]});
    let created = client.post_entity(&entity).send().await.expect("create the probe entity");
    assert_eq!(created.status, StatusCode::CREATED, "entity create: {:?}", created.body);
}

/// Subscribe `endpoint` to the creation of the probe Entity that already
/// exists, and return the Subscription's surrogate key.
pub async fn subscribe_to_created_probe(client: &TestClient, pool: &PgPool, entity_id: &str, subscription_id: &str, endpoint: &str) -> i64 {
    let subscription = json!({
        "id": subscription_id,
        "type": "Subscription",
        "entities": [{"id": entity_id, "type": PROBE_TYPE}],
        "notificationTrigger": ["entityCreated"],
        "notification": {"endpoint": {"uri": endpoint, "accept": "application/json"}},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.create_subscription(&subscription).await.expect("create the subscription");
    assert_eq!(created.status, StatusCode::CREATED, "subscription create: {:?}", created.body);

    sqlx::query_scalar("SELECT subscription_key FROM subscriptions WHERE id = $1")
        .bind(subscription_id)
        .fetch_one(pool)
        .await
        .expect("stored surrogate key")
}

/// Queue one due delivery of one event for `subscription_key`, as a durable
/// commit would have, and return the delivery's key.
pub async fn queue_delivery(pool: &PgPool, subscription_key: i64, event: QueuedEvent<'_>) -> i64 {
    let event_key: i64 = sqlx::query_scalar(
        "INSERT INTO notification_events (created_at, event_ordinal, lifecycle, write_id, entity_id, change_kinds, change_attribute_name_keys, change_dataset_ids, entity_snapshot) \
         VALUES (NOW(), 0, $1, $2, $3, $4, $5, $6, $7) RETURNING event_key",
    )
    .bind(event.lifecycle)
    .bind(Uuid::new_v4())
    .bind(event.entity_id)
    .bind(event.change.iter().map(|instance| instance.kind).collect::<Vec<i16>>())
    .bind(event.change.iter().map(|instance| instance.attribute_name_key).collect::<Vec<i32>>())
    .bind(event.change.iter().map(|instance| instance.dataset_id).collect::<Vec<Option<&str>>>())
    .bind(event.entity_snapshot)
    .fetch_one(pool)
    .await
    .expect("queue the notification event");
    sqlx::query_scalar("INSERT INTO notification_deliveries (event_key, subscription_key, next_attempt_at) VALUES ($1, $2, NOW()) RETURNING delivery_key")
        .bind(event_key)
        .bind(subscription_key)
        .fetch_one(pool)
        .await
        .expect("queue the notification delivery")
}

/// The stored shape of one queued notification event.
pub struct QueuedEvent<'a> {
    /// Compact lifecycle code: 1 created, 2 modified, 3 deleted.
    pub lifecycle: i16,
    /// The stored entity identifier, valid or not.
    pub entity_id: &'a str,
    /// Every Attribute instance the change touched.
    pub change: Vec<QueuedInstance<'a>>,
    /// The stored expanded Entity.
    pub entity_snapshot: Value,
}

/// The stored shape of one Attribute instance a queued change touched.
pub struct QueuedInstance<'a> {
    /// Compact transition code: 1 created, 2 updated, 3 deleted.
    pub kind: i16,
    /// Dictionary key of the instance's attribute.
    pub attribute_name_key: i32,
    /// The stored `datasetId`, valid or not, absent for the default instance.
    pub dataset_id: Option<&'a str>,
}

/// The dictionary key of `name` in the tenant's attribute name dictionary.
async fn attribute_name_key(pool: &PgPool, name: &str) -> i32 {
    sqlx::query_scalar(
        "INSERT INTO attribute_names (name) VALUES ($1) ON CONFLICT ON CONSTRAINT uq_attribute_names_name DO UPDATE SET name = EXCLUDED.name RETURNING attribute_name_key",
    )
    .bind(name)
    .fetch_one(pool)
    .await
    .expect("attribute name key")
}

/// Voids the tenant pacer's last obligation read, so the drain's next fallback
/// tick reads the rows seeded behind the broker's back instead of resting on a
/// verdict taken before they existed.
async fn touch_tenant(scope: &TestEnvironment, subscription_id: &str) {
    let retrieved = scope
        .client()
        .retrieve_subscription(subscription_id, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve the subscription");
    assert_eq!(retrieved.status, StatusCode::OK);
}

/// Wait until the delivery's row records an outcome, returning when it was
/// delivered and exhausted, its attempts, and its recorded failure.
pub async fn settled_delivery(pool: &PgPool, delivery: i64, deadline: Duration) -> (bool, bool, i32, Option<String>) {
    let give_up = Instant::now() + deadline;
    loop {
        let state: (bool, bool, i32, Option<String>) = sqlx::query_as(
            "SELECT delivered_at IS NOT NULL, exhausted_at IS NOT NULL, attempt_count, last_error FROM notification_deliveries WHERE delivery_key = $1",
        )
        .bind(delivery)
        .fetch_one(pool)
        .await
        .expect("delivery state");
        if state.0 || state.1 || Instant::now() >= give_up {
            return state;
        }
        sleep(Duration::from_millis(250)).await;
    }
}

#[tokio::test]
#[serial(broker_delivery)]
async fn durable_queue_a_live_delivery_broker_delivers_a_notification_its_database_queued() {
    let scope = TestEnvironment::from_suite("durablequeue").await.expect("isolated test environment");
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("LiveDrainProbe:queued");
    let subscription_id = scope.id("Subscription:live-drain");
    let subscription_key = subscribe_to_probe(scope.client(), scope.database_pool(), &entity_id, &subscription_id, &receiver.endpoint_url()).await;
    receiver.clear_notifications().await;

    let delivery = queue_delivery(
        scope.database_pool(),
        subscription_key,
        QueuedEvent {
            lifecycle: 1,
            entity_id: &entity_id,
            change: Vec::new(),
            entity_snapshot: json!([{"@id": entity_id, "@type": [PROBE_TYPE]}]),
        },
    )
    .await;
    touch_tenant(&scope, &subscription_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, DRAIN_DEADLINE).await;
    assert_eq!(
        notification.body["data"][0]["id"],
        json!(entity_id),
        "the queued notification carries its Entity"
    );
    let (delivered, exhausted, _, last_error) = settled_delivery(scope.database_pool(), delivery, ACKNOWLEDGEMENT_DEADLINE).await;
    assert!(delivered, "the drain acknowledges the delivery it sent");
    assert!(!exhausted, "{last_error:?}");

    receiver.shutdown().await;
    scope.client().delete_subscription(&subscription_id).await.expect("delete subscription");
    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
#[serial(broker_delivery)]
async fn durable_queue_a_live_delivery_broker_dead_letters_deliveries_it_can_never_send_and_sends_their_peer() {
    let scope = TestEnvironment::from_suite("durablequeue").await.expect("isolated test environment");
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("LiveDrainProbe:poisoned");
    let subscription_id = scope.id("Subscription:live-drain-poisoned");
    let subscription_key = subscribe_to_probe(scope.client(), scope.database_pool(), &entity_id, &subscription_id, &receiver.endpoint_url()).await;
    receiver.clear_notifications().await;
    let pool = scope.database_pool();

    // A stored entity identifier no URI parse accepts: the claim cannot read
    // the row back at all. It keeps the run's prefix so cleanup still finds it.
    let unreadable_entity = format!("{entity_id} not a uri");
    let unreadable = queue_delivery(
        pool,
        subscription_key,
        QueuedEvent {
            lifecycle: 1,
            entity_id: &unreadable_entity,
            change: Vec::new(),
            entity_snapshot: json!([{"@id": entity_id, "@type": [PROBE_TYPE]}]),
        },
    )
    .await;
    // An attribute deletion whose stored change names the deleted instance by
    // a datasetId that is not an identifier: the entity reads back, but the
    // change it records never does.
    let name_key = attribute_name_key(pool, PROBE_ATTRIBUTE).await;
    let unevaluable = queue_delivery(
        pool,
        subscription_key,
        QueuedEvent {
            lifecycle: 2,
            entity_id: &entity_id,
            change: vec![QueuedInstance {
                kind: 3,
                attribute_name_key: name_key,
                dataset_id: Some("not a dataset identifier"),
            }],
            entity_snapshot: json!([{"@id": entity_id, "@type": [PROBE_TYPE]}]),
        },
    )
    .await;
    let deliverable = queue_delivery(
        pool,
        subscription_key,
        QueuedEvent {
            lifecycle: 1,
            entity_id: &entity_id,
            change: Vec::new(),
            entity_snapshot: json!([{"@id": entity_id, "@type": [PROBE_TYPE]}]),
        },
    )
    .await;
    touch_tenant(&scope, &subscription_id).await;

    let notification = receiver.wait_for_subscription_notification(&subscription_id, DRAIN_DEADLINE).await;
    assert_eq!(notification.body["data"][0]["id"], json!(entity_id), "the deliverable peer is still sent");
    let (delivered, _, _, _) = settled_delivery(pool, deliverable, ACKNOWLEDGEMENT_DEADLINE).await;
    assert!(delivered, "the deliverable peer is acknowledged");

    for (delivery, reason) in [(unreadable, "invalid entity URI"), (unevaluable, "invalid datasetId")] {
        let (delivered, exhausted, attempts, last_error) = settled_delivery(pool, delivery, ACKNOWLEDGEMENT_DEADLINE).await;
        assert!(!delivered, "delivery {delivery} was never sent");
        assert!(exhausted, "delivery {delivery} is dead-lettered instead of leased again at every lease expiry");
        assert_eq!(attempts, 1, "delivery {delivery} is retired on its first claim");
        let last_error = last_error.expect("a dead-lettered delivery records why");
        assert!(last_error.contains(reason), "delivery {delivery}: {last_error}");
    }

    receiver.shutdown().await;
    scope.client().delete_subscription(&subscription_id).await.expect("delete subscription");
    scope.finish().await.expect("cleanup failed");
}
