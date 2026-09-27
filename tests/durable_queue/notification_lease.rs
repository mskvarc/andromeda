//! Exclusive ownership of a due notification delivery.
//!
//! Outbox exclusive lease: two workers run against
//! one due row; exactly one active lease is returned, the second worker
//! receives none until expiration, and a stale worker cannot deliver or
//! acknowledge after another worker reclaims the row.

use crate::{
    notification_fixture::{delivery_ids, queue_notification, readable},
    queue_database::queue_database,
    queue_workers::{delivery_worker, expiring_lease, holding_lease},
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_notification::{delivery_entry::DeliveryKey, delivery_lease::DeliveryLeaseError, delivery_store::DeliveryStore};
use andromeda_postgresql_context_store::notification_delivery::queue::NotificationDeliveryQueue;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use serial_test::serial;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;
use uuid::Uuid;

/// Deliveries claimed per pass, larger than any fixture in this module.
const BATCH_SIZE: i32 = 10;

/// The claim triple one delivery row currently holds.
async fn claim_triple(pool: &PgPool, delivery: i64) -> (Option<DateTime<Utc>>, Option<Uuid>, Option<DateTime<Utc>>) {
    sqlx::query_as("SELECT claimed_at, claimed_by, lease_until FROM notification_deliveries WHERE delivery_key = $1")
        .bind(delivery)
        .fetch_one(pool)
        .await
        .expect("claim triple")
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_due_delivery_is_leased_to_exactly_one_worker() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, 1).await;
    let delivery = *delivery_ids(&pool, &queued.record).await.first().expect("one queued delivery");
    let queue = NotificationDeliveryQueue::new(&pool);

    let first = delivery_worker();
    let second = delivery_worker();
    let claimed = readable(queue.claim_pending(&first, holding_lease(), BATCH_SIZE).await.expect("first claim"));
    assert_eq!(claimed.len(), 1, "the due delivery must be leased to the first worker");
    assert_eq!(claimed[0].key, DeliveryKey::from(delivery));

    let contended = readable(queue.claim_pending(&second, holding_lease(), BATCH_SIZE).await.expect("second claim"));
    assert!(contended.is_empty(), "a live lease must exclude every other worker");

    let (claimed_at, claimed_by, lease_until) = claim_triple(&pool, delivery).await;
    assert_eq!(claimed_by, Some(first.as_uuid()));
    assert!(claimed_at.is_some(), "a claim records when it was taken");
    assert!(lease_until.is_some_and(|until| until > Utc::now()), "a claim takes a live lease");

    queue
        .mark_delivered(DeliveryKey::from(delivery), &first, &ContextTarget::Live)
        .await
        .expect("the lease holder acknowledges");
    pool.close().await;
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_stale_worker_cannot_acknowledge_a_reclaimed_delivery() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, 1).await;
    let delivery = *delivery_ids(&pool, &queued.record).await.first().expect("one queued delivery");
    let queue = NotificationDeliveryQueue::new(&pool);

    let stale = delivery_worker();
    let claimed = readable(queue.claim_pending(&stale, expiring_lease(), BATCH_SIZE).await.expect("first claim"));
    assert_eq!(claimed.len(), 1);

    // The lease is the shortest the term type accepts, so waiting past it is
    // the same expiry a crashed worker's row goes through.
    sleep(Duration::from_millis(1_500)).await;

    let reclaiming = delivery_worker();
    let reclaimed = readable(
        queue
            .claim_pending(&reclaiming, holding_lease(), BATCH_SIZE)
            .await
            .expect("reclaim after expiry"),
    );
    assert_eq!(reclaimed.len(), 1, "an expired lease makes the delivery claimable again");
    assert_eq!(reclaimed[0].key, DeliveryKey::from(delivery));

    assert_lease_lost(
        queue.mark_delivered(DeliveryKey::from(delivery), &stale, &ContextTarget::Live).await,
        delivery,
        &stale,
    );
    assert_lease_lost(
        queue
            .mark_failed(DeliveryKey::from(delivery), &stale, "stale", Utc::now(), &ContextTarget::Live)
            .await,
        delivery,
        &stale,
    );
    assert_lease_lost(
        queue.mark_terminal(DeliveryKey::from(delivery), &stale, "stale", &ContextTarget::Live).await,
        delivery,
        &stale,
    );

    let (_, claimed_by, _) = claim_triple(&pool, delivery).await;
    assert_eq!(
        claimed_by,
        Some(reclaiming.as_uuid()),
        "the stale worker must not overwrite the reclaiming worker's ownership"
    );

    queue
        .mark_delivered(DeliveryKey::from(delivery), &reclaiming, &ContextTarget::Live)
        .await
        .expect("the reclaiming worker acknowledges");
    let delivered: Option<DateTime<Utc>> = sqlx::query_scalar("SELECT delivered_at FROM notification_deliveries WHERE delivery_key = $1")
        .bind(delivery)
        .fetch_one(&pool)
        .await
        .expect("delivery acknowledgement");
    assert!(delivered.is_some(), "the reclaiming worker's acknowledgement applies");
    pool.close().await;
}

/// Assert that an acknowledgement was refused because the lease had moved on.
fn assert_lease_lost(outcome: Result<(), DeliveryLeaseError>, delivery: i64, worker: &LeaseHolder) {
    let error = outcome.expect_err("a worker whose lease expired must not acknowledge the delivery");
    assert!(
        matches!(error, DeliveryLeaseError::LeaseLost { .. }),
        "the acknowledgement is refused for a lost lease, not a statement failure: {error:?}"
    );
    if let DeliveryLeaseError::LeaseLost {
        delivery: named,
        worker: owner,
    } = error
    {
        assert_eq!(named, DeliveryKey::from(delivery));
        assert_eq!(&owner, worker);
    }
}
