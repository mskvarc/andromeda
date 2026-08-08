//! Persisted (wire) form of a batch side-effect job stored in the
//! `pending_side_effects.payload` JSONB column.
//!
//! The `From` and `TryFrom` impls that convert between these shapes and
//! the domain types (`BatchSideEffectJob`, `ChangeEvent`) live in the
//! binary crate, because those domain types are defined there.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-persisted shape of a batch side-effect job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedBatchSideEffectJob {
    /// Temporal entity recordings queued for later.
    pub temporal_entities: Vec<PersistedTemporalEntity>,
    /// Change events queued for notification delivery.
    pub change_events: Vec<PersistedChangeEvent>,
}

/// Single temporal entity row queued for later recording.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTemporalEntity {
    /// Entity identifier.
    pub id: String,
    /// The expanded JSON-LD payload.
    pub expanded: Value,
}

/// Wire-shape of a change event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedChangeEvent {
    /// Entity identifier.
    pub entity_id: String,
    /// Entity type IRIs.
    pub entity_types: Vec<String>,
    /// The NGSI-LD notification trigger reason.
    pub trigger_reason: String,
    /// Attribute IRIs that changed.
    pub changed_attributes: Vec<String>,
    /// When the change occurred.
    pub occurred_at: DateTime<Utc>,
    /// Entity snapshot after the change, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_snapshot: Option<Value>,
    /// Previous attribute values before the change, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_attributes: Option<Value>,
    /// System attributes at the time of the change, if available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sys_attrs_snapshot: Option<PersistedSysAttrs>,
    /// Whether the change originated locally or was forwarded.
    pub origin: String,
}

/// Wire-shape of system attributes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSysAttrs {
    /// Entity creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Entity last-modification timestamp.
    pub modified_at: DateTime<Utc>,
    /// Entity expiration timestamp, if set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
}
