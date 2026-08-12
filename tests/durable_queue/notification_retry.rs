//! Retry accounting and the terminal state of a notification delivery.
//!
//! Verification-matrix row "Outbox retry/dead state": delivery fails through
//! the configured attempt limit; attempts increase monotonically, the claim
//! fields clear on retry, the row stops being claimable at the limit, and the
//! schema's state constraints hold after every transition.

use crate::{
    notification_fixture::{delivery_ids, queue_notification},
    queue_database::queue_database,
    queue_workers::{delivery_worker, holding_lease},
};
use andromeda_notification::{delivery_entry::DeliveryId, delivery_store::DeliveryStore};
use andromeda_postgresql_context_store::notification_delivery::queue::NotificationDeliveryQueue;
use chrono::{Duration, Utc};
use pretty_assertions::assert_eq;
use serial_test::serial;
use sqlx::PgPool;

/// Deliveries claimed per pass, larger than any fixture in this module.
const BATCH_SIZE: i32 = 10;

/// The attempt ceiling the queue's partial claim index carries.
const MAX_ATTEMPTS: i32 = 5;

/// Attempts recorded against one delivery, and how many of the claim columns it holds.
async fn attempts_and_claim_columns(pool: &PgPool, delivery: i64) -> (i32, i32) {
    sqlx::query_as("SELECT attempt_count, num_nonnulls(claimed_at, claimed_by, lease_until) FROM notification_deliveries WHERE id = $1")
        .bind(delivery)
        .fetch_one(pool)
        .await
        .expect("delivery retry state")
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_delivery_stops_being_claimable_once_it_reaches_the_attempt_limit() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, 1).await;
    let delivery = *delivery_ids(&pool, &queued.record).await.first().expect("one queued delivery");
    let queue = NotificationDeliveryQueue::new(&pool);
    let worker = delivery_worker();

    for attempt in 1..=MAX_ATTEMPTS {
        let claimed = queue
            .claim_pending(&worker, holding_lease(), BATCH_SIZE, MAX_ATTEMPTS)
            .await
            .expect("claim before the limit");
        assert_eq!(claimed.len(), 1, "attempt {attempt} must still find the delivery claimable");
        assert_eq!(claimed[0].attempt_count, attempt - 1, "the claim reports the attempts already made");

        // The retry instant is placed in the past so the next pass is due
        // immediately: this verification is about the attempt ceiling, not
        // about the backoff schedule.
        let next_attempt_at = Utc::now() - Duration::seconds(1);
        queue
            .mark_failed(DeliveryId::from(delivery), &worker, "endpoint refused the notification", next_attempt_at)
            .await
            .expect("record the failed attempt");

        let (attempts, claim_columns) = attempts_and_claim_columns(&pool, delivery).await;
        assert_eq!(attempts, attempt, "attempts increase by exactly one per recorded failure");
        assert_eq!(claim_columns, 0, "a retry releases the whole claim triple rather than half of it");
    }

    let exhausted = queue
        .claim_pending(&worker, holding_lease(), BATCH_SIZE, MAX_ATTEMPTS)
        .await
        .expect("claim at the limit");
    assert!(exhausted.is_empty(), "a delivery at the attempt limit is no longer claimable");
    pool.close().await;
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_terminal_failure_retires_the_delivery_and_releases_its_lease() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, 1).await;
    let delivery = *delivery_ids(&pool, &queued.record).await.first().expect("one queued delivery");
    let queue = NotificationDeliveryQueue::new(&pool);
    let worker = delivery_worker();

    let claimed = queue
        .claim_pending(&worker, holding_lease(), BATCH_SIZE, MAX_ATTEMPTS)
        .await
        .expect("claim before the terminal failure");
    assert_eq!(claimed.len(), 1);

    queue
        .mark_terminal(DeliveryId::from(delivery), &worker, "the endpoint rejected the payload")
        .await
        .expect("record the terminal failure");

    let (attempts, claim_columns) = attempts_and_claim_columns(&pool, delivery).await;
    assert_eq!(attempts, i32::MAX, "exhaustion is expressed by passing every reachable attempt ceiling");
    assert_eq!(claim_columns, 0, "a retired delivery holds no lease");

    let retired = queue
        .claim_pending(&worker, holding_lease(), BATCH_SIZE, MAX_ATTEMPTS)
        .await
        .expect("claim after the terminal failure");
    assert!(retired.is_empty(), "a retired delivery is permanently unclaimable");

    let undelivered: i64 = sqlx::query_scalar("SELECT count(*) FROM notification_deliveries WHERE id = $1 AND delivered_at IS NULL")
        .bind(delivery)
        .fetch_one(&pool)
        .await
        .expect("retired delivery state");
    assert_eq!(undelivered, 1, "a retired delivery is retained as evidence rather than marked delivered");
    pool.close().await;
}
