//! One immutable event per changed Entity, one delivery per matching Subscription.
//!
//! Notification event fan-out: a commit matching
//! zero, one and one hundred Subscriptions, and a batch commit affecting
//! several Entities. Zero matches create no event; positive fan-out creates one
//! immutable event per affected Entity and exactly one delivery per matching
//! Subscription; `(write_id, event_ordinal)` is unique across the batch and
//! makes a replay of the same commit idempotent rather than duplicating what it
//! already announced; claims update only the narrow delivery rows; and the
//! stored bytes show the single-payload saving the split exists for.

use crate::{
    notification_fixture::{decoded, entity_document, entity_id, entity_snapshot, readable, subscription_id},
    queue_database::{attribute_name, deliveries_of, delivery_queue, events_of, queue_database, record_events, subscription, subscription_key},
    queue_workers::{delivery_worker, holding_lease},
};
use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_entity::{
    entity_state::EntityState,
    reads::{
        attribute_change::{AttributeChange, AttributeChangeKind},
        attribute_instance_key::AttributeInstanceKey,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
use andromeda_notification::{delivery_store::DeliveryStore, held_events::HeldEvents};
use andromeda_postgresql_context_store::notification_delivery::rows::NewNotificationEvent;
use chrono::Utc;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::PgPool;
use uuid::Uuid;

/// Subscriptions in the high-fan-out fixture.
const HIGH_FAN_OUT: usize = 100;

/// An Entity snapshot large enough that repeating it per subscriber would dominate the queue.
fn large_snapshot(entity: &Identifier) -> EntityState {
    let members: Vec<Value> = (0..2_000).map(|index| json!({"@value": format!("{index}-{}", Uuid::new_v4())})).collect();
    let mut document = entity_document(entity);
    document[0]["https://uri.etsi.org/ngsi-ld/default-context/readings"] = json!([{
        "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
        "https://uri.etsi.org/ngsi-ld/hasValue": members
    }]);
    decoded(&document)
}

/// Stored bytes of the one event a write left behind.
async fn event_bytes(pool: &PgPool, record: &WriteRecord) -> i64 {
    sqlx::query_scalar("SELECT sum(pg_column_size(e.*))::bigint FROM notification_events e WHERE e.write_id = $1")
        .bind(record.write_id().as_uuid())
        .fetch_one(pool)
        .await
        .expect("event bytes")
}

/// Stored bytes of every delivery a write's events owe.
async fn delivery_bytes(pool: &PgPool, record: &WriteRecord) -> i64 {
    sqlx::query_scalar(
        "SELECT sum(pg_column_size(d.*))::bigint FROM notification_deliveries d JOIN notification_events e ON e.event_key = d.event_key WHERE e.write_id = $1",
    )
    .bind(record.write_id().as_uuid())
    .fetch_one(pool)
    .await
    .expect("delivery bytes")
}

/// The change that updated `temperature` and `speed`.
fn updated_temperature_and_speed() -> EntityChange {
    let updated = |attribute: &str| AttributeChange {
        key: AttributeInstanceKey {
            attr: attribute.parse().expect("a valid attribute name"),
            dataset_id: None,
        },
        kind: AttributeChangeKind::Updated,
    };
    EntityChange::new(
        EntityLifecycle::Modified,
        vec![
            updated("https://uri.etsi.org/ngsi-ld/default-context/temperature"),
            updated("https://uri.etsi.org/ngsi-ld/default-context/speed"),
        ],
    )
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_change_no_subscription_matched_persists_nothing() {
    let pool = queue_database().await;
    let record = WriteRecord::mint(Utc::now());

    // An event no delivery would reference is dropped before the statement
    // runs, so a write that matched nothing hands the queue no events at all.
    record_events(&pool, &record, &[]).await.expect("a write that matched no subscription");

    assert_eq!(events_of(&pool, &record).await, 0, "an unmatched change stores no payload");
    let deliveries: i64 = sqlx::query_scalar("SELECT count(*) FROM notification_deliveries")
        .fetch_one(&pool)
        .await
        .expect("delivery count");
    assert_eq!(deliveries, 0, "an unmatched change owes no delivery");
    pool.close().await;
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_one_event_owes_exactly_one_delivery_per_matching_subscription() {
    let pool = queue_database().await;
    let entity = entity_id();
    let snapshot = large_snapshot(&entity);
    let temperature = attribute_name(&pool, "https://uri.etsi.org/ngsi-ld/default-context/temperature").await;
    let speed = attribute_name(&pool, "https://uri.etsi.org/ngsi-ld/default-context/speed").await;
    let updated = updated_temperature_and_speed();

    let single = subscription_id();
    let single_key = subscription(&pool, single.as_str()).await;
    let one_subscriber = WriteRecord::mint(Utc::now());
    record_events(
        &pool,
        &one_subscriber,
        &[NewNotificationEvent {
            ordinal: WriteOrdinal::at(0),
            change: &updated,
            entity_id: &entity,
            change_attribute_name_keys: vec![temperature, speed],
            entity_snapshot: Some(&snapshot),
            previous_snapshot: None,
            sys_attrs_snapshot: None,
            subscription_keys: vec![subscription_key(single_key)],
        }],
    )
    .await
    .expect("the single-subscriber write");

    assert_eq!(events_of(&pool, &one_subscriber).await, 1, "one affected Entity produces one event");
    assert_eq!(deliveries_of(&pool, single_key).await, 1, "one matching Subscription owes one delivery");

    let many: Vec<Identifier> = (0..HIGH_FAN_OUT).map(|_| subscription_id()).collect();
    let mut many_keys = Vec::with_capacity(HIGH_FAN_OUT);
    for identifier in &many {
        many_keys.push(subscription(&pool, identifier.as_str()).await);
    }
    let high_fan_out = WriteRecord::mint(Utc::now());
    record_events(
        &pool,
        &high_fan_out,
        &[NewNotificationEvent {
            ordinal: WriteOrdinal::at(0),
            change: &updated,
            entity_id: &entity,
            change_attribute_name_keys: vec![temperature, speed],
            entity_snapshot: Some(&snapshot),
            previous_snapshot: None,
            sys_attrs_snapshot: None,
            subscription_keys: many_keys.iter().map(|key| subscription_key(*key)).collect(),
        }],
    )
    .await
    .expect("the high-fan-out write");

    assert_eq!(events_of(&pool, &high_fan_out).await, 1, "a hundred subscribers still store one payload");
    let owed: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT d.subscription_key) FROM notification_deliveries d JOIN notification_events e ON e.event_key = d.event_key WHERE e.write_id = $1",
    )
    .bind(high_fan_out.write_id().as_uuid())
    .fetch_one(&pool)
    .await
    .expect("distinct subscribers owed a delivery");
    assert_eq!(
        owed,
        i64::try_from(HIGH_FAN_OUT).expect("the fan-out size fits in i64"),
        "every matching Subscription owes exactly one delivery"
    );

    // The saving the split exists for, stated as a measurement the test can
    // fail on: a hundred narrow delivery rows together stay smaller than the
    // single payload that would otherwise be copied into each of them.
    let payload = event_bytes(&pool, &high_fan_out).await;
    let queue_state = delivery_bytes(&pool, &high_fan_out).await;
    assert!(
        queue_state < payload,
        "a hundred delivery rows ({queue_state} bytes) must cost less than one stored payload ({payload} bytes)"
    );

    let queue = delivery_queue(&pool);
    let worker = delivery_worker();
    let claimed = readable(
        queue
            .claim_pending(&worker, holding_lease(), 200, &HeldEvents::default())
            .await
            .expect("claim the whole fan-out"),
    );
    assert_eq!(claimed.len(), HIGH_FAN_OUT + 1, "every owed delivery is claimable");
    let rendered = claimed.first().expect("a claimed delivery");
    assert_eq!(
        rendered.event.change.attributes().into_iter().map(AttributeName::as_str).collect::<Vec<&str>>(),
        vec![
            "https://uri.etsi.org/ngsi-ld/default-context/temperature",
            "https://uri.etsi.org/ngsi-ld/default-context/speed"
        ],
        "the dictionary keys resolve back in the order the event recorded them"
    );
    assert_eq!(rendered.event.change, updated, "the stored change reads back instance for instance");
    assert_eq!(
        rendered.event.entity_snapshot.as_deref(),
        Some(&snapshot),
        "delivery rendering carries the stored payload unchanged"
    );
    pool.close().await;
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_a_batch_write_numbers_its_events_uniquely_and_absorbs_a_replay() {
    let pool = queue_database().await;
    let first = subscription_id();
    let second = subscription_id();
    let first_key = subscription(&pool, first.as_str()).await;
    let second_key = subscription(&pool, second.as_str()).await;

    let entities: Vec<Identifier> = (0..3).map(|_| entity_id()).collect();
    let snapshots: Vec<EntityState> = entities.iter().map(entity_snapshot).collect();
    let record = WriteRecord::mint(Utc::now());
    let change = EntityChange::new(EntityLifecycle::Modified, Vec::new());
    let events: Vec<NewNotificationEvent<'_>> = entities
        .iter()
        .zip(snapshots.iter())
        .enumerate()
        .map(|(index, (entity, snapshot))| NewNotificationEvent {
            ordinal: WriteOrdinal::at(u32::try_from(index).expect("a representable ordinal")),
            change: &change,
            entity_id: entity,
            change_attribute_name_keys: Vec::new(),
            entity_snapshot: Some(snapshot),
            previous_snapshot: None,
            sys_attrs_snapshot: None,
            subscription_keys: vec![subscription_key(first_key), subscription_key(second_key)],
        })
        .collect();
    record_events(&pool, &record, &events).await.expect("the batch write");

    assert_eq!(events_of(&pool, &record).await, 3, "each affected Entity produces its own event");
    assert_eq!(deliveries_of(&pool, first_key).await, 3);
    assert_eq!(deliveries_of(&pool, second_key).await, 3);
    let ordinals: Vec<i32> = sqlx::query_scalar("SELECT event_ordinal FROM notification_events WHERE write_id = $1 ORDER BY event_ordinal")
        .bind(record.write_id().as_uuid())
        .fetch_all(&pool)
        .await
        .expect("event ordinals");
    assert_eq!(ordinals, vec![0, 1, 2], "the ordinals are unique and deterministic across the batch");

    // A commit replayed after a crash re-runs the same statement under the
    // write identity it was durably enqueued with. The replay has to succeed,
    // since a refusal would strand the crashed write's queue entry forever, and it
    // has to announce nothing a second time, which the write identity's own
    // uniqueness constraint is what settles.
    record_events(&pool, &record, &events).await.expect("a replayed write is accepted");
    assert_eq!(
        events_of(&pool, &record).await,
        3,
        "the replay writes no second copy of an event it already wrote"
    );
    assert_eq!(deliveries_of(&pool, first_key).await, 3, "the replay owes no second delivery");
    assert_eq!(deliveries_of(&pool, second_key).await, 3, "no subscriber is told the same change twice");
    pool.close().await;
}

#[tokio::test]
#[serial(durable_queue)]
async fn durable_queue_claiming_a_delivery_leaves_its_event_untouched() {
    let pool = queue_database().await;
    let entity = entity_id();
    let snapshot = entity_snapshot(&entity);
    let subscriber = subscription_id();
    let subscriber_key = subscription(&pool, subscriber.as_str()).await;
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
            sys_attrs_snapshot: None,
            subscription_keys: vec![subscription_key(subscriber_key)],
        }],
    )
    .await
    .expect("the write");

    // `xmin` is the transaction that produced the event's current row version:
    // a claim that rewrote the event, rather than only its delivery, would
    // change it.
    let before: (String, String) = sqlx::query_as("SELECT xmin::text, entity_snapshot::text FROM notification_events WHERE write_id = $1")
        .bind(record.write_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("event row version");

    let queue = delivery_queue(&pool);
    let worker = delivery_worker();
    let claimed = readable(queue.claim_pending(&worker, holding_lease(), 10, &HeldEvents::default()).await.expect("claim"));
    assert_eq!(claimed.len(), 1);

    let after: (String, String) = sqlx::query_as("SELECT xmin::text, entity_snapshot::text FROM notification_events WHERE write_id = $1")
        .bind(record.write_id().as_uuid())
        .fetch_one(&pool)
        .await
        .expect("event row version after the claim");
    assert_eq!(after, before, "a claim must not rewrite the event row or its stored payload");
    assert_eq!(
        claimed.first().and_then(|entry| entry.event.entity_snapshot.as_deref()),
        Some(&snapshot),
        "the claim reads the stored payload back as the state written"
    );
    pool.close().await;
}
