//! A queued delivery whose Subscription the store cannot read back stays owed.
//!
//! Before it sends a queued notification the drain re-reads the Subscription
//! the delivery is due to, so the change is matched against the Subscription
//! as it now stands. A store that cannot answer is not a Subscription that is
//! gone: acknowledging the delivery as obsolete would lose a notification
//! clause 5.8.6 owes the subscriber. The delivery is released for a retry with
//! the failure recorded against it, and no notification was attempted, so
//! the Subscription's clause 5.2.14.2 `timesSent` does not move.
//!
//! The store is made unable to answer deterministically, at the database: the
//! Subscription's stored `jsonldContext` is overwritten with a value that is
//! not a URL, which the store refuses to read back as a Subscription. That
//! runs the broker's real drain and store over a real row, and it touches one
//! row of a tenant of the test's own, so nothing else can meet the failure.
//! Restoring the value then lets the retry deliver.

use crate::{
    live_delivery_drain::{
        ACKNOWLEDGEMENT_DEADLINE,
        DRAIN_DEADLINE,
        PROBE_TYPE,
        QueuedEvent,
        create_probe,
        queue_delivery,
        settled_delivery,
        subscribe_to_created_probe,
    },
    notification_adapter::NotificationAdapter,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::{
    configuration::TestRunConfiguration,
    tenant_provisioning::registered_tenant_database,
    tenant_scoped_environment::TenantScopedEnvironment,
};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::{Instant, sleep};

/// A stored `jsonldContext` the store refuses to read back.
const UNREADABLE_CONTEXT: &str = "not a url";

/// How long an unexpected notification is waited for.
const ABSENT: Duration = Duration::from_secs(2);

/// A pool on the database of the private tenant `tenant`, which exists once a
/// write created it.
async fn private_tenant_database(tenant: &str) -> PgPool {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let control = PgPool::connect(configuration.database().url().as_str())
        .await
        .expect("control database connection");
    let tenant: TenantName = tenant.parse().expect("the private tenant has a valid name");
    let database = registered_tenant_database(&control, &tenant)
        .await
        .expect("the tenant registry is readable")
        .expect("the written tenant is registered");
    control.close().await;
    PgPool::connect(configuration.database().with_database(database.as_str()).url().as_str())
        .await
        .expect("tenant database connection")
}

/// A request on the tenant that reads no Subscription, so the tenant's pacer
/// reads the seeded delivery on the drain's next fallback tick instead of
/// resting on a verdict taken before it existed.
async fn touch_tenant(client: &TestClient, entity_id: &str) {
    let read = client.get_entity(entity_id, "", "application/json", None).await.expect("read the probe entity");
    assert_eq!(read.status, StatusCode::OK, "the probe entity is readable");
}

/// Wait until the delivery's row records its first attempt or an outcome, and
/// return whether it was delivered and exhausted, its attempts, and its
/// recorded failure.
async fn first_attempt(pool: &PgPool, delivery: i64) -> (bool, bool, i32, Option<String>) {
    let give_up = Instant::now() + DRAIN_DEADLINE;
    loop {
        let state: (bool, bool, i32, Option<String>) = sqlx::query_as(
            "SELECT delivered_at IS NOT NULL, exhausted_at IS NOT NULL, attempt_count, last_error FROM notification_deliveries WHERE delivery_key = $1",
        )
        .bind(delivery)
        .fetch_one(pool)
        .await
        .expect("delivery state");
        if state.0 || state.1 || state.2 > 0 || Instant::now() >= give_up {
            return state;
        }
        sleep(Duration::from_millis(250)).await;
    }
}

/// How many notifications the Subscription's clause 5.2.14.2 `timesSent` counts.
async fn times_sent(pool: &PgPool, subscription_key: i64) -> i32 {
    sqlx::query_scalar("SELECT COALESCE((SELECT times_sent FROM subscription_delivery_state WHERE subscription_key = $1), 0)")
        .bind(subscription_key)
        .fetch_one(pool)
        .await
        .expect("delivery bookkeeping")
}

#[tokio::test]
#[serial(broker_delivery)]
async fn durable_queue_a_delivery_whose_subscription_cannot_be_read_stays_pending_and_is_retried() {
    let scope = TenantScopedEnvironment::from_suite("durablequeue").await.expect("private tenant environment");
    let receiver = NotificationAdapter::start().await;
    let entity_id = scope.id("LiveDrainProbe:unreadable-subscription");
    let subscription_id = scope.id("Subscription:unreadable");
    create_probe(scope.client(), &entity_id).await;
    let pool = private_tenant_database(scope.tenant()).await;
    let subscription_key = subscribe_to_created_probe(scope.client(), &pool, &entity_id, &subscription_id, &receiver.endpoint_url()).await;
    receiver.clear_notifications().await;

    let stored_context: Option<String> = sqlx::query_scalar("SELECT jsonld_context FROM subscriptions WHERE subscription_key = $1")
        .bind(subscription_key)
        .fetch_one(&pool)
        .await
        .expect("the stored context");
    sqlx::query("UPDATE subscriptions SET jsonld_context = $2 WHERE subscription_key = $1")
        .bind(subscription_key)
        .bind(UNREADABLE_CONTEXT)
        .execute(&pool)
        .await
        .expect("make the subscription unreadable");
    let delivery = queue_delivery(
        &pool,
        subscription_key,
        QueuedEvent {
            lifecycle: 1,
            entity_id: &entity_id,
            change: Vec::new(),
            entity_snapshot: json!([{"@id": entity_id, "@type": [PROBE_TYPE]}]),
        },
    )
    .await;
    touch_tenant(scope.client(), &entity_id).await;

    let (delivered, exhausted, attempts, last_error) = first_attempt(&pool, delivery).await;
    assert!(
        !delivered,
        "a delivery whose subscription could not be read is not acknowledged as sent or obsolete"
    );
    assert!(!exhausted, "the delivery is retried, not dead-lettered: {last_error:?}");
    assert_eq!(attempts, 1, "the failed attempt is counted");
    let last_error = last_error.expect("the failed attempt records why");
    assert!(
        last_error.contains(&subscription_id) && last_error.contains("could not be read back"),
        "{last_error}"
    );
    receiver.wait_for_no_subscription_notification(&subscription_id, ABSENT).await;
    assert_eq!(times_sent(&pool, subscription_key).await, 0, "no notification was attempted");

    sqlx::query("UPDATE subscriptions SET jsonld_context = $2 WHERE subscription_key = $1")
        .bind(subscription_key)
        .bind(stored_context)
        .execute(&pool)
        .await
        .expect("make the subscription readable again");
    touch_tenant(scope.client(), &entity_id).await;
    let notification = receiver.wait_for_subscription_notification(&subscription_id, DRAIN_DEADLINE).await;
    assert_eq!(notification.body["data"][0]["id"], json!(entity_id), "the retry delivers the owed notification");
    let (delivered, _, _, _) = settled_delivery(&pool, delivery, ACKNOWLEDGEMENT_DEADLINE).await;
    assert!(delivered, "the retried delivery is acknowledged");

    receiver.shutdown().await;
    pool.close().await;
    scope.finish().await.expect("cleanup failed");
}
