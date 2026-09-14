//! Reaping deliveries and the shared event they reference.
//!
//! Verification-matrix row "Notification event cleanup": completed, obsolete,
//! retry-exhausted and retained deliveries share one event, and each cleanup
//! horizon is advanced in turn. The event remains while any retained delivery
//! references it and is removed only once every delivery can be removed; no
//! delivery loses its payload and no unreferenced event survives cleanup.

use crate::{
    notification_fixture::{delivery_ids, queue_notification},
    queue_database::{deliveries_of, queue_database},
    queue_workers::{delivery_worker, holding_lease},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_notification::{delivery_entry::DeliveryId, delivery_store::DeliveryStore};
use andromeda_postgresql_context_store::notification_delivery::queue::NotificationDeliveryQueue;
use chrono::{Duration, Utc};
use pretty_assertions::assert_eq;
use serde_json::Value;
use serial_test::serial;
use sqlx::PgPool;

/// The attempt ceiling the delivery worker is configured with; the queue does
/// not know it.
const MAX_ATTEMPTS: i32 = 5;

/// How many events the queue currently holds.
async fn stored_events(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notification_events")
        .fetch_one(pool)
        .await
        .expect("stored event count")
}

/// How many deliveries the queue currently holds.
async fn stored_deliveries(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM notification_deliveries")
        .fetch_one(pool)
        .await
        .expect("stored delivery count")
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_shared_event_outlives_every_delivery_that_still_references_it() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, 4).await;
    let deliveries = delivery_ids(&pool, &queued.record).await;
    assert_eq!(deliveries.len(), 4, "one event owes one delivery per matching Subscription");
    for subscription_key in &queued.subscription_keys {
        assert_eq!(
            deliveries_of(&pool, *subscription_key).await,
            1,
            "each matching Subscription owes exactly one delivery"
        );
    }
    let queue = NotificationDeliveryQueue::new(&pool);
    let worker = delivery_worker();

    let claimed = queue.claim_pending(&worker, holding_lease(), 10).await.expect("claim the fan-out");
    assert_eq!(claimed.len(), 4);
    // clone: the payload is compared again after the claimed batch is consumed.
    let payload = claimed.first().expect("a claimed delivery").event.entity_snapshot.clone();

    // Four dispositions over one shared event: delivered, obsolete (which the
    // drain acknowledges the same way), retry-exhausted, and retained pending.
    queue
        .mark_delivered(DeliveryId::from(deliveries[0]), &worker, &ContextTarget::Live)
        .await
        .expect("completed delivery");
    queue
        .mark_delivered(DeliveryId::from(deliveries[1]), &worker, &ContextTarget::Live)
        .await
        .expect("obsolete delivery");
    queue
        .mark_terminal(
            DeliveryId::from(deliveries[2]),
            &worker,
            "the endpoint rejected the payload",
            &ContextTarget::Live,
        )
        .await
        .expect("retry-exhausted delivery");
    queue
        .mark_failed(
            DeliveryId::from(deliveries[3]),
            &worker,
            "endpoint unavailable",
            Utc::now() + Duration::hours(1),
            &ContextTarget::Live,
        )
        .await
        .expect("retained delivery");

    let removed = queue.delete_delivered_before(Utc::now() + Duration::minutes(1)).await.expect("first reap");
    assert_eq!(removed, 2, "only the acknowledged deliveries are past the horizon");
    assert_eq!(stored_deliveries(&pool).await, 2, "the exhausted and retained deliveries stay");
    assert_eq!(stored_events(&pool).await, 1, "the event survives while any delivery references it");
    let retained: Option<Value> = sqlx::query_scalar("SELECT entity_snapshot FROM notification_events")
        .fetch_one(&pool)
        .await
        .expect("retained payload");
    assert_eq!(retained, payload, "no surviving delivery loses its payload");

    // Dead-letter retention removes the exhausted delivery kept as evidence;
    // the delivered-row reap never touches an undelivered row.
    let dead_lettered = queue
        .delete_exhausted_before(Utc::now() + Duration::minutes(1))
        .await
        .expect("dead-letter reap");
    assert_eq!(dead_lettered, 1, "only the exhausted delivery is past the dead-letter horizon");
    assert_eq!(stored_deliveries(&pool).await, 1, "the retained delivery is untouched by dead-letter retention");
    assert_eq!(stored_events(&pool).await, 1, "an event referenced by the retained delivery still survives");

    // The orphan-event pass does not touch this event either: the retained
    // delivery still references it, so it is not orphaned yet.
    let orphaned = queue
        .delete_orphan_events_before(Utc::now() + Duration::minutes(1))
        .await
        .expect("orphan-event reap");
    assert_eq!(orphaned, 0, "an event a live delivery still references is not orphaned");

    sqlx::query("UPDATE notification_deliveries SET next_attempt_at = NOW() - INTERVAL '1 second' WHERE id = $1")
        .bind(deliveries[3])
        .execute(&pool)
        .await
        .expect("make the retained delivery due");
    let last = queue.claim_pending(&worker, holding_lease(), 10).await.expect("claim the last delivery");
    assert_eq!(last.len(), 1);
    assert_eq!(
        last.first().expect("the last delivery").event.entity_snapshot,
        payload,
        "the last delivery still carries the shared payload"
    );
    queue
        .mark_delivered(DeliveryId::from(deliveries[3]), &worker, &ContextTarget::Live)
        .await
        .expect("last delivery");

    let final_removal = queue.delete_delivered_before(Utc::now() + Duration::minutes(1)).await.expect("final reap");
    assert_eq!(final_removal, 1);
    assert_eq!(stored_deliveries(&pool).await, 0, "every delivery is past its horizon");
    assert_eq!(stored_events(&pool).await, 0, "no unreferenced event survives cleanup");
    pool.close().await;
}

