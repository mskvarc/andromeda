//! The subscription store port, and the field sets it takes.
//!
//! One contract serves both entity subscriptions (clause 5.8) and
//! context-source subscriptions (clause 5.11): the two differ in what they
//! select and how they are notified, not in how they are stored.

use crate::core::error::AndromedaError;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

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
    /// When true, subscription only matches locally stored entities (clause 5.5.13).
    pub local_only: bool,
}

/// Fields that may be present in a subscription update request.
///
/// `Option<Option<T>>` distinguishes "not provided" (outer `None`) from
/// "provided as null" (inner `None`), which clause 5.8.4 needs in order to
/// clear an `expiresAt` rather than leave it alone.
#[derive(Debug)]
pub struct StatusUpdateFields {
    /// New active/paused flag, if provided in the update request.
    pub is_active: Option<bool>,
    /// New expiration timestamp; outer `None` means not provided, inner `None` means cleared.
    pub expires_at: Option<Option<DateTime<Utc>>>,
}

/// Storing subscriptions, and the lifecycle bookkeeping the notification path
/// writes back to them.
#[async_trait]
pub trait SubscriptionStore: Send + Sync {
    /// Retrieve a subscription by identifier.
    async fn get(&self, id: &str) -> Result<Option<Subscription>, AndromedaError>;
    /// List subscriptions with pagination, returning the items and total count.
    async fn list(&self, limit: Option<i64>, offset: Option<i64>) -> Result<(Vec<Subscription>, i64), AndromedaError>;
    /// Delete a subscription by identifier. Returns rows affected.
    async fn delete(&self, id: &str) -> Result<u64, AndromedaError>;
    /// Check whether a subscription with the given identifier exists.
    async fn exists(&self, id: &str) -> Result<bool, AndromedaError>;
    /// List all subscriptions with active status.
    async fn list_active(&self) -> Result<Vec<Subscription>, AndromedaError>;
    /// Atomically claim a notification slot. `earliest_allowed_last_notification`,
    /// when provided, gates the claim on `last_notification IS NULL OR
    /// last_notification <= $2`. The policy layer computes this timestamp from
    /// the subscription's throttling configuration.
    async fn claim_notification_slot(&self, id: &str, earliest_allowed_last_notification: Option<DateTime<Utc>>) -> Result<bool, AndromedaError>;
    /// Record a successful notification delivery.
    async fn mark_success(&self, id: &str) -> Result<(), AndromedaError>;
    /// Record a failed notification delivery.
    async fn mark_failure(&self, id: &str) -> Result<(), AndromedaError>;
    /// Expire subscriptions whose `expiresAt` has passed. Returns the identifiers.
    async fn expire_due(&self) -> Result<Vec<String>, AndromedaError>;
    /// Find active interval-based subscriptions that are due for their next notification.
    async fn find_due_interval_subscriptions(&self) -> Result<Vec<Subscription>, AndromedaError>;
    /// Create a new subscription with the given fields and status.
    async fn create_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), AndromedaError>;
    /// Update an existing subscription. Returns rows affected.
    async fn update_subscription(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<u64, AndromedaError>;
}
