//! Persistence-shaped row structs for the notification outbox table.
//!
//! These types mirror the `notification_outbox` columns verbatim and are
//! produced by `sqlx::FromRow`. They do not leak past the service/worker
//! layer: a mapper converts them into the domain [`OutboxEntry`].
//!
//! [`OutboxEntry`]: crate::repository::outbox::entry::OutboxEntry

use chrono::{DateTime, Utc};
use serde_json::Value;

/// A row from the `notification_outbox` table.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct OutboxEntryRow {
    /// Auto-generated row identifier.
    pub id: i64,
    /// Subscription URI (stored as text).
    pub subscription_id: String,
    /// Entity URI (stored as text).
    pub entity_id: String,
    /// Trigger reason (stored as text; parsed via `FromStr`).
    pub trigger: String,
    /// Expanded entity snapshot at the time of the mutation.
    pub entity_snapshot: Option<Value>,
    /// Entity state before the mutation.
    pub previous_snapshot: Option<Value>,
    /// Attribute IRIs that were changed.
    pub changed_attrs: Vec<String>,
    /// Row creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Timestamp when a worker claimed this entry.
    pub claimed_at: Option<DateTime<Utc>>,
    /// Worker that claimed this entry.
    pub claimed_by: Option<String>,
    /// Successful delivery timestamp.
    pub delivered_at: Option<DateTime<Utc>>,
    /// Most recent delivery error.
    pub delivery_error: Option<String>,
    /// Number of delivery attempts so far.
    pub attempt_count: i32,
    /// Earliest next delivery attempt.
    pub next_attempt_at: DateTime<Utc>,
}
