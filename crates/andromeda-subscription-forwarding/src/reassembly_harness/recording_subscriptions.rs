//! The subscription store a re-assembly test runs against: it holds the one
//! local subscription and records what the re-assembly writes back to it.

use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_subscription::{
    error::SubscriptionError,
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The surrogate key the one stored subscription is stored under.
const KEY: i64 = 1;

/// A store holding one subscription, whose notification slot is always free
/// and whose claims and successful deliveries are counted.
pub(crate) struct RecordingSubscriptions {
    /// The subscription in the form the store keeps it.
    document: Value,
    /// How many notification slots were claimed.
    claims: AtomicUsize,
    /// How many deliveries were recorded as successful.
    successes: AtomicUsize,
}

impl RecordingSubscriptions {
    /// A store holding the subscription `document` describes.
    pub(crate) const fn holding(document: Value) -> RecordingSubscriptions {
        RecordingSubscriptions {
            document,
            claims: AtomicUsize::new(0),
            successes: AtomicUsize::new(0),
        }
    }

    /// How many notification slots were claimed.
    pub(crate) fn claims(&self) -> usize {
        self.claims.load(Ordering::SeqCst)
    }

    /// How many deliveries were recorded as successful.
    pub(crate) fn successes(&self) -> usize {
        self.successes.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SubscriptionStore for RecordingSubscriptions {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        let subscription: Subscription = serde_json::from_value(self.document.clone())?; // clone: each read decodes its own copy of the stored form
        let key = SubscriptionKey::try_from(KEY).map_err(|_unrepresentable| SubscriptionError::StoreUnavailable)?;
        Ok((subscription.id == *id).then_some(StoredSubscription { key, subscription }))
    }

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn exists(&self, _target: &ContextTarget, _id: &Identifier) -> Result<bool, SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn list_active(&self, _target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn claim_notification_slot(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _gate: NotificationSlotGate,
    ) -> Result<NotificationSlot, SubscriptionError> {
        self.claims.fetch_add(1, Ordering::SeqCst);
        Ok(NotificationSlot::Claimed(OutcomeRecord::OutcomeOnly))
    }

    async fn record_outcome(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        outcome: NotificationOutcome,
        _record: OutcomeRecord,
    ) -> Result<(), SubscriptionError> {
        match outcome {
            NotificationOutcome::Success => {
                self.successes.fetch_add(1, Ordering::SeqCst);
            }
            NotificationOutcome::Failure => {}
        }
        Ok(())
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn record_interval_tick(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn create_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<(), SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn update_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<u64, SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }
}
