//! A subscription store that records every outcome write it is asked for, and
//! holds the writes of one subscription until a test releases them, so a test
//! can tell which outcomes one write carried.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_subscription::{
    error::SubscriptionError,
    notification_slot::{NotificationSlot, NotificationSlotGate},
    notifying_since::NotifyingRestart,
    outcome_tally::OutcomeTally,
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::Mutex;
use tokio::sync::{Notify, Semaphore};

/// Whether the store answers an outcome write as written or as failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteAnswer {
    /// Every write is recorded and answered as written.
    Written,
    /// Every write is recorded and answered as a saturated store.
    Saturated,
}

/// The store; every operation other than recording outcomes answers a
/// saturated store.
pub(crate) struct OutcomeRecordingStore {
    /// The writes asked for, in the order they started.
    writes: Mutex<Vec<(Identifier, OutcomeTally)>>,
    /// The subscription whose writes wait for [`OutcomeRecordingStore::release`].
    held: Option<Identifier>,
    /// One permit per held write let through.
    released: Semaphore,
    /// Signalled when a held write starts waiting.
    held_write_started: Notify,
    /// How every write is answered.
    answer: WriteAnswer,
}

impl OutcomeRecordingStore {
    /// A store whose writes for `held` wait to be released, and every write
    /// answered with `answer`.
    pub(crate) fn new(held: Option<Identifier>, answer: WriteAnswer) -> OutcomeRecordingStore {
        OutcomeRecordingStore {
            writes: Mutex::new(Vec::new()),
            held,
            released: Semaphore::new(0),
            held_write_started: Notify::new(),
            answer,
        }
    }

    /// The writes asked for so far.
    pub(crate) fn writes(&self) -> Vec<(Identifier, OutcomeTally)> {
        self.writes.lock().expect("the write record is never poisoned").clone() // clone: the assertion reads a snapshot
    }

    /// Wait until a held write has started.
    pub(crate) async fn held_write_started(&self) {
        self.held_write_started.notified().await;
    }

    /// Let `writes` held writes through.
    pub(crate) fn release(&self, writes: usize) {
        self.released.add_permits(writes);
    }
}

#[async_trait]
impl SubscriptionStore for OutcomeRecordingStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn exists(&self, _target: &ContextTarget, _id: &Identifier) -> Result<bool, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn list_active(&self, _target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn claim_notification_slot(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _gate: NotificationSlotGate,
    ) -> Result<NotificationSlot, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn record_outcomes(&self, _target: &ContextTarget, id: &Identifier, tally: OutcomeTally) -> Result<(), SubscriptionError> {
        self.writes.lock().expect("the write record is never poisoned").push((id.clone(), tally)); // clone: the record owns what it saw
        if self.held.as_ref() == Some(id) {
            self.held_write_started.notify_one();
            self.released.acquire().await.expect("the semaphore is never closed").forget();
        }
        match self.answer {
            WriteAnswer::Written => Ok(()),
            WriteAnswer::Saturated => Err(SubscriptionError::StoreSaturated),
        }
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn record_interval_tick(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn create_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<(), SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }

    async fn update_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
        _restart: NotifyingRestart,
    ) -> Result<u64, SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
    }
}
