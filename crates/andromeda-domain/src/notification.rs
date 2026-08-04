//! Change events and the notification payloads they produce (ETSI GS CIM 009 clause 5.3).

use crate::{attribute::name::AttributeName, constant_string, entity_type::EntityTypeName, uri::Uri};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
use std::sync::Arc;
use strum::{Display, EnumString};

constant_string!(NotificationType, "Notification");
constant_string!(CsourceNotificationType, "CsourceNotification");

/// Origin of an entity change event (ETSI GS CIM 009 v1.9.1, clause 5.5.13).
///
/// Distinguishes whether an entity mutation originated from the local broker
/// or was forwarded from a registered context source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOrigin {
    /// The entity is stored and managed by this broker instance.
    Local,
    /// The entity was forwarded from a registered context source.
    Forwarded,
}

/// System attribute timestamps captured before entity deletion.
#[derive(Clone, Debug)]
pub struct SysAttrsSnapshot {
    /// Creation timestamp the entity carried.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp the entity carried.
    pub modified_at: DateTime<Utc>,
    /// Expiration timestamp the entity carried, if any.
    pub expires_at: Option<DateTime<Utc>>,
}

/// Trigger reason for entity notifications (ETSI GS CIM 009 clause 5.3.3).
///
/// Maps NGSI-LD entity mutation operations to spec trigger reasons.
#[derive(Clone, Copy, Debug, Display, EnumString, PartialEq, Eq)]
#[strum(serialize_all = "camelCase")]
pub enum TriggerReason {
    /// An entity was created.
    EntityCreated,
    /// An entity was updated.
    EntityUpdated,
    /// An entity was deleted.
    EntityDeleted,
    /// An attribute was created on an entity.
    AttributeCreated,
    /// An attribute was updated on an entity.
    AttributeUpdated,
    /// An attribute was deleted from an entity.
    AttributeDeleted,
}

impl TriggerReason {
    /// Default triggers per NGSI-LD spec.
    pub const fn defaults() -> &'static [&'static str] {
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
    /// Identifier of the mutated entity.
    pub entity_id: Uri,
    /// Types the mutated entity declares.
    pub entity_types: Vec<EntityTypeName>,
    /// What kind of mutation this was.
    pub trigger_reason: TriggerReason,
    /// Names of the attributes the mutation touched.
    pub changed_attributes: Vec<AttributeName>,
    /// When the mutation happened.
    pub occurred_at: DateTime<Utc>,
    /// For EntityDeleted events, a snapshot of the entity before deletion (expanded form).
    pub entity_snapshot: Option<Arc<Value>>,
    /// Entity state before the mutation (expanded form), for showChanges support.
    pub previous_attributes: Option<Arc<Value>>,
    /// For EntityDeleted events, a snapshot of (createdAt, modifiedAt, expiresAt) captured before deletion.
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
    /// Whether this event originated locally or from a forwarded context source (clause 5.5.13).
    pub origin: ChangeOrigin,
    /// Post-mutation expanded entity payload, when the emitter has it in
    /// memory (every service write path does). Consumers that would
    /// otherwise re-fetch from the database (e.g. spatial index sync) read
    /// this directly to avoid an extra query per event during batch ingest.
    /// `None` for events emitted by background workers that only know the
    /// entity id (e.g. expiration sweeps); consumers fall back to fetching.
    pub current_payload: Option<Arc<Value>>,
}

impl ChangeEvent {
    /// Build an event for a mutation that has just happened locally.
    pub fn new(entity_id: Uri, entity_types: Vec<EntityTypeName>, trigger_reason: TriggerReason) -> ChangeEvent {
        ChangeEvent {
            entity_id,
            entity_types,
            trigger_reason,
            changed_attributes: Vec::new(),
            occurred_at: Utc::now(),
            entity_snapshot: None,
            previous_attributes: None,
            sys_attrs_snapshot: None,
            origin: ChangeOrigin::Local,
            current_payload: None,
        }
    }

