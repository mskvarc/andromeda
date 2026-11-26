//! The Subscription a rehearsal's deliveries are owed to, the events they
//! answer and the deliveries themselves, written column by column.

use crate::fixtures::{
    statements::{insert_returning, instant, integers, json_document, smallint, smallints, texts, uuid},
    test_error::UpgradeTestError,
};
use sea_query::{Expr, SimpleExpr};
use serde_json::json;
use sqlx::PgPool;

/// Inserts the entity Subscription the deliveries are owed to, as v1.0.0
/// wrote it, and returns its key.
pub(crate) async fn insert_subscription(pool: &PgPool) -> Result<i64, UpgradeTestError> {
    let row = vec![
        ("id", Expr::val("urn:ngsi-ld:Subscription:queued")),
        ("kind", smallint(1)),
        ("status", smallint(1)),
        ("created_at", instant("2025-01-27T21:00:00Z")),
        ("modified_at", instant("2025-01-27T21:00:00Z")),
        ("active_since", instant("2025-01-27T21:00:00Z")),
        ("selected_since", instant("2025-01-27T21:00:00Z")),
        (
            "notification",
            json_document(&json!({"endpoint": {"uri": "https://receiver.example.com/notify"}})),
        ),
    ];
    insert_returning(pool, "subscriptions", "subscription_key", row).await
}

/// Records the creation of an Entity, the event at `ordinal` of the write
/// `write_id` committed at `committed_at`, and returns its key.
pub(crate) async fn insert_event(pool: &PgPool, write_id: &str, ordinal: i32, committed_at: &str) -> Result<i64, UpgradeTestError> {
    let row = vec![
        ("created_at", instant(committed_at)),
        ("committed_at", instant(committed_at)),
        ("event_ordinal", Expr::val(ordinal)),
        ("lifecycle", smallint(1)),
        ("write_id", uuid(write_id)),
        ("entity_id", Expr::val("urn:ngsi-ld:Store:001")),
        ("change_kinds", smallints(&[])),
        ("change_attribute_name_keys", integers(&[])),
        ("change_dataset_ids", texts(&[])),
    ];
    insert_returning(pool, "notification_events", "event_key", row).await
}

/// Queues the delivery of `event` to `subscription` with the columns v1.0.0
/// wrote and the further ones in `named`, and returns its key.
pub(crate) async fn insert_delivery(pool: &PgPool, event: i64, subscription: i64, named: Vec<(&str, SimpleExpr)>) -> Result<i64, UpgradeTestError> {
    let written = vec![
        ("event_key", Expr::val(event)),
        ("subscription_key", Expr::val(subscription)),
        ("next_attempt_at", instant("2025-01-27T21:00:00Z")),
    ];
    insert_returning(pool, "notification_deliveries", "delivery_key", written.into_iter().chain(named).collect()).await
}
