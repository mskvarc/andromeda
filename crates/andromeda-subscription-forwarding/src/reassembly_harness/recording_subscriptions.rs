//! The subscription store a re-assembly test runs against: it holds the one
//! local subscription and the companions mirroring it, records what the
//! re-assembly writes back, and can hold a deletion until a test releases it.

use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_subscription::{
    error::SubscriptionError,
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
    notifying_since::{NotifyingRestart, NotifyingSince},
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::{sync::Notify, task::yield_now};

/// The surrogate key the one stored subscription is stored under.
const KEY: i64 = 1;

/// The instant every document the store writes was created and modified at.
const WRITTEN_AT: &str = "2020-01-01T00:00:00Z";

/// A deletion held until the test releases it.
#[derive(Default)]
struct DeletionHold {
    /// Whether the next deletions wait for the release.
    armed: AtomicBool,
    /// Signalled when a held deletion has started.
    reached: Notify,
    /// Signalled by the test to let a held deletion finish.
    released: Notify,
}

/// A store holding one local subscription and its companions, whose
/// notification slot is always free, whose claims and successful deliveries
/// are counted, and which records every subscription deleted from it and how
/// many companion updates were in flight at once.
pub(crate) struct RecordingSubscriptions {
    /// Every subscription stored, in the form the store keeps it.
    documents: Mutex<Vec<Value>>,
    /// How many notification slots were claimed.
    claims: AtomicUsize,
    /// How many deliveries were recorded as successful.
    successes: AtomicUsize,
    /// The subscriptions deleted, in the order they were.
    deleted: Mutex<Vec<Identifier>>,
    /// The deletion a test holds between its start and its effect.
    hold: DeletionHold,
    /// How many updates are running.
    updates_in_flight: AtomicUsize,
    /// The most updates that ran at once.
    most_updates_at_once: AtomicUsize,
}

impl RecordingSubscriptions {
    /// A store holding the subscriptions `documents` describe.
    pub(crate) fn holding(documents: Vec<Value>) -> RecordingSubscriptions {
        RecordingSubscriptions {
            documents: Mutex::new(documents),
            claims: AtomicUsize::new(0),
            successes: AtomicUsize::new(0),
            deleted: Mutex::new(Vec::new()),
            hold: DeletionHold::default(),
            updates_in_flight: AtomicUsize::new(0),
            most_updates_at_once: AtomicUsize::new(0),
        }
    }

    /// Store `document` in place of the subscription carrying its identifier.
    pub(crate) fn replace(&self, document: Value) {
        let mut documents = self.documents.lock().expect("the documents are never poisoned");
        documents.retain(|stored| stored["id"] != document["id"]);
        documents.push(document);
    }

    /// The document stored under `id`.
    fn document(&self, id: &Identifier) -> Option<Value> {
        self.documents
            .lock()
            .expect("the documents are never poisoned")
            .iter()
            .find(|document| document["id"] == id.as_str())
            .cloned() // clone: each read decodes its own copy of the stored form
    }

    /// Whether a subscription is stored under `id`.
    pub(crate) fn holds(&self, id: &Identifier) -> bool {
        self.document(id).is_some()
    }

    /// Hold every deletion from now on until [`Self::release_deletions`].
    pub(crate) fn hold_deletions(&self) {
        self.hold.armed.store(true, Ordering::SeqCst);
    }

    /// Wait until a held deletion has started.
    pub(crate) async fn deletion_reached(&self) {
        self.hold.reached.notified().await;
    }

    /// Let the held deletion finish, and the deletions after it run at once.
    pub(crate) fn release_deletions(&self) {
        self.hold.armed.store(false, Ordering::SeqCst);
        self.hold.released.notify_one();
    }

    /// The most updates that ran at once.
    pub(crate) fn most_updates_at_once(&self) -> usize {
        self.most_updates_at_once.load(Ordering::SeqCst)
    }

    /// The subscriptions deleted, in the order they were.
    pub(crate) fn deleted(&self) -> Vec<Identifier> {
        self.deleted.lock().expect("the deletion record is never poisoned").clone() // clone: the caller owns its snapshot
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
        let Some(document) = self.document(id) else {
            return Ok(None);
        };
        let subscription: Subscription = serde_json::from_value(document)?;
        let key = SubscriptionKey::try_from(KEY).map_err(|_unrepresentable| SubscriptionError::StoreUnavailable)?;
        let since = StoreInstant::new(subscription.lifecycle.created_at);
        let notifying_since = NotifyingSince {
            active: since,
            selected: since,
        };
        Ok(Some(StoredSubscription {
            key,
            subscription,
            notifying_since,
        }))
    }

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(SubscriptionError::StoreUnavailable)
    }

    async fn delete(&self, _target: &ContextTarget, id: &Identifier) -> Result<u64, SubscriptionError> {
        if self.hold.armed.load(Ordering::SeqCst) {
            self.hold.reached.notify_one();
            self.hold.released.notified().await;
        }
        self.deleted.lock().expect("the deletion record is never poisoned").push(id.clone()); // clone: the record owns the identifier
        let mut documents = self.documents.lock().expect("the documents are never poisoned");
        let before = documents.len();
        documents.retain(|document| document["id"] != id.as_str());
        Ok(u64::from(documents.len() < before))
    }

    async fn exists(&self, _target: &ContextTarget, id: &Identifier) -> Result<bool, SubscriptionError> {
        Ok(self.holds(id))
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

    /// Stores the selection of the subscription created, which is all a
    /// companion is matched against registrations by.
    async fn create_subscription(
        &self,
        _target: &ContextTarget,
        id: &Identifier,
        fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<(), SubscriptionError> {
        let mut document = json!({
            "id": id.as_str(),
            "type": "Subscription",
            "entities": fields.entities,
            "notification": {"endpoint": {"uri": "http://broker.example:1056/ngsi-ld/v1/notifications"}},
            "status": "active",
            "createdAt": WRITTEN_AT,
            "modifiedAt": WRITTEN_AT
        });
        if let Some(watched) = fields.watched_attributes {
            document["watchedAttributes"] = json!(watched);
        }
        self.replace(document);
        Ok(())
    }

    /// Counts the updates running at once, and yields once while it runs so
    /// that a second update started beside it can begin before it ends. The
    /// stored document is left as it was.
    async fn update_subscription(
        &self,
        _target: &ContextTarget,
        id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
        _restart: NotifyingRestart,
    ) -> Result<u64, SubscriptionError> {
        if !self.holds(id) {
            return Ok(0);
        }
        let running = self.updates_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_updates_at_once.fetch_max(running, Ordering::SeqCst);
        yield_now().await;
        self.updates_in_flight.fetch_sub(1, Ordering::SeqCst);
        Ok(1)
    }
}
