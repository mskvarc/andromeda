//! NGSI-LD Notification types (ETSI GS CIM 009 clause 5.3.1).

use crate::time_format;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Maps NGSI-LD entity mutation operations to spec trigger reasons.
#[derive(Clone, Debug)]
pub enum TriggerReason {
    EntityCreated,
    EntityUpdated,
    EntityDeleted,
    AttributeCreated,
    AttributeUpdated,
    AttributeDeleted,
}

impl TriggerReason {
    /// Returns the NGSI-LD notificationTrigger string for this reason.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EntityCreated => "entityCreated",
            Self::EntityUpdated => "entityUpdated",
            Self::EntityDeleted => "entityDeleted",
            Self::AttributeCreated => "attributeCreated",
            Self::AttributeUpdated => "attributeUpdated",
            Self::AttributeDeleted => "attributeDeleted",
        }
    }

    /// Default triggers per NGSI-LD spec.
    pub fn defaults() -> Vec<&'static str> {
        vec![
            "entityCreated",
            "entityUpdated",
            "entityDeleted",
            "attributeCreated",
            "attributeUpdated",
            "attributeDeleted",
        ]
    }
}

/// Represents an entity mutation that may trigger notifications.
#[derive(Clone, Debug)]
pub struct ChangeEvent {
    pub entity_id: String,
    pub entity_types: Vec<String>,
    pub trigger_reason: TriggerReason,
    pub changed_attributes: Vec<String>,
    pub occurred_at: DateTime<Utc>,
    /// For EntityDeleted events, a snapshot of the entity before deletion (expanded form).
    pub entity_snapshot: Option<Value>,
    /// Entity state before the mutation (expanded form), for showChanges support.
    pub previous_attributes: Option<Value>,
    /// For EntityDeleted events, a snapshot of (createdAt, modifiedAt, expiresAt) captured before deletion.
    pub sys_attrs_snapshot: Option<(DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)>,
}

impl ChangeEvent {
    pub fn new(entity_id: String, entity_types: Vec<String>, trigger_reason: TriggerReason) -> Self {
        Self {
            entity_id,
            entity_types,
            trigger_reason,
            changed_attributes: Vec::new(),
            occurred_at: Utc::now(),
            entity_snapshot: None,
            previous_attributes: None,
            sys_attrs_snapshot: None,
        }
    }

    pub fn with_changed_attributes(mut self, attrs: Vec<String>) -> Self {
        self.changed_attributes = attrs;
        self
    }

    pub fn with_snapshot(mut self, snapshot: Value) -> Self {
        self.entity_snapshot = Some(snapshot);
        self
    }

    pub fn with_previous(mut self, previous: Value) -> Self {
        self.previous_attributes = Some(previous);
        self
    }

    pub fn with_optional_previous(mut self, previous: Option<Value>) -> Self {
        self.previous_attributes = previous;
        self
    }

    pub fn with_sys_attrs(mut self, sys_attrs: Option<(DateTime<Utc>, DateTime<Utc>, Option<DateTime<Utc>>)>) -> Self {
        self.sys_attrs_snapshot = sys_attrs;
        self
    }
}

/// The NGSI-LD Notification payload structure (clause 5.3.1).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,

    #[serde(rename = "type")]
    pub type_: String,

    #[serde(rename = "subscriptionId")]
    pub subscription_id: String,

    #[serde(rename = "notifiedAt", with = "time_format::rfc3339")]
    pub notified_at: DateTime<Utc>,

    pub data: Vec<Value>,
}

/// Trigger reasons for CSR notifications (spec clause 5.3.2).
#[derive(Clone, Debug, PartialEq)]
pub enum CsrTriggerReason {
    NewlyMatching,
    Updated,
    NoLongerMatching,
}

impl CsrTriggerReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NewlyMatching => "newlyMatching",
            Self::Updated => "updated",
            Self::NoLongerMatching => "noLongerMatching",
        }
    }
}

/// Represents a CSR mutation that may trigger CSR subscription notifications.
#[derive(Clone, Debug)]
pub struct CsrChangeEvent {
    pub csr_id: String,
    pub trigger_reason: CsrTriggerReason,
    pub occurred_at: DateTime<Utc>,
    /// For delete events, a snapshot of the CSR before deletion.
    pub csr_snapshot: Option<Value>,
}

/// The NGSI-LD CsourceNotification payload structure (clause 5.3.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CsourceNotification {
    pub id: String,

    #[serde(rename = "type")]
    pub type_: String,

    #[serde(rename = "subscriptionId")]
    pub subscription_id: String,

    #[serde(rename = "notifiedAt", with = "time_format::rfc3339")]
    pub notified_at: DateTime<Utc>,

    pub data: Vec<Value>,

    #[serde(skip_serializing_if = "Option::is_none", rename = "triggerReason")]
    pub trigger_reason: Option<String>,
}
