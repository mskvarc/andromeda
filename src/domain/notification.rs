use crate::{
    constant_string,
    domain::{subscription::SubscriptionStatus, uri::Uri},
    query::q::ast::Expr,
    renderer::SysAttrsInclusion,
    transport::endpoint::EndpointTransport,
};
use chrono::{DateTime, Utc};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_with::skip_serializing_none;
use std::{collections::HashSet, sync::Arc};
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
#[derive(Clone, Copy, Debug, Display, EnumString, PartialEq, Eq)]
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
    pub entity_id: Uri,
    pub entity_types: Vec<String>,
    pub trigger_reason: TriggerReason,
    pub changed_attributes: Vec<String>,
    pub occurred_at: DateTime<Utc>,
    /// For EntityDeleted events, a snapshot of the entity before deletion (expanded form).
    pub entity_snapshot: Option<Arc<Value>>,
    /// Entity state before the mutation (expanded form), for showChanges support.
    pub previous_attributes: Option<Arc<Value>>,
    /// For EntityDeleted events, a snapshot of (createdAt, modifiedAt, expiresAt) captured before deletion.
    pub sys_attrs_snapshot: Option<SysAttrsSnapshot>,
    /// Whether this event originated locally or from a forwarded context source (clause 5.5.13).
    pub origin: ChangeOrigin,
}

impl ChangeEvent {
    pub fn new(entity_id: Uri, entity_types: Vec<String>, trigger_reason: TriggerReason) -> ChangeEvent {
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
        }
    }

    pub fn with_changed_attributes(mut self, attrs: Vec<String>) -> ChangeEvent {
        self.changed_attributes = attrs;
        self
    }

    pub fn with_snapshot(mut self, snapshot: Value) -> ChangeEvent {
        self.entity_snapshot = Some(Arc::new(snapshot));
        self
    }

    pub fn with_previous(mut self, previous: Value) -> ChangeEvent {
        self.previous_attributes = Some(Arc::new(previous));
        self
    }

    pub fn with_optional_previous(mut self, previous: Option<Value>) -> ChangeEvent {
        self.previous_attributes = previous.map(Arc::new);
        self
    }

    pub const fn with_sys_attrs(mut self, sys_attrs: Option<SysAttrsSnapshot>) -> ChangeEvent {
        self.sys_attrs_snapshot = sys_attrs;
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
    pub id: Uri,
    #[serde(rename = "type")]
    pub type_: NotificationType,
    pub subscription_id: Uri,
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

/// NGSI-LD CsourceNotification payload (ETSI GS CIM 009 clause 5.3.2).
///
/// Represents a notification sent to a subscriber when context source
/// registration changes occur. Contains the subscription reference,
/// timestamp, CSR data, and the trigger reason.
#[skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CsourceNotification {
    pub id: Uri,

    #[serde(rename = "type")]
    pub type_: CsourceNotificationType,

    pub subscription_id: Uri,

    pub notified_at: DateTime<Utc>,

    pub data: Vec<Value>,

    pub trigger_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// Subscription compilation types
// ---------------------------------------------------------------------------

/// Signals subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum SubscriptionChange {
    Upserted(Uri), // subscription id
    Deleted(Uri),  // subscription id
}

/// Signals CSR subscription CRUD changes for cache invalidation.
#[derive(Clone, Debug)]
pub enum CsourceSubscriptionChange {
    Upserted(Uri),
    Deleted(Uri),
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
    pub notification_sys_attrs: SysAttrsInclusion,
    pub notification_show_changes: bool,
    pub jsonld_context: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub compiled_q: Option<Expr>,
    /// Pre-parsed geometry + georel from the `geoQ` parameter, if set.
    /// `None` when the subscription does not carry a `geoQ` filter or the
    /// filter could not be parsed at compile time (in which case the
    /// subscription never matches an entity through the geo path).
    pub compiled_geo_q: Option<CompiledGeoQuery>,
    /// Pre-parsed `scopeQ` pattern tree, if set.
    pub compiled_scope_q: Option<CompiledScopePattern>,
    pub dataset_id: Option<Vec<String>>,
    pub notification_join: Option<String>,
    pub notification_join_level: u32,
    pub compiled_csf: Option<Expr>,
    /// When true, this subscription only matches locally-stored entities (clause 5.5.13).
    pub local_only: bool,
}

/// Pre-compiled `geoQ` query (ETSI GS CIM 009 clause 4.10).
///
/// Stores the reference geometry parsed into `geo::Geometry`, the georel
/// operator pre-parsed with its modifier distances, and the expanded IRI of
/// the geoproperty to evaluate against. Built once in `compile_subscription`
/// so event matching does not re-parse GeoJSON on every event.
#[derive(Debug, Clone)]
pub struct CompiledGeoQuery {
    /// Reference geometry the entity geometry is compared against.
    pub ref_geometry: geo::Geometry,
    /// Parsed georel operator + distance modifiers.
    pub georel: CompiledGeoRel,
    /// Expanded IRI of the entity geoproperty to read.
    pub geoproperty_iri: String,
}

/// Parsed `georel` value including `near` distance modifiers.
#[derive(Debug, Clone, PartialEq)]
pub enum CompiledGeoRel {
    Within,
    Contains,
    Intersects,
    Disjoint,
    Equals,
    Overlaps,
    Near { max_distance: Option<f64>, min_distance: Option<f64> },
}

/// Pre-parsed `scopeQ` pattern (ETSI GS CIM 009 clause 4.18).
///
/// The raw scopeQ string uses `,` for OR and `;` for AND between terms; each
/// term may be an exact scope path, a hierarchical wildcard (`/#`), or a
/// single-level wildcard (`/+`). Splitting and regex pre-compilation happen
/// once at subscription load time.
#[derive(Debug, Clone)]
pub struct CompiledScopePattern {
    /// Disjunction of conjunctions: the outer `Vec` is OR, the inner is AND.
    pub or_terms: Vec<Vec<CompiledScopeTerm>>,
}

/// A single term in a compiled scope pattern.
#[derive(Debug, Clone)]
pub enum CompiledScopeTerm {
    /// Exact match against a scope path.
    Exact(String),
    /// Hierarchical wildcard `<prefix>/#` — matches any path whose prefix
    /// equals the stored prefix followed by a single additional segment.
    HierarchicalWildcard { prefix: String },
    /// `/+` wildcard expressed as a compiled regex.
    Regex(Regex),
}
