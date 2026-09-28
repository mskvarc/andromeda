//! A delivery the drain can never deliver is retired, not leased forever.
//!
//! A claim leases a row before it reads the row back, so a row whose stored
//! event cannot be read is leased to the claiming worker all the same. Left
//! unacknowledged it would be claimed again at every lease expiry with its
//! attempt count unchanged, and the tenant's pending work would report it due
//! for as long as it existed. The drain retires it as exhausted with the reason
//! as its recorded failure, and the readable delivery claimed with it is still
//! delivered.

use crate::{
    notification_fixture::{delivery_ids, queue_notification},
    queue_database::queue_database,
};
use andromeda_notification::{
    delivery_config::DeliveryConfig,
    delivery_drain::{DeliveryDisposition, NotificationDeliveryDrain, NotificationDeliveryTransport},
    delivery_entry::{DeliveryEntry, DeliveryKey},
    delivery_store::DeliveryStore,
};
use andromeda_postgresql_context_store::{notification_delivery::queue::NotificationDeliveryQueue, pending_work::earliest_obligation};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use pretty_assertions::assert_eq;
use serial_test::serial;
use sqlx::PgPool;
use std::{
    sync::{Arc, Mutex},
    time::SystemTime,
};
use uuid::Uuid;

/// A stored entity identifier no clause 4.6.2 URI parse accepts.
const UNREADABLE_ENTITY_ID: &str = "not a uri";

/// A transport that accepts every delivery it is handed and remembers which.
#[derive(Default)]
struct AcceptingTransport {
    delivered: Mutex<Vec<DeliveryKey>>,
}

impl NotificationDeliveryTransport for AcceptingTransport {
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
        Box::pin(async move {
            self.delivered.lock().expect("delivered record lock").push(entry.key);
            DeliveryDisposition::Delivered
        })
    }
}

/// Queue one delivery, for `subscription_key`, of an event whose entity
/// identifier cannot be read back, and return the delivery's key.
async fn queue_unreadable_delivery(pool: &PgPool, subscription_key: i64) -> i64 {
    let event: i64 = sqlx::query_scalar(
        "INSERT INTO notification_events (created_at, event_ordinal, lifecycle, write_id, entity_id, change_kinds, change_attribute_name_keys, change_dataset_ids) \
         VALUES (NOW(), 0, 1, $1, $2, '{}', '{}', '{}') RETURNING event_key",
    )
    .bind(Uuid::new_v4())
    .bind(UNREADABLE_ENTITY_ID)
    .fetch_one(pool)
    .await
    .expect("queue the unreadable event");
    sqlx::query_scalar("INSERT INTO notification_deliveries (event_key, subscription_key, next_attempt_at) VALUES ($1, $2, NOW()) RETURNING delivery_key")
        .bind(event)
        .bind(subscription_key)
        .fetch_one(pool)
        .await
        .expect("queue the unreadable delivery")
}

/// The terminal state one delivery row holds: when it was delivered and
/// exhausted, the attempts counted, the claim columns still held, and the
/// recorded failure.
async fn delivery_state(pool: &PgPool, delivery: i64) -> (Option<DateTime<Utc>>, Option<DateTime<Utc>>, i32, i32, Option<String>) {
    sqlx::query_as(
        "SELECT delivered_at, exhausted_at, attempt_count, num_nonnulls(claimed_at, claimed_by, lease_until), last_error \
         FROM notification_deliveries WHERE delivery_key = $1",
    )
    .bind(delivery)
    .fetch_one(pool)
    .await
    .expect("delivery state")
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_an_unreadable_delivery_is_dead_lettered_without_holding_back_its_batch() {
    let pool = queue_database().await;
    let queued = queue_notification(&pool, 1).await;
    let readable = *delivery_ids(&pool, &queued.record).await.first().expect("one readable delivery");
    let subscription_key = *queued.subscription_keys.first().expect("one subscription");
    let unreadable = queue_unreadable_delivery(&pool, subscription_key).await;

    let transport = Arc::new(AcceptingTransport::default());
    let drain = NotificationDeliveryDrain::new(
        Arc::new(NotificationDeliveryQueue::new(&pool)) as Arc<dyn DeliveryStore>,
        Arc::clone(&transport) as Arc<dyn NotificationDeliveryTransport>,
        &DeliveryConfig::default(),
    )
    .expect("the default delivery configuration composes a drain");

    let first = drain.execute(Utc::now()).await.expect("a pass over a batch holding an unreadable row succeeds");
    assert_eq!(first.claimed, 2, "the unreadable row is claimed alongside the readable one");
    assert_eq!(first.lease_lost, 0);
    assert_eq!(
        transport.delivered.lock().expect("delivered record lock").as_slice(),
        &[DeliveryKey::from(readable)],
        "only the readable delivery reaches the transport"
    );

    let (delivered_at, exhausted_at, attempts, claim_columns, last_error) = delivery_state(&pool, readable).await;
    assert!(delivered_at.is_some(), "the readable delivery is acknowledged as delivered");
    assert!(exhausted_at.is_none());
    assert_eq!((attempts, claim_columns, last_error), (0, 3, None), "a delivered row keeps its claim triple");

    let (delivered_at, exhausted_at, attempts, claim_columns, last_error) = delivery_state(&pool, unreadable).await;
    assert!(delivered_at.is_none(), "an unreadable row is not reported delivered");
    assert!(exhausted_at.is_some(), "an unreadable row is dead-lettered");
    assert_eq!(attempts, 1, "the retiring acknowledgement is counted");
    assert_eq!(claim_columns, 0, "a dead-lettered row holds no lease");
    let last_error = last_error.expect("the dead-lettered row records why it could not be read");
    assert!(
        last_error.contains(&format!("notification delivery {unreadable}")) && last_error.contains(UNREADABLE_ENTITY_ID),
        "{last_error}"
    );

    let second = drain.execute(Utc::now()).await.expect("a second pass");
    assert_eq!(second.claimed, 0, "a retired row is not claimed again");
    let due = earliest_obligation(&pool).await.expect("the tenant's pending work");
    assert!(
        due.is_none_or(|instant| instant > SystemTime::now()),
        "a retired row no longer reports the tenant due: {due:?}"
    );
    pool.close().await;
}