/// A delivery that fails one ordinary failure at a time is exhausted by the
/// worker on the attempt that reaches [`MAX_ATTEMPTS`]: the drain acknowledges
/// that last failure as terminal, which stamps `exhausted_at` exactly as a
/// permanent transport failure would. The dead-letter reap keys on that stamp
/// alone, so both routes to exhaustion are reaped by the same pass.
#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_delivery_exhausted_by_repeated_retries_is_reaped() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, 1).await;
    let deliveries = delivery_ids(&pool, &queued.record).await;
    assert_eq!(deliveries.len(), 1, "one Subscription owes exactly one delivery");
    let queue = NotificationDeliveryQueue::new(&pool);
    let worker = delivery_worker();

    for attempt in 0..MAX_ATTEMPTS {
        let claimed = queue
            .claim_pending(&worker, holding_lease(), 10)
            .await
            .unwrap_or_else(|_| panic!("claim attempt {attempt}"));
        assert_eq!(claimed.len(), 1, "the delivery is still below the attempt ceiling on attempt {attempt}");
        // The worker's rule: a retryable failure below the ceiling is
        // rescheduled, the one that reaches it is acknowledged as terminal.
        if attempt + 1 < MAX_ATTEMPTS {
            queue
                .mark_failed(
                    DeliveryId::from(deliveries[0]),
                    &worker,
                    "endpoint unavailable",
                    Utc::now() - Duration::seconds(1),
                    &ContextTarget::Live,
                )
                .await
                .unwrap_or_else(|_| panic!("retry failure {attempt}"));
        } else {
            queue
                .mark_terminal(DeliveryId::from(deliveries[0]), &worker, "endpoint unavailable", &ContextTarget::Live)
                .await
                .unwrap_or_else(|_| panic!("exhausting failure {attempt}"));
        }
    }

    let claimed_after_ceiling = queue.claim_pending(&worker, holding_lease(), 10).await.expect("claim past the ceiling");
    assert!(
        claimed_after_ceiling.is_empty(),
        "a delivery exhausted at the attempt ceiling is permanently unclaimable, exactly like a terminally failed one"
    );

    let dead_lettered = queue
        .delete_exhausted_before(Utc::now() + Duration::minutes(1))
        .await
        .expect("dead-letter reap");
    assert_eq!(
        dead_lettered, 1,
        "the retry-exhausted delivery is reaped by the same pass as a terminally failed one"
    );
    assert_eq!(stored_deliveries(&pool).await, 0, "no delivery survives dead-letter retention");

    let orphaned = queue
        .delete_orphan_events_before(Utc::now() + Duration::minutes(1))
        .await
        .expect("orphan-event reap");
    assert_eq!(orphaned, 1, "the event the reaped delivery released is picked up by the orphan pass");
    assert_eq!(stored_events(&pool).await, 0, "no unreferenced event survives cleanup");
    pool.close().await;
}