    /// Record which attributes the mutation touched.
    pub fn with_changed_attributes(mut self, attrs: Vec<AttributeName>) -> ChangeEvent {
        self.changed_attributes = attrs;
        self
    }

    /// Attach the pre-deletion entity snapshot.
    pub fn with_snapshot(mut self, snapshot: Value) -> ChangeEvent {
        self.entity_snapshot = Some(Arc::new(snapshot));
        self
    }

    /// Attach the pre-mutation entity state, for `showChanges`.
    pub fn with_previous(mut self, previous: Value) -> ChangeEvent {
        self.previous_attributes = Some(Arc::new(previous));
        self
    }

    /// Attach the pre-mutation entity state when the caller has it.
    pub fn with_optional_previous(mut self, previous: Option<Value>) -> ChangeEvent {
        self.previous_attributes = previous.map(Arc::new);
        self
    }

    /// Attach the timestamps captured before a deletion.
    pub const fn with_sys_attrs(mut self, sys_attrs: Option<SysAttrsSnapshot>) -> ChangeEvent {
        self.sys_attrs_snapshot = sys_attrs;
        self
    }

    /// Attach the post-mutation expanded payload so payload-aware consumers
    /// (notably the spatial sync worker) skip the per-event entity fetch
    /// during high-throughput batch ingest.
    pub fn with_current_payload(mut self, payload: Arc<Value>) -> ChangeEvent {
        self.current_payload = Some(payload);
        self
    }
}

/// NGSI-LD Notification payload (ETSI GS CIM 009 clause 5.3.1).
///
/// Represents a notification sent to a subscriber when matching entity
/// changes occur. Contains the subscription reference, timestamp, and
/// the entity data that triggered the notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    /// Generated notification identifier.
    pub id: Uri,
    /// Always `Notification`.
    #[serde(rename = "type")]
    pub type_: NotificationType,
    /// Subscription this notification answers.
    pub subscription_id: Uri,
    /// When the notification was generated.
    pub notified_at: DateTime<Utc>,
    /// The entity representations that triggered it.
    pub data: Vec<Value>,
}

/// Trigger reason for CSR notifications (ETSI GS CIM 009 clause 5.3.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, EnumString)]
#[serde(rename_all = "camelCase")]
#[strum(serialize_all = "camelCase")]
pub enum CsrTriggerReason {
    /// The registration started matching the subscription.
    NewlyMatching,
    /// A matching registration changed.
    Updated,
    /// The registration stopped matching the subscription.
    NoLongerMatching,
}

/// Represents a CSR mutation that may trigger CSR subscription notifications.
#[derive(Clone, Debug)]
pub struct CsrChangeEvent {
    /// Identifier of the mutated registration.
    pub csr_id: Uri,
    /// What kind of mutation this was.
    pub trigger_reason: CsrTriggerReason,
    /// For delete events, a snapshot of the CSR before deletion.
    pub csr_snapshot: Option<Value>,
}

/// NGSI-LD CsourceNotification payload (ETSI GS CIM 009 clause 5.3.2).
///
/// Represents a notification sent to a subscriber when context source
/// registration changes occur. Contains the subscription reference,
/// timestamp, CSR data, and the trigger reason.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsourceNotification {
    /// Generated notification identifier.
    pub id: Uri,
    /// Always `CsourceNotification`.
    #[serde(rename = "type")]
    pub type_: CsourceNotificationType,
    /// Subscription this notification answers.
    pub subscription_id: Uri,
    /// When the notification was generated.
    pub notified_at: DateTime<Utc>,
    /// The registration representations that triggered it.
    pub data: Vec<Value>,
    /// Why the notification fired.
    pub trigger_reason: Option<CsrTriggerReason>,
}

/// Signals subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    /// The subscription with this id was created or replaced.
    Upserted(Uri),
    /// The subscription with this id was deleted.
    Deleted(Uri),
}

/// Signals CSR subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    /// The CSR subscription with this id was created or replaced.
    Upserted(Uri),
    /// The CSR subscription with this id was deleted.
    Deleted(Uri),
}
