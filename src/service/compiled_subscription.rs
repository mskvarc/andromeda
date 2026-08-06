//! Pre-compiled, in-memory forms of subscriptions used by the matching hot path.
//!
//! These are derived artefacts, not stored state: `compile_subscription` builds
//! them once when a subscription enters the cache so that per-event matching
//! never re-parses a `q` expression, a `geoQ` geometry or a `scopeQ` pattern.
//! They live in the service layer rather than the domain because they reference
//! the query AST, the renderer's system-attribute policy and the transport a
//! notification will go out over -- all concerns above the data model.

use andromeda_delivery::endpoint::transport::EndpointTransport;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    notification::NotificationTrigger,
    subscription::{EntitySelector, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::query::GeoQuery, scope::query::ScopeQuery};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use mediatype::MediaTypeBuf;
use std::collections::HashSet;
use url::Url;

/// Pre-compiled CSR subscription for in-memory matching against CSR changes.
#[derive(Debug)]
pub struct CompiledCsrSubscription {
    pub id: Uri,
    pub status: SubscriptionStatus,
    pub endpoint_uri: Url,
    pub endpoint_accept: Option<MediaTypeBuf>,
    pub endpoint_receiver_info: Option<Vec<KeyValuePair>>,
    pub endpoint_transport: EndpointTransport,
    pub selectors: Vec<EntitySelector>,
    pub watched_attrs: Option<HashSet<String>>,
    pub throttling_secs: Option<f64>,
    pub time_interval_secs: Option<f64>,
    pub jsonld_context: Option<Url>,
    pub expires_at: Option<DateTime<Utc>>,
    pub compiled_csf: Option<Expr>,
}

/// Pre-compiled subscription for in-memory matching.
#[derive(Debug)]
pub struct CompiledSubscription {
    pub id: Uri,
    pub status: SubscriptionStatus,
    pub endpoint_uri: Url,
    pub endpoint_accept: Option<MediaTypeBuf>,
    pub endpoint_receiver_info: Option<Vec<KeyValuePair>>,
    pub endpoint_transport: EndpointTransport,
    pub triggers: Vec<NotificationTrigger>,
    pub watched_attrs: Option<HashSet<String>>, // expanded IRIs; None = all
    pub selectors: Vec<EntitySelector>,
    pub throttling_secs: Option<f64>,
    pub time_interval_secs: Option<f64>,
    pub notification_attributes: Option<Vec<AttributeName>>,
    pub notification_format: Option<String>,
    pub notification_sys_attrs: SysAttrsInclusion,
    pub notification_show_changes: bool,
    pub jsonld_context: Option<Url>,
    pub expires_at: Option<DateTime<Utc>>,
    pub compiled_q: Option<Expr>,
    /// The subscription's `geoQ`, compiled to the same [`GeoQuery`] a
    /// request-borne geo filter produces (clause 4.10).
    ///
    /// `None` when the subscription carries no `geoQ`, or when the one it
    /// carries could not be parsed — in which case it never matches an entity
    /// through the geo path at all.
    pub compiled_geo_q: Option<GeoQuery>,
    /// The subscription's parsed `scopeQ`, if set.
    pub compiled_scope_q: Option<ScopeQuery>,
    pub dataset_id: Option<Vec<String>>,
    pub notification_join: Option<String>,
    pub notification_join_level: u32,
    pub compiled_csf: Option<Expr>,
    /// When true, this subscription only matches locally-stored entities (clause 5.5.13).
    pub local_only: bool,
}
