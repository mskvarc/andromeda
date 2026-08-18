//! The event and deliveries a notification verification starts from.

use crate::queue_database::{record_events, subscription, subscription_key};
use andromeda_context_commit::{write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use andromeda_postgresql_context_store::notification_delivery::rows::NewNotificationEvent;
use chrono::Utc;
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
    let identifiers: Vec<Uri> = (0..subscriptions).map(|_| subscription_id()).collect();
    let mut subscription_keys = Vec::with_capacity(subscriptions);
    for identifier in &identifiers {
        subscription_keys.push(subscription(pool, identifier.as_str()).await);
    }

    let snapshot = entity_snapshot(&entity);
    let record = WriteRecord::mint(Utc::now());
    let events = vec![NewNotificationEvent {
        ordinal: WriteOrdinal::at(0),
        trigger: NotificationTrigger::EntityCreated,
        entity_id: &entity,
        changed_attribute_name_ids: Vec::new(),
        entity_snapshot: Some(&snapshot),
        previous_snapshot: None,
        subscription_keys: subscription_keys.iter().map(|key| subscription_key(*key)).collect(),
    }];
    record_events(pool, &record, &events).await.expect("one write's events and deliveries");

    QueuedNotification { record, subscription_keys }
}

/// Identifiers of the deliveries one write owes, in queue order.
pub async fn delivery_ids(pool: &PgPool, record: &WriteRecord) -> Vec<i64> {
    sqlx::query_scalar("SELECT d.id FROM notification_deliveries d JOIN notification_events e ON e.id = d.event_id WHERE e.write_id = $1 ORDER BY d.id")
        .bind(record.write_id().as_uuid())
        .fetch_all(pool)
        .await
        .expect("delivery identifiers")
}

/// A unique Entity identifier for one verification's fixture.
pub fn entity_id() -> Uri {
    format!("urn:ngsi-ld:Vehicle:durable-queue:{}", Uuid::new_v4())
        .parse()
        .expect("a valid entity URI")
}

/// A unique Subscription identifier for one verification's fixture.
pub fn subscription_id() -> Uri {
    format!("urn:ngsi-ld:Subscription:durable-queue:{}", Uuid::new_v4())
        .parse()
        .expect("a valid subscription URI")
}

/// The expanded Entity state an event carries.
pub fn entity_snapshot(entity: &Uri) -> Value {
    json!({
        "@id": entity.as_str(),
        "@type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
        "https://uri.etsi.org/ngsi-ld/default-context/speed": [{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 120}]
        }]
    })
}
