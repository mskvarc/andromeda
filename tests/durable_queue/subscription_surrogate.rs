//! The narrow key a delivery carries instead of a Subscription URI.
//!
//! Subscription surrogate key: Subscriptions with
//! many pending deliveries, then one Subscription deleted. Delivery rows
//! reference the narrow key, the cascade uses the delivery-side index rather
//! than a sequential scan, and external Subscription URNs render unchanged.

use crate::{
    notification_fixture::{entity_id, entity_snapshot, subscription_id},
    queue_database::{deliveries_of, queue_database, record_events, subscription, subscription_key},
};
use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
use andromeda_postgresql_context_store::notification_delivery::rows::NewNotificationEvent;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use chrono::Utc;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;

/// Subscriptions that hold the deliveries the cascade must not scan.
const OTHER_SUBSCRIPTIONS: i32 = 40;

/// Deliveries queued per unaffected Subscription.
const OTHER_DELIVERIES: i32 = 150;

/// Deliveries queued for the Subscription that is deleted.
const OWNED_DELIVERIES: i32 = 20;

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_deleting_a_subscription_reclaims_its_deliveries_through_the_delivery_side_index() {
    let pool = queue_database().await;
    let owner = subscription_id();
    let owner_key = subscription(&pool, owner.as_str()).await;
    for _ in 0..OTHER_SUBSCRIPTIONS {
        let identifier = subscription_id();
        subscription(&pool, identifier.as_str()).await;
    }

    let entity = entity_id();
    let snapshot = entity_snapshot(&entity);
    let record = WriteRecord::mint(Utc::now());
    record_events(
        &pool,
        &record,
        &[NewNotificationEvent {
            ordinal: WriteOrdinal::at(0),
            change: &EntityChange::new(EntityLifecycle::Created, Vec::new()),
            entity_id: &entity,
            change_attribute_name_keys: Vec::new(),
            entity_snapshot: Some(&snapshot),
            previous_snapshot: None,
            subscription_keys: vec![subscription_key(owner_key)],
        }],
    )
    .await
    .expect("the event the queued deliveries reference");
    let event: i64 = sqlx::query_scalar("SELECT event_key FROM notification_events WHERE write_id = $1")
        .bind(record.write_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("event identifier");

    // Production-shaped cardinality: the deleted Subscription owns a small
    // fraction of a queue dominated by deliveries that must survive. The live
    // branch's deliveries live in the default schema, so nothing names a branch.
    sqlx::query(
        "INSERT INTO notification_deliveries (event_key, subscription_key, next_attempt_at) \
         SELECT $1, s.subscription_key, NOW() FROM subscriptions s, generate_series(1, $2) WHERE s.subscription_key <> $3",
    )
    .bind(event)
    .bind(OTHER_DELIVERIES)
    .bind(owner_key)
    .execute(&pool)
    .await
    .expect("queue the unaffected deliveries");
    sqlx::query("INSERT INTO notification_deliveries (event_key, subscription_key, next_attempt_at) SELECT $1, $2, NOW() FROM generate_series(1, $3)")
        .bind(event)
        .bind(owner_key)
        .bind(OWNED_DELIVERIES - 1)
        .execute(&pool)
        .await
        .expect("queue the owned deliveries");
    sqlx::query("ANALYZE notification_deliveries")
        .execute(&pool)
        .await
        .expect("refresh the planner statistics");

    let column_type: String = sqlx::query_scalar(
        "SELECT data_type FROM information_schema.columns WHERE table_name = 'notification_deliveries' AND column_name = 'subscription_key'",
    )
    .fetch_one(&pool)
    .await
    .expect("delivery key column type");
    assert_eq!(column_type, "bigint", "a delivery references the narrow key, not the Subscription URI");

    let plan: Vec<String> = sqlx::query_scalar("EXPLAIN DELETE FROM notification_deliveries WHERE subscription_key = $1")
        .bind(owner_key)
        .fetch_all(&pool)
        .await
        .expect("the plan of the statement the cascade runs");
    let plan = plan.join("\n");
    assert!(
        plan.contains("idx_notification_deliveries_subscription"),
        "the cascade must reach its rows through the delivery-side index, plan was:\n{plan}"
    );
    assert!(
        !plan.contains("Seq Scan on notification_deliveries"),
        "the cascade must not scan the whole queue, plan was:\n{plan}"
    );

    assert_eq!(deliveries_of(&pool, owner_key).await, i64::from(OWNED_DELIVERIES));
    let survivors: i64 = sqlx::query_scalar("SELECT count(*) FROM notification_deliveries WHERE subscription_key <> $1")
        .bind(owner_key)
        .fetch_one(&pool)
        .await
        .expect("unaffected delivery count");
    assert_eq!(survivors, i64::from(OTHER_SUBSCRIPTIONS * OTHER_DELIVERIES));

    sqlx::query("DELETE FROM subscriptions WHERE id = $1")
        .bind(owner.as_str())
        .execute(&pool)
        .await
        .expect("delete the Subscription");
    assert_eq!(
        deliveries_of(&pool, owner_key).await,
        0,
        "deleting a Subscription reclaims its queued deliveries"
    );
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM notification_deliveries")
        .fetch_one(&pool)
        .await
        .expect("delivery count after the cascade");
    assert_eq!(
        after,
        i64::from(OTHER_SUBSCRIPTIONS * OTHER_DELIVERIES),
        "no other Subscription's deliveries are touched"
    );
    pool.close().await;
}

#[tokio::test]
async fn durable_queue_a_subscription_renders_the_external_urn_it_was_created_with() {
    let scope = TestEnvironment::from_suite("durablequeue").await.expect("isolated test environment");
    let identifier = scope.id("Subscription:surrogate");
    let payload = json!({
        "id": identifier,
        "type": "Subscription",
        "entities": [{"type": "Building"}],
        "notification": {
            "endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"}
        },
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = scope.client().create_subscription(&payload).await.expect("create subscription failed");
    assert_eq!(created.status, StatusCode::CREATED, "subscription create: {:?}", created.body);

    let retrieved = scope
        .client()
        .retrieve_subscription(&identifier, Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve subscription failed");
    assert_eq!(retrieved.status, StatusCode::OK);
    let body = retrieved.json_value().expect("subscription body");
    assert_eq!(
        body["id"],
        json!(identifier),
        "the surrogate key never replaces the identifier a client addresses"
    );

    let key: i64 = sqlx::query_scalar("SELECT subscription_key FROM subscriptions WHERE id = $1")
        .bind(&identifier)
        .fetch_one(scope.database_pool())
        .await
        .expect("stored surrogate key");
    assert!(key > 0, "every stored Subscription carries a surrogate key for its deliveries");

    scope.client().delete_subscription(&identifier).await.expect("delete subscription failed");
    scope.finish().await.expect("cleanup failed");
}
