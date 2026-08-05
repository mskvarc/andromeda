//! Pre-compiled, in-memory forms of subscriptions used by the matching hot path.
//!
//! These are derived artefacts, not stored state: `compile_subscription` builds
//! them once when a subscription enters the cache so that per-event matching
//! never re-parses a `q` expression, a `geoQ` geometry or a `scopeQ` pattern.
//! They live in the service layer rather than the domain because they reference
//! the query AST, the renderer's system-attribute policy and the transport a
//! notification will go out over -- all concerns above the data model.

use crate::transport::endpoint::EndpointTransport;
use andromeda_ngsi_ld_geometry::geo_rel::GeoRel;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::subscription::{EntitySelector, SubscriptionStatus};
use andromeda_ngsi_ld_query::{filter::expression::Expr, scope::query::ScopeQuery};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashSet;

/// Pre-compiled CSR subscription for in-memory matching against CSR changes.
#[derive(Debug)]
pub struct CompiledCsrSubscription {
    pub id: String,
    pub status: SubscriptionStatus,
    pub endpoint_uri: String,
    pub endpoint_accept: Option<String>,
    pub endpoint_receiver_info: Option<Vec<Value>>,
    pub endpoint_transport: EndpointTransport,
    pub selectors: Vec<EntitySelector>,
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
    pub selectors: Vec<EntitySelector>,
    pub throttling_secs: Option<f64>,
    pub time_interval_secs: Option<f64>,
    pub notification_attributes: Option<Vec<AttributeName>>,
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
    /// The subscription's parsed `scopeQ`, if set.
    pub compiled_scope_q: Option<ScopeQuery>,
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
    /// Reference geometry the entity geometry is compared against. Authoritative decoded form.
    pub ref_geometry: geo::Geometry,
    /// Parsed georel operator + distance modifiers.
    pub georel: GeoRel,
    /// Expanded IRI of the entity geoproperty to read.
    pub geoproperty_iri: String,
}
