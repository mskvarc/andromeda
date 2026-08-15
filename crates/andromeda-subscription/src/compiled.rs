//! Pre-compiled, in-memory forms of subscriptions used by the matching hot path.
//!
//! These are derived artefacts, not stored state: `compile_subscription` builds
//! them once when a subscription enters the cache so that per-event matching
//! never re-parses a `q` expression, a `geoQ` geometry or a `scopeQ` pattern.
//! They live in subscription policy rather than the domain model because they
//! reference the query AST and the renderer's system-attribute policy. Transport
//! selection remains in delivery bindings and is not compiled here.

use crate::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    notification::NotificationTrigger,
    subscription::{EntitySelector, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::query::GeoQuery, scope::query::ScopeQuery};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, response_format::ResponseFormat, sys_attrs::SysAttrsInclusion};
use chrono::{DateTime, Utc};
use mediatype::MediaTypeBuf;
use std::{collections::HashSet, num::NonZeroU32};
use url::Url;

/// Pre-compiled CSR subscription for in-memory matching against CSR changes.
#[derive(Debug)]
pub struct CompiledCsrSubscription {
    /// Subscription identifier.
    pub id: Uri,
    /// Current lifecycle status.
    pub status: SubscriptionStatus,
    /// Notification endpoint URL.
    pub endpoint_uri: Url,
    /// Preferred notification response media type.
    pub endpoint_accept: Option<MediaTypeBuf>,
    /// Receiver metadata sent with a notification.
    pub endpoint_receiver_info: Option<Vec<KeyValuePair>>,
    /// Transport-selection metadata interpreted by delivery bindings.
    pub endpoint_notifier_info: Option<Vec<KeyValuePair>>,
    /// Entity selectors applied to context-source information.
    pub selectors: Vec<EntitySelector>,
    /// Expanded watched attribute names, or every attribute when absent.
    pub watched_attrs: Option<HashSet<AttributeName>>,
    /// Minimum seconds between notifications.
    pub throttling_secs: Option<f64>,
    /// Periodic notification interval in seconds.
    pub time_interval_secs: Option<f64>,
    /// JSON-LD context supplied with notifications.
    pub jsonld_context: Option<Url>,
    /// Subscription expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// Parsed context-source filter.
    pub compiled_csf: Option<Expr>,
}

/// Pre-compiled subscription for in-memory matching.
#[derive(Debug)]
pub struct CompiledSubscription {
    /// Subscription identifier.
    pub id: Uri,
    /// The identity the store assigned this subscription, carried beside the
    /// identifier so a match enqueues its delivery against the narrow key while
    /// the notification it produces still names the subscription by its URI.
    pub key: SubscriptionKey,
    /// Current lifecycle status.
    pub status: SubscriptionStatus,
    /// Notification endpoint URL.
    pub endpoint_uri: Url,
    /// Preferred notification response media type.
    pub endpoint_accept: Option<MediaTypeBuf>,
    /// Receiver metadata sent with a notification.
    pub endpoint_receiver_info: Option<Vec<KeyValuePair>>,
    /// Transport-selection metadata interpreted by delivery bindings.
    pub endpoint_notifier_info: Option<Vec<KeyValuePair>>,
    /// Events that trigger this subscription.
    pub triggers: Vec<NotificationTrigger>,
    /// Expanded watched attribute names, or every attribute when absent.
    pub watched_attrs: Option<HashSet<AttributeName>>, // expanded IRIs; None = all
    /// Entity selectors evaluated during matching.
    pub selectors: Vec<EntitySelector>,
    /// Minimum seconds between notifications.
    pub throttling_secs: Option<f64>,
    /// Periodic notification interval in seconds.
    pub time_interval_secs: Option<f64>,
    /// Attributes projected into notification data.
    pub notification_attributes: Option<Vec<AttributeName>>,
    /// Requested notification representation format.
    pub notification_format: Option<ResponseFormat>,
    /// Whether system attributes remain in notification data.
    pub notification_sys_attrs: SysAttrsInclusion,
    /// Whether notification data contains previous values.
    pub notification_show_changes: bool,
    /// JSON-LD context supplied with notifications.
    pub jsonld_context: Option<Url>,
    /// Subscription expiration timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// When the subscription was created (clause 5.2.12 `createdAt`).
    ///
    /// A subscription is notified of changes that happen after it exists, so an
    /// entity change whose `occurredAt` precedes this instant is not matched.
    /// Without it, an entity change committed before the subscription was
    /// created still reaches it once the notification worker processes that
    /// change after the subscription has entered the cache.
    pub created_at: DateTime<Utc>,
    /// Parsed entity filter expression.
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
    /// Dataset identifiers admitted by the subscription.
    pub dataset_id: Option<Vec<Uri>>,
    /// Linked-entity join mode requested for notification data.
    pub notification_join: Option<JoinMode>,
    /// Maximum linked-entity join depth (the clause 4.5.23 joinLevel query parameter), which
    /// the spec bounds only by being a positive integer.
    pub notification_join_level: NonZeroU32,
    /// Parsed context-source filter expression.
    pub compiled_csf: Option<Expr>,
    /// When true, this subscription only matches locally-stored entities (clause 5.5.13).
    pub local_only: bool,
}
