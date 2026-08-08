//! Persistence-shaped rows for the `subscriptions` table.
//!
//! The table holds both entity and context-source subscriptions; they are
//! discriminated via the `kind` column. Rows are produced by hand-assembled
//! query readers rather than `sqlx::FromRow` because the `dataset_id` column
//! is only present for entity subscriptions (see [`DatasetIdPresence`]).
//!
//! [`DatasetIdPresence`]: crate::subscription_store::params::DatasetIdPresence

use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use chrono::{DateTime, Utc};
use serde_json::Value;

/// A subscription row read from the `subscriptions` table. Column presence
/// matches the macro-generated projection list; optional/nullable columns
/// remain `Option`.
#[derive(Debug)]
pub struct SubscriptionRow {
    /// Subscription identifier.
    pub id: String,
    /// Optional human-readable name.
    pub subscription_name: Option<String>,
    /// Optional free-text description.
    pub description: Option<String>,
    /// Entity selector JSON (nullable in DB).
    pub entities_json: Option<Value>,
    /// Watched attribute IRIs.
    pub watched_attributes: Option<Vec<String>>,
    /// Notification trigger strings (parsed by the mapper).
    pub notification_trigger_strs: Option<Vec<String>>,
    /// Periodic notification interval seconds.
    pub time_interval: Option<f64>,
    /// NGSI-LD q expression.
    pub q: Option<String>,
    /// Geo query JSON (nullable).
    pub geo_q: Option<Value>,
    /// Scope query expression.
    pub scope_q: Option<String>,
    /// Context source filter.
    pub csf: Option<String>,
    /// Dataset IDs (entity subscriptions only; `None` when absent).
    pub dataset_id: Option<Vec<String>>,
    /// Whether the subscription is active.
    pub is_active: bool,
    /// Notification endpoint configuration JSON.
    pub notification_json: Value,
    /// Expiry timestamp.
    pub expires_at: Option<DateTime<Utc>>,
    /// Throttling seconds.
    pub throttling: Option<f64>,
    /// Current lifecycle status.
    pub status: SubscriptionStatus,
    /// Temporal query JSON.
    pub temporal_q: Option<Value>,
    /// Preferred language.
    pub lang: Option<String>,
    /// Associated JSON-LD context URI.
    pub jsonld_context: Option<String>,
    /// Local-only scope flag.
    pub local_only: bool,
    /// Counter for notifications sent.
    pub times_sent: i32,
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
