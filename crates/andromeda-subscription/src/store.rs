//! The subscription store port, and the field sets it takes.
//!
//! One contract serves both entity subscriptions (clause 5.8) and
//! context-source subscriptions (clause 5.11): the two differ in what they
//! select and how they are notified, not in how they are stored.

use crate::{error::SubscriptionError, stored_subscription::StoredSubscription};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    notification::NotificationTrigger,
    subscription::{EntitySelector, NotificationParams, Subscription, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::{filter::expression::Expr, scope::query::ScopeQuery};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use langtag::LangTagBuf;
use serde_json::Value;
use url::Url;

/// Borrowed field values used when creating or updating a subscription.
#[derive(Debug)]
pub struct SubscriptionFields<'a> {
    /// Human-readable name of the subscription.
    pub subscription_name: Option<&'a str>,
    /// Free-text description of the subscription.
    pub description: Option<&'a str>,
    /// Validated entity selectors.
    pub entities: Option<&'a [EntitySelector]>,
    /// Attribute IRIs that trigger notifications when changed.
    pub watched_attributes: Option<&'a [AttributeName]>,
    /// Events that trigger a notification (e.g., entityCreated).
    pub notification_trigger: Option<&'a [NotificationTrigger]>,
    /// Periodic notification interval in seconds.
    pub time_interval: Option<f64>,
    /// NGSI-LD query expression for filtering entities.
    pub q: Option<&'a Expr>,
    /// Geo-spatial query as a JSON value.
    pub geo_q: Option<&'a Value>,
    /// Scope query expression.
    pub scope_q: Option<&'a ScopeQuery>,
    /// Context source filter expression.
    pub csf: Option<&'a Expr>,
    /// Dataset identifiers to restrict matching.
    pub dataset_id: Option<&'a [Uri]>,
    /// Whether the subscription is currently active.
    pub is_active: bool,
    /// Validated notification endpoint and format configuration.
    pub notification: Option<&'a NotificationParams>,
    /// Timestamp after which the subscription expires.
    pub expires_at: Option<DateTime<Utc>>,
    /// Minimum interval in seconds between consecutive notifications.
    pub throttling: Option<f64>,
    /// Temporal query specification as a JSON value.
    pub temporal_q: Option<&'a Value>,
    /// Preferred language for notifications.
    pub lang: Option<&'a LangTagBuf>,
    /// JSON-LD context URI associated with the subscription.
    pub jsonld_context: Option<&'a Url>,
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
pub trait SubscriptionStore: Send + Sync + 'static {
    /// Retrieve a subscription by identifier, with the key it is stored under.
    fn get<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<StoredSubscription>, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// List subscriptions with pagination, returning the items and total count.
    fn list<'life0, 'async_trait>(
        &'life0 self,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> BoxFuture<'async_trait, Result<(Vec<Subscription>, i64), SubscriptionError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
    /// Delete a subscription by identifier. Returns rows affected.
    fn delete<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<u64, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Check whether a subscription with the given identifier exists.
    fn exists<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<bool, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// List all subscriptions with active status, each with the key it is
    /// stored under, because these are the ones a matched event enqueues a
    /// delivery against.
    fn list_active<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<StoredSubscription>, SubscriptionError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
    /// Atomically claim a notification slot. `earliest_allowed_last_notification`,
    /// when provided, gates the claim on `last_notification IS NULL OR
    /// last_notification <= $2`. The policy layer computes this timestamp from
    /// the subscription's throttling configuration.
    fn claim_notification_slot<'life0, 'life1, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        earliest_allowed_last_notification: Option<DateTime<Utc>>,
    ) -> BoxFuture<'async_trait, Result<bool, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Record a successful notification delivery.
    fn mark_success<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Record a failed notification delivery.
    fn mark_failure<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Expire subscriptions at or before an explicit execution cutoff and return their identifiers.
    fn expire_due<'life0, 'async_trait>(&'life0 self, cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<Vec<Uri>, SubscriptionError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
    /// Find active interval-based subscriptions that are due for their next notification.
    fn find_due_interval_subscriptions<'life0, 'async_trait>(&'life0 self) -> BoxFuture<'async_trait, Result<Vec<Subscription>, SubscriptionError>>
    where
        'life0: 'async_trait,
        Self: 'async_trait;
    /// Create a new subscription with the given fields and status.
    fn create_subscription<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        fields: &'life2 SubscriptionFields<'life3>,
        status: SubscriptionStatus,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;
    /// Update an existing subscription. Returns rows affected.
    fn update_subscription<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        id: &'life1 Uri,
        fields: &'life2 SubscriptionFields<'life3>,
        status: SubscriptionStatus,
    ) -> BoxFuture<'async_trait, Result<u64, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        Self: 'async_trait;
}
