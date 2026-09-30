//! The subscription store port, and the field sets it takes.
//!
//! One contract serves both entity subscriptions (clause 5.8) and
//! context-source subscriptions (clause 5.11): the two differ in what they
//! select and how they are notified, not in how they are stored.

use crate::{
    error::SubscriptionError,
    notification_slot::{NotificationSlot, NotificationSlotGate},
    stored_subscription::StoredSubscription,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::{
    notification::NotificationTrigger,
    subscription::{EntitySelector, NotificationParams, Subscription, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::{cursor::PageCursor, dataset::DatasetSelector, filter::expression::Expr, scope::query::ScopeQuery};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use langtag::LangTagBuf;
use serde_json::Value;

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
    /// The `datasetId` selectors of the Attribute instances notified
    /// (Table 5.2.12-1), `@none` naming the default instance.
    pub dataset_id: Option<&'a [DatasetSelector]>,
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
    /// Expanded attribute names whose `q` operands were coerced through the
    /// `@context` (Table 5.2.12-1 `expandValues`).
    pub expand_values: Option<&'a [AttributeName]>,
    /// Expanded attribute names whose `q` object keys were expanded while
    /// their values were left uninterpreted (Table 5.2.12-1 `jsonKeys`).
    pub json_keys: Option<&'a [AttributeName]>,
    /// Preferred language for notifications.
    pub lang: Option<&'a LangTagBuf>,
    /// JSON-LD context URI associated with the subscription.
    pub jsonld_context: Option<&'a IriBuf>,
    /// When true, subscription only matches locally stored entities (clause 5.5.13).
    pub local_only: bool,
    /// Whether entities are assumed split across sources (clause 5.2.12
    /// `splitEntities`); `None` defers to the deployment default.
    pub split_entities: Option<bool>,
    /// The NGSI-LD version a notification's Entities are amended to conform to
    /// (Table 5.2.12-1 `ngsildConformance`); `None` amends nothing.
    pub ngsild_conformance: Option<SpecVersion>,
}

/// Fields that may be present in a subscription update request.
///
/// `Option<Option<T>>` distinguishes "not provided" (outer `None`) from
/// "provided as null" (inner `None`), which clause 5.8.2 needs in order to
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
    fn get<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<Option<StoredSubscription>, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// List subscriptions with pagination, returning the items and total count.
    fn list<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        limit: Option<i64>,
        offset: Option<i64>,
    ) -> BoxFuture<'async_trait, Result<(Vec<Subscription>, i64), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// List subscriptions by keyset over the identifier, for stable pagination
    /// (clause 5.5.9.1).
    ///
    /// `cursor` is the opaque position: `None` is the first page, ascending from
    /// the start; [`PageCursor::After`]/[`PageCursor::Before`] continue the walk
    /// and [`PageCursor::End`] jumps to the final page. The order is the
    /// subscription identifier -- unique, unlike the `createdAt` of the
    /// [`SubscriptionStore::list`] offset listing, and a single-key keyset needs
    /// a unique key. A backward or from-end page is read descending and reversed
    /// to ascending before it is returned. The total is counted as `list` counts
    /// it.
    fn list_keyset<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        cursor: Option<&'life2 PageCursor>,
        limit: i64,
    ) -> BoxFuture<'async_trait, Result<(Vec<Subscription>, i64), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Delete a subscription by identifier. Returns rows affected.
    fn delete<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<u64, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Check whether a subscription with the given identifier exists.
    fn exists<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<bool, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// List all subscriptions with active status, each with the key it is
    /// stored under, because these are the ones a matched event enqueues a
    /// delivery against.
    fn list_active<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
    ) -> BoxFuture<'async_trait, Result<Vec<StoredSubscription>, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Atomically claim a notification slot, gated on the ceilings the policy
    /// layer computed from the subscription's `throttling` and its endpoint's
    /// `cooldown`: each ceiling that is present narrows the claim to a state
    /// row whose corresponding instant is null or at or before it.
    fn claim_notification_slot<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
        gate: NotificationSlotGate,
    ) -> BoxFuture<'async_trait, Result<NotificationSlot, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Record a successful notification delivery.
    fn mark_success<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Record a failed notification delivery.
    fn mark_failure<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Expire subscriptions at or before an explicit execution cutoff and return their identifiers.
    fn expire_due<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        cutoff: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Vec<Identifier>, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Find active interval-based subscriptions due at or before an explicit
    /// execution cutoff.
    ///
    /// The cutoff is the drain pass's own instant rather than the wall clock, so
    /// the set found matches the pass the drain was scheduled for.
    fn find_due_interval_subscriptions<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        cutoff: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Vec<Subscription>, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// The earliest instant strictly after `cutoff` at which some active
    /// interval subscription is next due, or `None` when the family has none.
    ///
    /// The interval drain sleeps until this instant instead of polling, so an
    /// idle tenant issues no periodic-notification query between due times.
    fn next_interval_due<'life0, 'life1, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        cutoff: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Option<DateTime<Utc>>, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait;
    /// Advance one active interval subscription's grid anchor to `cutoff` without
    /// counting a notification, so a subscription that matched nothing this pass
    /// is not re-read every pass.
    fn advance_interval_anchor<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
        cutoff: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        Self: 'async_trait;
    /// Create a new subscription with the given fields and status.
    fn create_subscription<'life0, 'life1, 'life2, 'life3, 'life4, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
        fields: &'life3 SubscriptionFields<'life4>,
        status: SubscriptionStatus,
    ) -> BoxFuture<'async_trait, Result<(), SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        'life4: 'async_trait,
        Self: 'async_trait;
    /// Update an existing subscription. Returns rows affected.
    fn update_subscription<'life0, 'life1, 'life2, 'life3, 'life4, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
        fields: &'life3 SubscriptionFields<'life4>,
        status: SubscriptionStatus,
    ) -> BoxFuture<'async_trait, Result<u64, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        'life4: 'async_trait,
        Self: 'async_trait;
}
