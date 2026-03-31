use crate::{
    domain::{serde_helpers, subscription::SubscriptionStatus},
    query::q::Expr,
    transport::endpoint::EndpointTransport,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use strum::{Display, EnumString};

/// System attribute timestamps captured before entity deletion.
#[derive(Clone, Debug)]
pub struct SysAttrsSnapshot {
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

// ---------------------------------------------------------------------------
// Notification types from NGSI-LD spec (ETSI GS CIM 009 clause 5.3.1)
// ---------------------------------------------------------------------------

/// Trigger reason for entity notifications (ETSI GS CIM 009 clause 5.3.3).
///
/// Maps NGSI-LD entity mutation operations to spec trigger reasons.
#[derive(Clone, Debug, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum TriggerReason {
    EntityCreated,
    EntityUpdated,
    EntityDeleted,
    AttributeCreated,
    AttributeUpdated,
    AttributeDeleted,
}

impl TriggerReason {
    /// Default triggers per NGSI-LD spec.
    pub fn defaults() -> &'static [&'static str] {
        &[
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
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
}

impl ChangeEvent {
    pub fn new(entity_id: String, entity_types: Vec<String>, trigger_reason: TriggerReason) -> ChangeEvent {
        ChangeEvent {
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

    pub fn with_changed_attributes(mut self, attrs: Vec<String>) -> ChangeEvent {
        self.changed_attributes = attrs;
        self
    }

    pub fn with_snapshot(mut self, snapshot: Value) -> ChangeEvent {
        self.entity_snapshot = Some(snapshot);
        self
    }

    pub fn with_previous(mut self, previous: Value) -> ChangeEvent {
        self.previous_attributes = Some(previous);
        self
    }

    pub fn with_optional_previous(mut self, previous: Option<Value>) -> ChangeEvent {
        self.previous_attributes = previous;
        self
    }

    pub fn with_sys_attrs(mut self, sys_attrs: Option<SysAttrsSnapshot>) -> ChangeEvent {
        self.sys_attrs_snapshot = sys_attrs;
        self
    }
}

fn default_notification_type() -> String {
    "Notification".to_string()
}

/// NGSI-LD Notification payload (ETSI GS CIM 009 clause 5.3.1).
///
/// Represents a notification sent to a subscriber when matching entity
/// changes occur. Contains the subscription reference, timestamp, and
/// the entity data that triggered the notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: String,

    #[serde(rename = "type", default = "default_notification_type")]
    pub type_: String,

    pub subscription_id: String,

    #[serde(with = "serde_helpers::rfc3339")]
    pub notified_at: DateTime<Utc>,

    pub data: Vec<Value>,
}

/// Trigger reason for CSR notifications (ETSI GS CIM 009 clause 5.3.2).
#[derive(Clone, Debug, PartialEq, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum CsrTriggerReason {
    NewlyMatching,
    Updated,
    NoLongerMatching,
}

/// Represents a CSR mutation that may trigger CSR subscription notifications.
#[derive(Clone, Debug)]
pub struct CsrChangeEvent {
    pub csr_id: String,
    pub trigger_reason: CsrTriggerReason,
    /// For delete events, a snapshot of the CSR before deletion.
    pub csr_snapshot: Option<Value>,
}

fn default_csource_notification_type() -> String {
    "CsourceNotification".to_string()
}

/// NGSI-LD CsourceNotification payload (ETSI GS CIM 009 clause 5.3.2).
///
/// Represents a notification sent to a subscriber when context source
/// registration changes occur. Contains the subscription reference,
/// timestamp, CSR data, and the trigger reason.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsourceNotification {
    pub id: String,

    #[serde(rename = "type", default = "default_csource_notification_type")]
    pub type_: String,

    pub subscription_id: String,

    #[serde(with = "serde_helpers::rfc3339")]
    pub notified_at: DateTime<Utc>,

    pub data: Vec<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Subscription compilation types
// ---------------------------------------------------------------------------

/// Signals subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    Upserted(String), // subscription id
    Deleted(String),  // subscription id
}

/// Signals CSR subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    Upserted(String),
    Deleted(String),
}

/// Pre-compiled entity selector for fast matching.
#[derive(Debug)]
pub struct CompiledSelector {
    pub id: Option<String>,
    pub id_pattern: Option<Regex>,
    pub type_: String,               // expanded IRI
    pub type_pattern: Option<Regex>, // regex for alternation patterns like (Building|Tower)
}

/// Pre-compiled CSR subscription for in-memory matching against CSR changes.
#[derive(Debug)]
pub struct CompiledCsrSubscription {
    pub id: String,
    pub status: SubscriptionStatus,
    pub endpoint_uri: String,
    pub endpoint_accept: Option<String>,
    pub endpoint_receiver_info: Option<Vec<Value>>,
    pub endpoint_transport: EndpointTransport,
    pub selectors: Vec<CompiledSelector>,
    pub watched_attrs: Option<HashSet<String>>,
    pub throttling_secs: Option<f64>,
    pub time_interval_secs: Option<f64>,
    pub jsonld_context: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub compiled_csf: Option<Expr>,
}

/// Pre-compiled subscription for in-memory matching.
#[derive(Debug)]
pub struct CompiledSubscription {
    pub id: String,
    pub status: SubscriptionStatus,
    pub endpoint_uri: String,
    pub endpoint_accept: Option<String>,
    pub endpoint_receiver_info: Option<Vec<Value>>,
    pub endpoint_transport: EndpointTransport,
    pub triggers: Vec<String>,
    pub watched_attrs: Option<HashSet<String>>, // expanded IRIs; None = all
    pub selectors: Vec<CompiledSelector>,
    pub throttling_secs: Option<f64>,
    pub time_interval_secs: Option<f64>,
    pub notification_attributes: Option<Vec<String>>,
    pub notification_format: Option<String>,
    pub notification_sys_attrs: bool,
    pub notification_show_changes: bool,
    pub jsonld_context: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub compiled_q: Option<Expr>,
    pub geo_q: Option<Value>,
    pub scope_q: Option<String>,
    pub dataset_id: Option<Vec<String>>,
    pub notification_join: Option<String>,
    pub notification_join_level: u32,
    pub compiled_csf: Option<Expr>,
}
