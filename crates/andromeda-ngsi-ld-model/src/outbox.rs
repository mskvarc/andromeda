//! Outbox-based notification delivery domain types.
//!
//! Defines the data structures for the notification outbox pattern,
//! enabling atomic persistence of notification intents alongside entity
//! mutations (ETSI GS CIM 009 clause 5.3).

use crate::notification::TriggerReason;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use chrono::{DateTime, Utc};
use derive_more::{Display, From};
use serde_json::Value;
use typed_builder::TypedBuilder;

/// Unique identifier for a notification outbox entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, From, Display)]
pub struct OutboxEntryId(i64);

impl OutboxEntryId {
    /// Returns the raw `i64` value.
    pub const fn into_inner(self) -> i64 {
        self.0
    }
}

/// Identifies a worker instance that has claimed outbox entries for delivery.
#[derive(Debug, Clone, PartialEq, Eq, From, Display)]
pub struct WorkerInstanceId(String);

impl WorkerInstanceId {
    /// Returns the inner string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A persisted notification outbox row, representing a pending or completed
/// notification delivery attempt.
#[derive(Debug, Clone, TypedBuilder)]
pub struct OutboxEntry {
    /// Auto-generated row identifier.
    pub id: OutboxEntryId,
    /// Subscription that matched the entity change.
    pub subscription_id: Uri,
    /// Entity that triggered the notification.
    pub entity_id: Uri,
    /// Mutation trigger reason (e.g. entityCreated, entityUpdated).
    pub trigger: TriggerReason,
    /// Expanded entity snapshot at the time of the mutation.
    #[builder(default)]
    pub entity_snapshot: Option<Value>,
    /// Entity state before the mutation (for showChanges support).
    #[builder(default)]
    pub previous_snapshot: Option<Value>,
    /// Names of the attributes the mutation changed.
    #[builder(default)]
    pub changed_attrs: Vec<AttributeName>,
    /// Timestamp when the outbox entry was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp when a worker claimed this entry for delivery.
    #[builder(default)]
    pub claimed_at: Option<DateTime<Utc>>,
    /// Identifier of the worker that claimed this entry.
    #[builder(default)]
    pub claimed_by: Option<WorkerInstanceId>,
    /// Timestamp when the notification was successfully delivered.
    #[builder(default)]
    pub delivered_at: Option<DateTime<Utc>>,
    /// Error message from the most recent failed delivery attempt.
    #[builder(default)]
    pub delivery_error: Option<String>,
    /// Number of delivery attempts made so far.
    pub attempt_count: i32,
    /// Earliest time at which the next delivery attempt may be made.
    pub next_attempt_at: DateTime<Utc>,
}

/// Data required to insert a new notification outbox entry.
///
/// Contains only the fields known at insert time; delivery tracking fields
/// (claimed_at, delivered_at, etc.) are populated by the database or the
/// delivery worker.
#[derive(Debug, TypedBuilder)]
pub struct NewOutboxEntry {
    /// Subscription that matched the entity change.
    pub subscription_id: Uri,
    /// Entity that triggered the notification.
    pub entity_id: Uri,
    /// Mutation trigger reason.
    pub trigger: TriggerReason,
    /// Expanded entity snapshot at the time of the mutation.
    #[builder(default)]
    pub entity_snapshot: Option<Value>,
    /// Entity state before the mutation.
    #[builder(default)]
    pub previous_snapshot: Option<Value>,
    /// Names of the attributes the mutation changed.
    #[builder(default)]
    pub changed_attrs: Vec<AttributeName>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbox_entry_id_into_inner_returns_raw_value() {
        let id = OutboxEntryId::from(42_i64);
        assert_eq!(id.into_inner(), 42);
    }

    #[test]
    fn outbox_entry_id_display_formats_correctly() {
        let id = OutboxEntryId::from(123_i64);
        assert_eq!(format!("{}", id), "123");
    }

    #[test]
    fn worker_instance_id_as_str_returns_inner_string() {
        let worker_id = WorkerInstanceId::from("worker-001".to_string());
        assert_eq!(worker_id.as_str(), "worker-001");
    }

    #[test]
    fn worker_instance_id_display_formats_correctly() {
        let worker_id = WorkerInstanceId::from("test-worker".to_string());
        assert_eq!(format!("{}", worker_id), "test-worker");
    }
}
