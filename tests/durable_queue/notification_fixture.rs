//! The event and deliveries a notification verification starts from.

use crate::queue_database::{deliveries_of_write, record_events, subscription, subscription_key};
use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
use andromeda_notification::{delivery_claim::DeliveryClaim, delivery_entry::DeliveryEntry};
use andromeda_postgresql_context_store::notification_delivery::rows::NewNotificationEvent;
use andromeda_postgresql_schema::notification::NotificationDeliveries;
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{TimeZone, Utc};
use sea_query::Order;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// One committed write, its single event, and the deliveries that event owes.
pub struct QueuedNotification {
    /// Identity the event was written under.
    pub record: WriteRecord,
    /// Surrogate keys of the Subscriptions the event is due to, in seed order.
    pub subscription_keys: Vec<i64>,
}

/// Seed `subscriptions` Subscriptions and commit one event due to all of them.
pub async fn queue_notification(pool: &PgPool, subscriptions: usize) -> QueuedNotification {
    let entity = entity_id();
    let identifiers: Vec<Identifier> = (0..subscriptions).map(|_| subscription_id()).collect();
    let mut subscription_keys = Vec::with_capacity(subscriptions);
    for identifier in &identifiers {
        subscription_keys.push(subscription(pool, identifier.as_str()).await);
    }

    let snapshot = entity_snapshot(&entity);
    let change = EntityChange::new(EntityLifecycle::Created, Vec::new());
    let record = WriteRecord::mint(Utc::now());
    let events = vec![NewNotificationEvent {
        ordinal: WriteOrdinal::at(0),
        change: &change,
        entity_id: &entity,
        change_attribute_name_keys: Vec::new(),
        entity_snapshot: Some(&snapshot),
        previous_snapshot: None,
        sys_attrs_snapshot: None,
        subscription_keys: subscription_keys.iter().map(|key| subscription_key(*key)).collect(),
    }];
    record_events(pool, &record, &events).await.expect("one write's events and deliveries");

    QueuedNotification { record, subscription_keys }
}

/// Identifiers of the deliveries one write owes, in queue order.
pub async fn delivery_ids(pool: &PgPool, record: &WriteRecord) -> Vec<i64> {
    deliveries_of_write(record.write_id().as_uuid())
        .column((NotificationDeliveries::Table, NotificationDeliveries::DeliveryKey))
        .order_by((NotificationDeliveries::Table, NotificationDeliveries::DeliveryKey), Order::Asc)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("delivery identifiers")
}

/// A unique Entity identifier for one verification's fixture.
pub fn entity_id() -> Identifier {
    format!("urn:ngsi-ld:Vehicle:durable-queue:{}", Uuid::new_v4())
        .parse()
        .expect("a valid entity URI")
}

/// A unique Subscription identifier for one verification's fixture.
pub fn subscription_id() -> Identifier {
    format!("urn:ngsi-ld:Subscription:durable-queue:{}", Uuid::new_v4())
        .parse()
        .expect("a valid subscription URI")
}

/// The stored expanded document of the Entity an event carries.
pub fn entity_document(entity: &Identifier) -> Value {
    json!([{
        "@id": entity.as_str(),
        "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
        "https://uri.etsi.org/ngsi-ld/default-context/speed": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 120}]
        }]
    }])
}

/// The Entity state an event carries.
pub fn entity_snapshot(entity: &Identifier) -> EntityState {
    decoded(&entity_document(entity))
}

/// Decode a stored expanded document into the state a commit of it stores,
/// every instance stamped at one whole-second instant a `timestamptz` keeps
/// exactly.
pub fn decoded(document: &Value) -> EntityState {
    let mut state = EntityState::from_slice(&serde_json::to_vec(document).expect("the document serialises")).expect("the document is an entity state");
    state.settle(
        None,
        SystemInstant::new(Utc.with_ymd_and_hms(2026, 10, 12, 0, 0, 0).single().expect("a valid instant")),
    );
    state
}

/// The deliveries a claim read back, failing the verification on any row the
/// claim could not read: every fixture here writes only readable rows.
pub fn readable(claims: Vec<DeliveryClaim>) -> Vec<DeliveryEntry> {
    claims
        .into_iter()
        .map(|claim| match claim {
            DeliveryClaim::Readable(entry) => Ok(*entry),
            DeliveryClaim::Unreadable(unreadable) => Err(unreadable.diagnostic),
        })
        .collect::<Result<Vec<_>, _>>()
        .expect("every fixture row reads back")
}
