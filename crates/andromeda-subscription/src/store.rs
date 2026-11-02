//! The subscription store port, and the field sets it takes.
//!
//! One contract serves both entity subscriptions (clause 5.8) and
//! context-source subscriptions (clause 5.11): the two differ in what they
//! select and how they are notified, not in how they are stored.

use crate::{
    error::SubscriptionError,
    notification_slot::{NotificationSlot, NotificationSlotGate},
    notifying_since::NotifyingRestart,
    outcome_tally::OutcomeTally,
    stored_subscription::StoredSubscription,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, spec_version::SpecVersion};
use andromeda_ngsi_ld_model::{
    notification::NotificationTrigger,
    query::temporal_query::TemporalQuery,
    subscription::{NotificationParams, Subscription, SubscriptionStatus, entity_selector::EntitySelector},
};
use andromeda_ngsi_ld_query::{
    dataset::DatasetSelector,
    filter::expression::Expr,
    geo::input::GeoQueryInput,
    page_window::PageWindow,
    scope::query::ScopeQuery,
};
use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use iri_rs::IriBuf;
use langtag::LangTagBuf;

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
    /// Geo-spatial query.
    pub geo_q: Option<&'a GeoQueryInput>,
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
    /// Temporal query specification.
    pub temporal_q: Option<&'a TemporalQuery>,
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

/// The members of a subscription request that decide its status.
#[derive(Debug)]
pub struct StatusUpdateFields {
    /// New active/paused flag, if provided in the request.
    pub is_active: Option<bool>,
    /// New expiration timestamp, if provided in the request.
    pub expires_at: Option<DateTime<Utc>>,
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
    /// List the page `window` selects of the subscriptions of this kind, in
    /// identifier order, with the total number of them (clause 6.3.13).
    ///
    /// The collection is ordered by the subscription identifier in either
    /// pagination mode: it is unique, so an offset page is deterministic and a
    /// cursor (clause 5.5.9.1) has a single key to continue from. The page is
    /// returned ascending; a store that reads a backward or from-end page
    /// descending reverses it first.
    fn list<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        window: &'life2 PageWindow,
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
    /// Atomically claim a notification slot of an active subscription, gated
    /// on the ceilings the policy layer computed from the subscription's
    /// `throttling` and its endpoint's `cooldown`: each ceiling that is present
    /// narrows the claim to a state row whose corresponding instant is null or
    /// at or before it.
    ///
    /// A gate stamping at the claim moves `timesSent` and `lastNotification`
    /// with it and answers [`OutcomeRecord::OutcomeOnly`](crate::notification_slot::OutcomeRecord::OutcomeOnly); any other gate
    /// moves nothing and answers [`OutcomeRecord::CountingNotification`](crate::notification_slot::OutcomeRecord::CountingNotification) with
    /// the claim's instant, so the outcome counts the notification.
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
    /// Record the outcomes `tally` adds up on the subscription in one write,
    /// whatever the subscription's status is by then.
    fn record_outcomes<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
        tally: OutcomeTally,
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
    ) -> BoxFuture<'async_trait, Result<Vec<StoredSubscription>, SubscriptionError>>
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
    /// Record `cutoff` as the sweep instant one active interval subscription
    /// answered its grid at without a notification: it matched nothing, so it
    /// is not re-read before its next grid point and its `lastNotification`
    /// stays the instant of the last notification actually sent (Table
    /// 5.2.14.2-1).
    fn record_interval_tick<'life0, 'life1, 'life2, 'async_trait>(
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
    /// Create a new subscription with the given fields and status, both
    /// instants of its [`NotifyingSince`](crate::notifying_since::NotifyingSince)
    /// the creating transaction's timestamp.
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
    /// Update an existing subscription, restarting at the update's transaction
    /// timestamp each instant `restart` names. Returns rows affected.
    fn update_subscription<'life0, 'life1, 'life2, 'life3, 'life4, 'async_trait>(
        &'life0 self,
        target: &'life1 ContextTarget,
        id: &'life2 Identifier,
        fields: &'life3 SubscriptionFields<'life4>,
        status: SubscriptionStatus,
        restart: NotifyingRestart,
    ) -> BoxFuture<'async_trait, Result<u64, SubscriptionError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
        'life4: 'async_trait,
        Self: 'async_trait;
}
