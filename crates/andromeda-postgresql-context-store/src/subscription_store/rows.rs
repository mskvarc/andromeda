//! Persistence-shaped rows for the `subscriptions` table.
//!
//! The table holds both entity and context-source subscriptions; they are
//! discriminated via the `kind` column. Rows are produced by hand-assembled
//! query readers rather than `sqlx::FromRow` because the `dataset_id` column
//! is only present for entity subscriptions (see [`DatasetIdPresence`]).
//!
//! [`DatasetIdPresence`]: crate::subscription_store::params::DatasetIdPresence

use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    notification::NotificationTrigger,
    subscription::{EntitySelector, NotificationParams, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::{filter::expression::Expr, scope::query::ScopeQuery};
use chrono::{DateTime, Utc};
use langtag::LangTagBuf;
use serde_json::Value;
use url::Url;

/// A subscription row read from the `subscriptions` table. Column presence
/// matches the macro-generated projection list; optional/nullable columns
/// remain `Option`.
#[derive(Debug)]
pub struct SubscriptionRow {
    /// Subscription identifier.
    pub id: Uri,
    /// Store-assigned surrogate key the notification queue references.
    pub key: SubscriptionKey,
    /// Optional human-readable name.
    pub subscription_name: Option<String>,
    /// Optional free-text description.
    pub description: Option<String>,
    /// Entity selector JSON (nullable in DB).
    pub entities: Option<Vec<EntitySelector>>,
    /// Watched attribute IRIs.
    pub watched_attributes: Option<Vec<AttributeName>>,
    /// Validated notification triggers.
    pub notification_trigger: Option<Vec<NotificationTrigger>>,
    /// Periodic notification interval seconds.
    pub time_interval: Option<f64>,
    /// NGSI-LD q expression.
    pub q: Option<Expr>,
    /// Geo query JSON (nullable).
    pub geo_q: Option<Value>,
    /// Scope query expression.
    pub scope_q: Option<ScopeQuery>,
    /// Context source filter.
    pub csf: Option<Expr>,
    /// Expanded attribute names whose `q` operands were coerced (clause 5.2.12 `expandValues`).
    pub expand_values: Option<Vec<AttributeName>>,
    /// Expanded attribute names whose `q` object keys were expanded (clause 5.2.12 `jsonKeys`).
    pub json_keys: Option<Vec<AttributeName>>,
    /// Dataset IDs (entity subscriptions only; `None` when absent).
    pub dataset_id: Option<Vec<Uri>>,
    /// Notification endpoint configuration JSON.
    pub notification: NotificationParams,
    /// Expiry timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// Throttling seconds.
    pub throttling: Option<f64>,
    /// Current lifecycle status.
    pub status: SubscriptionStatus,
    /// Temporal query JSON.
    pub temporal_q: Option<Value>,
    /// Preferred language.
    pub lang: Option<LangTagBuf>,
    /// Associated JSON-LD context URI.
    pub jsonld_context: Option<Url>,
    /// Local-only scope flag.
    pub local_only: bool,
    /// Whether entities are assumed split across sources (`NULL` defers to the deployment default).
    pub split_entities: Option<bool>,
    /// Counter for notifications sent.
    pub times_sent: i32,
    /// Counter for deliveries that drew an unsuccessful response or timed out.
    pub times_failed: i32,
    /// Timestamp of the most recent notification attempt.
    pub last_notification: Option<DateTime<Utc>>,
    /// Timestamp of the most recent successful notification.
    pub last_success: Option<DateTime<Utc>>,
    /// Timestamp of the most recent failed notification.
    pub last_failure: Option<DateTime<Utc>>,
    /// Row creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Row last-modified timestamp.
    pub modified_at: DateTime<Utc>,
}
