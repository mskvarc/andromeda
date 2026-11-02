//! The `@context` a notification is rendered under.
//!
//! A `jsonldContext` found unreadable names a notification that failed, so it
//! is recorded as one: the subscription's `lastFailure` and `timesFailed`
//! report it, exactly as a failed delivery would. The periodic senders
//! (clause 5.8.6 `timeInterval`, clause 5.11.7) resolve after claiming the
//! slot and record with what the claim left to record. The on-change sender
//! resolves before claiming, so a notification it cannot build takes no
//! throttling slot, and the failure is recorded only when the subscription
//! would have been allowed to send it.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::notification_context::NotificationContext;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_subscription::{
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
    outcome_tally::OutcomeTally,
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
            record_failure(sub_repo, target, subscription_id, record).await;
            None
        }
    }
}

/// What resolving the context of a notification whose slot is not yet claimed
/// came to.
pub(crate) enum UnclaimedContext {
    /// The context is loaded; the caller goes on to claim the slot.
    Resolved(NotificationContext),
    /// The context cannot be loaded and the failure is recorded.
    Failed,
    /// The context cannot be loaded, but the gate would have refused the
    /// notification anyway, so nothing is recorded.
    Suppressed,
}

/// The notification `@context` of the subscription `subscription_id`, whose
/// `jsonldContext` is `jsonld_context`, before its slot on `target` is
/// claimed under `gate`.
///
/// A `jsonldContext` that cannot be loaded (clause 5.5.6) is recorded as a
/// failed notification only when `gate`, with its stamping removed, admits
/// one: clause 5.8.6 updates `lastFailure` for a notification that was sent,
/// and a `throttling` window or `cooldown` stops one from being sent. The slot
/// is never taken.
pub(crate) async fn unclaimed_notification_context(
    engine: &JsonLdEngine,
    jsonld_context: Option<&IriBuf>,
    sub_repo: &dyn SubscriptionStore,
    target: &ContextTarget,
    subscription_id: &Identifier,
    gate: NotificationSlotGate,
) -> UnclaimedContext {
    match NotificationContext::resolve(jsonld_context, engine).await {
        Ok(context) => UnclaimedContext::Resolved(context),
        Err(error) => {
            warn!(subscription = %subscription_id, %error, "the jsonldContext of the subscription cannot be loaded");
            match sub_repo.claim_notification_slot(target, subscription_id, gate.unstamped()).await {
                Ok(NotificationSlot::Claimed(_)) => {
                    record_failure(sub_repo, target, subscription_id, OutcomeRecord::OutcomeOnly).await;
                    UnclaimedContext::Failed
                }
                Ok(NotificationSlot::Suppressed) => UnclaimedContext::Suppressed,
                Err(error) => {
                    debug!(subscription = %subscription_id, %error, "failed to probe the notification slot");
                    UnclaimedContext::Failed
                }
            }
        }
    }
}

/// Record the notification of `subscription_id` as failed with `record`.
async fn record_failure(sub_repo: &dyn SubscriptionStore, target: &ContextTarget, subscription_id: &Identifier, record: OutcomeRecord) {
    if let Err(error) = sub_repo
        .record_outcomes(target, subscription_id, OutcomeTally::new(NotificationOutcome::Failure, record))
        .await
    {
        debug!(subscription = %subscription_id, %error, "failed to record the notification failure");
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::resolved_notification_context::{UnclaimedContext, claimed_notification_context, unclaimed_notification_context};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::engine::processor::JsonLdEngine;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
    use andromeda_ngsi_ld_query::page_window::PageWindow;
    use andromeda_subscription::{
        error::SubscriptionError,
        notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
        notifying_since::NotifyingRestart,
        outcome_tally::OutcomeTally,
        store::{SubscriptionFields, SubscriptionStore},
        stored_subscription::StoredSubscription,
    };
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use iri_rs::IriBuf;
    use std::sync::Mutex;

    /// A store whose exercised operations are `record_outcome`, which it
    /// records, and `claim_notification_slot`, which answers `slot` (a
    /// saturated store when there is none); every other operation answers a
    /// saturated store.
    #[derive(Default)]
    struct FailureRecordingStore {
        slot: Option<NotificationSlot>,
        failures: Mutex<Vec<(Identifier, OutcomeTally)>>,
    }

    impl FailureRecordingStore {
        fn failures(&self) -> Vec<(Identifier, OutcomeTally)> {
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
            self.slot.ok_or(SubscriptionError::StoreSaturated)
        }

        async fn record_outcomes(&self, _target: &ContextTarget, id: &Identifier, tally: OutcomeTally) -> Result<(), SubscriptionError> {
            self.failures.lock().expect("the failure record is never poisoned").push((id.clone(), tally)); // clone: the record owns what it saw
            Ok(())
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
    async fn an_unloadable_jsonld_context_records_the_notification_as_failed() {
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
        pretty_assertions::assert_eq!(
            store.failures(),
            vec![(subscription(), OutcomeTally::new(NotificationOutcome::Failure, counting_record()))]
        );
    }

    async fn unclaimed(store: &FailureRecordingStore, jsonld_context: Option<&IriBuf>) -> UnclaimedContext {
        unclaimed_notification_context(
            &offline_engine(),
            jsonld_context,
            store,
            &ContextTarget::Live,
            &subscription(),
            NotificationSlotGate::default(),
        )
        .await
    }

    fn unreachable_context() -> IriBuf {
        IriBuf::new("https://example.com/unreachable.jsonld".to_owned()).expect("a valid IRI")
    }

    /// Clause 5.8.6: a notification the gate admits but whose context cannot be
    /// loaded is recorded as failed with the outcome alone, so it counts no
    /// notification and moves no `lastNotification` a `throttling` window reads.
    #[tokio::test]
    async fn an_unloadable_context_before_the_claim_records_the_outcome_alone_when_the_gate_admits() {
        let store = FailureRecordingStore {
            slot: Some(NotificationSlot::Claimed(OutcomeRecord::OutcomeOnly)),
            ..FailureRecordingStore::default()
        };

        let resolved = unclaimed(&store, Some(&unreachable_context())).await;

        assert!(matches!(resolved, UnclaimedContext::Failed));
        pretty_assertions::assert_eq!(
            store.failures(),
            vec![(subscription(), OutcomeTally::new(NotificationOutcome::Failure, OutcomeRecord::OutcomeOnly))]
        );
    }

    /// A notification a `throttling` window or `cooldown` refuses is not sent,
    /// so its unloadable context records nothing.
    #[tokio::test]
    async fn an_unloadable_context_before_the_claim_records_nothing_when_the_gate_refuses() {
        let store = FailureRecordingStore {
            slot: Some(NotificationSlot::Suppressed),
            ..FailureRecordingStore::default()
        };

        let resolved = unclaimed(&store, Some(&unreachable_context())).await;

        assert!(matches!(resolved, UnclaimedContext::Suppressed));
        pretty_assertions::assert_eq!(store.failures(), Vec::new());
    }

    /// A loadable context leaves the slot untouched and records nothing.
    #[tokio::test]
    async fn a_loadable_context_before_the_claim_touches_nothing() {
        let store = FailureRecordingStore::default();

        let resolved = unclaimed(&store, None).await;

        assert!(matches!(resolved, UnclaimedContext::Resolved(_)));
        pretty_assertions::assert_eq!(store.failures(), Vec::new());
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
