//! Repository input parameters for subscription persistence.

use chrono::{DateTime, Utc};
use serde_json::Value;

/// Whether the subscription table includes a `dataset_id` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetIdPresence {
    /// The `dataset_id` column exists and should be read.
    Present,
    /// The `dataset_id` column does not exist in this table.
    Absent,
}

/// Borrowed field values used when creating or updating a subscription.
#[derive(Debug)]
pub struct SubscriptionFields<'a> {
    /// Human-readable name of the subscription.
    pub subscription_name: Option<&'a str>,
    /// Free-text description of the subscription.
    pub description: Option<&'a str>,
    /// Entity selector information as a JSON value.
    pub entities: Option<&'a Value>,
    /// Attribute IRIs that trigger notifications when changed.
    pub watched_attributes: Option<&'a [String]>,
    /// Events that trigger a notification (e.g., entityCreated).
    pub notification_trigger: Option<&'a [String]>,
    /// Periodic notification interval in seconds.
    pub time_interval: Option<f64>,
    /// NGSI-LD query expression for filtering entities.
    pub q: Option<&'a str>,
    /// Geo-spatial query as a JSON value.
    pub geo_q: Option<&'a Value>,
    /// Scope query expression.
    pub scope_q: Option<&'a str>,
    /// Context source filter expression.
    pub csf: Option<&'a str>,
    /// Dataset identifiers to restrict matching.
    pub dataset_id: Option<&'a [String]>,
    /// Whether the subscription is currently active.
    pub is_active: bool,
    /// Notification endpoint and format configuration as a JSON value.
    pub notification: Option<&'a Value>,
    /// Timestamp after which the subscription expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// Minimum interval in seconds between consecutive notifications.
    pub throttling: Option<f64>,
    /// Temporal query specification as a JSON value.
    pub temporal_q: Option<&'a Value>,
    /// Preferred language for notifications.
    pub lang: Option<&'a str>,
    /// JSON-LD context URI associated with the subscription.
    pub jsonld_context: Option<&'a str>,
    /// When true, subscription only matches locally stored entities (§5.5.13).
    pub local_only: bool,
}

/// Fields that may be present in a subscription update request.
/// Uses `Option<Option<T>>` to distinguish "not provided" (`None`) from "provided" (`Some`).
#[derive(Debug)]
pub struct StatusUpdateFields {
    /// New active/paused flag, if provided in the update request.
    pub is_active: Option<bool>,
    /// New expiration timestamp; outer `None` means not provided, inner `None` means cleared.
    pub expires_at: Option<Option<DateTime<Utc>>>,
}
