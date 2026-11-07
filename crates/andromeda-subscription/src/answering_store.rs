//! A subscription store answering only `get`, for tests of code that reads a
//! subscription back.

use crate::{
    error::SubscriptionError,
    notification_slot::{NotificationSlot, NotificationSlotGate},
    notifying_since::{NotifyingRestart, NotifyingSince},
    outcome_tally::OutcomeTally,
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicU32, Ordering};

/// What the store double answers when a subscription is read back.
#[derive(Clone, Copy, Debug)]
pub enum StoredAnswer {
    /// The store fails the read, as an unreachable database would.
    Unreadable,
    /// The store holds no such subscription.
    Absent,
    /// The store fails this many reads, then holds the subscription.
    UnreadableForReads(u32),
    /// The store holds a row that no longer reads back as a subscription.
    UndecodableRow,
}

/// A subscription store that answers `get` as `answer` says and counts the
/// reads; every other operation is refused as a saturated store.
#[derive(Debug)]
pub struct AnsweringStore {
    /// How every `get` is answered.
    answer: StoredAnswer,
    /// The `get` calls answered so far.
    reads: AtomicU32,
}

impl AnsweringStore {
    /// A store answering every read as `answer` says.
    #[must_use]
    pub const fn new(answer: StoredAnswer) -> AnsweringStore {
        AnsweringStore {
            answer,
            reads: AtomicU32::new(0),
        }
    }

    /// How many reads the store has been asked for.
    #[must_use]
    pub fn reads(&self) -> u32 {
        self.reads.load(Ordering::Relaxed)
    }
}

/// A stored Vehicle subscription named `id` that is active and never expires.
///
/// The fixture fails the way a store decoding a bad row does, so a key or body
/// the store would refuse reads as undecodable rather than panicking.
fn stored(id: &Identifier) -> Result<StoredSubscription, SubscriptionError> {
    let text = format!(
        r#"{{"id":"{id}","type":"Subscription","entities":[{{"type":"https://example.org/Vehicle"}}],"notification":{{"endpoint":{{"uri":"http://subscriber.example.org/notify","accept":"application/json"}}}},"status":"active","createdAt":"2024-01-01T00:00:00Z","modifiedAt":"2024-01-01T00:00:00Z"}}"#
    );
    Ok(StoredSubscription {
        key: SubscriptionKey::try_from(1).map_err(|_| SubscriptionError::from(PersistenceFailure::Undecodable))?,
        subscription: serde_json::from_str(&text)?,
        notifying_since: NotifyingSince {
            active: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
            selected: StoreInstant::new(DateTime::<Utc>::MIN_UTC),
        },
    })
}

#[async_trait]
impl SubscriptionStore for AnsweringStore {
    async fn get(&self, _target: &ContextTarget, id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        let read = self.reads.fetch_add(1, Ordering::Relaxed);
        match self.answer {
            StoredAnswer::Unreadable => Err(SubscriptionError::from(PersistenceFailure::Unavailable)),
            StoredAnswer::UnreadableForReads(failing) if read < failing => Err(SubscriptionError::from(PersistenceFailure::Unavailable)),
            StoredAnswer::UndecodableRow => serde_json::from_str::<u8>("not a number")
                .map(|_| None)
                .map_err(SubscriptionError::Representation),
            StoredAnswer::Absent => Ok(None),
            StoredAnswer::UnreadableForReads(_) => stored(id).map(Some),
        }
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

    async fn record_outcomes(&self, _target: &ContextTarget, _id: &Identifier, _tally: OutcomeTally) -> Result<(), SubscriptionError> {
        Err(SubscriptionError::StoreSaturated)
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
