//! The `@context` of a notification whose slot is already claimed.
//!
//! The periodic senders (clause 5.8.6 `timeInterval`, clause 5.11.7) claim a
//! subscription's notification slot before they render the notification, so a
//! `jsonldContext` found unreadable afterwards is a notification that failed:
//! it is recorded as one, so the subscription's `lastFailure` and
//! `timesFailed` report it and its endpoint's `cooldown` applies, exactly as a
//! failed delivery would.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::notification_context::NotificationContext;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::{
    notification_slot::{NotificationOutcome, OutcomeRecord},
    store::SubscriptionStore,
};
use iri_rs::IriBuf;
use tracing::{debug, warn};

/// The notification `@context` of the subscription `subscription_id`, whose
/// `jsonldContext` is `jsonld_context` and whose slot on `target` is claimed,
/// its outcome to be recorded with `record`.
///
/// Answers `None` when the `jsonldContext` names a document that cannot be
/// loaded (clause 5.5.6), after recording the claimed notification as failed.
pub(crate) async fn claimed_notification_context(
    engine: &JsonLdEngine,
    jsonld_context: Option<&IriBuf>,
    sub_repo: &dyn SubscriptionStore,
    target: &ContextTarget,
    subscription_id: &Identifier,
    record: OutcomeRecord,
) -> Option<NotificationContext> {
    match NotificationContext::resolve(jsonld_context, engine).await {
        Ok(context) => Some(context),
        Err(error) => {
            warn!(subscription = %subscription_id, %error, "the jsonldContext of the subscription cannot be loaded");
            if let Err(error) = sub_repo.record_outcome(target, subscription_id, NotificationOutcome::Failure, record).await {
                debug!(subscription = %subscription_id, %error, "failed to record the notification failure");
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::claimed_notification_context::claimed_notification_context;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::engine::processor::JsonLdEngine;
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
    use iri_rs::IriBuf;
    use std::sync::Mutex;

    /// A store whose only exercised operation is `record_outcome`, which it
    /// records; every other operation answers a saturated store.
    #[derive(Default)]
    struct FailureRecordingStore {
        failures: Mutex<Vec<(Identifier, NotificationOutcome, OutcomeRecord)>>,
    }

    impl FailureRecordingStore {
        fn failures(&self) -> Vec<(Identifier, NotificationOutcome, OutcomeRecord)> {
            self.failures.lock().expect("the failure record is never poisoned").clone() // clone: the assertion reads a snapshot
        }
    }

    #[async_trait]
    impl SubscriptionStore for FailureRecordingStore {
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

        async fn record_outcome(
            &self,
            _target: &ContextTarget,
            id: &Identifier,
            outcome: NotificationOutcome,
            record: OutcomeRecord,
        ) -> Result<(), SubscriptionError> {
            self.failures
                .lock()
                .expect("the failure record is never poisoned")
                .push((id.clone(), outcome, record)); // clone: the record owns what it saw
            Ok(())
        }

        async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
            Err(SubscriptionError::StoreSaturated)
        }

        async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Subscription>, SubscriptionError> {
            Err(SubscriptionError::StoreSaturated)
        }

        async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
            Err(SubscriptionError::StoreSaturated)
        }

        async fn advance_interval_anchor(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
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
        ) -> Result<u64, SubscriptionError> {
            Err(SubscriptionError::StoreSaturated)
        }
    }

    fn offline_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds")
    }

    fn subscription() -> Identifier {
        "urn:ngsi-ld:Subscription:periodic".parse().expect("a valid identifier")
    }

    fn counting_record() -> OutcomeRecord {
        OutcomeRecord::CountingNotification {
            notified_at: DateTime::<Utc>::UNIX_EPOCH,
        }
    }

    /// Clause 5.5.6: a claimed notification whose `jsonldContext` cannot be
    /// loaded is not sent, and it is recorded on the subscription as a failed
    /// notification, with what its claim left the outcome to record.
    #[tokio::test]
    async fn an_unloadable_jsonld_context_records_the_claimed_notification_as_failed() {
        let store = FailureRecordingStore::default();
        let unreachable = IriBuf::new("https://example.com/unreachable.jsonld".to_owned()).expect("a valid IRI");

        let context = claimed_notification_context(
            &offline_engine(),
            Some(&unreachable),
            &store,
            &ContextTarget::Live,
            &subscription(),
            counting_record(),
        )
        .await;

        assert!(context.is_none());
        pretty_assertions::assert_eq!(store.failures(), vec![(subscription(), NotificationOutcome::Failure, counting_record())]);
    }

    /// A readable `jsonldContext` records nothing: the delivery outcome is
    /// recorded once the notification is sent.
    #[tokio::test]
    async fn a_readable_jsonld_context_records_nothing() {
        let store = FailureRecordingStore::default();

        let context = claimed_notification_context(&offline_engine(), None, &store, &ContextTarget::Live, &subscription(), counting_record()).await;

        assert!(context.is_some());
        pretty_assertions::assert_eq!(store.failures(), Vec::new());
    }
}
