use crate::{
    csource::CsrSubscriptionVariant,
    endpoint_admission::SubscriptionEndpointValidator,
    endpoint_refusal::EndpointRefusal,
    error::SubscriptionError,
    forwarding_hook::{ForwardedCopy, SubscriptionForwardingHook, SubscriptionTeardown},
    lifecycle::{SubscriptionService, generate_id},
    notification_slot::{NotificationSlot, NotificationSlotGate},
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    subscription::{Subscription, SubscriptionStatus},
};
use andromeda_ngsi_ld_query::cursor::PageCursor;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use pretty_assertions::assert_eq;
use std::sync::{Arc, Mutex};
use url::Url;

#[test]
fn generated_subscription_identifiers_are_ngsi_ld_urns() {
    let identifier = generate_id();
    assert!(matches!(
        identifier,
        Ok(identifier) if identifier.as_str().starts_with("urn:ngsi-ld:Subscription:")
    ));
}

/// One observable step of a subscription deletion.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DeletionStep {
    /// The hook read what the deletion must tear down.
    TeardownCaptured,
    /// The store deleted the subscription row.
    RowDeleted,
    /// The hook was told the row is gone, with the teardown it was handed.
    TornDown(SubscriptionTeardown),
}

/// The order the deletion steps happened in, shared by the store and the hook.
type DeletionLog = Arc<Mutex<Vec<DeletionStep>>>;

fn record(log: &DeletionLog, step: DeletionStep) {
    log.lock().expect("the deletion log is never poisoned").push(step);
}

/// A store whose only exercised operation is `delete`, which reports
/// `rows_deleted` and logs the step.
struct DeletingStore {
    log: DeletionLog,
    rows_deleted: u64,
}

fn unused(method: &str) -> SubscriptionError {
    panic!("{}", format!("DeletingStore::{method} is not exercised by the deletion tests"))
}

#[async_trait]
impl SubscriptionStore for DeletingStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Err(unused("get"))
    }

    async fn list(&self, _target: &ContextTarget, _limit: Option<i64>, _offset: Option<i64>) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused("list"))
    }

    async fn list_keyset(&self, _target: &ContextTarget, _cursor: Option<&PageCursor>, _limit: i64) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused("list_keyset"))
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        record(&self.log, DeletionStep::RowDeleted);
        Ok(self.rows_deleted)
    }

    async fn exists(&self, _target: &ContextTarget, _id: &Identifier) -> Result<bool, SubscriptionError> {
        Err(unused("exists"))
    }

    async fn list_active(&self, _target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(unused("list_active"))
    }

    async fn claim_notification_slot(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _gate: NotificationSlotGate,
    ) -> Result<NotificationSlot, SubscriptionError> {
        Err(unused("claim_notification_slot"))
    }

    async fn mark_success(&self, _target: &ContextTarget, _id: &Identifier) -> Result<(), SubscriptionError> {
        Err(unused("mark_success"))
    }

    async fn mark_failure(&self, _target: &ContextTarget, _id: &Identifier) -> Result<(), SubscriptionError> {
        Err(unused("mark_failure"))
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Err(unused("expire_due"))
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Subscription>, SubscriptionError> {
        Err(unused("find_due_interval_subscriptions"))
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(unused("next_interval_due"))
    }

    async fn advance_interval_anchor(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(unused("advance_interval_anchor"))
    }

    async fn create_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<(), SubscriptionError> {
        Err(unused("create_subscription"))
    }

    async fn update_subscription(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _fields: &SubscriptionFields<'_>,
        _status: SubscriptionStatus,
    ) -> Result<u64, SubscriptionError> {
        Err(unused("update_subscription"))
    }
}

/// A hook that hands out a fixed teardown and logs every deletion step.
struct RecordingHook {
    log: DeletionLog,
    teardown: SubscriptionTeardown,
}

#[async_trait]
impl SubscriptionForwardingHook for RecordingHook {
    async fn created(
        &self,
        _subscription: &Subscription,
        _context: &ContextSpec,
        _encountered_sources: &[ContextSourcePseudonym],
    ) -> Result<(), SubscriptionError> {
        Ok(())
    }

    async fn updated(&self, _subscription: &Subscription, _context: &ContextSpec) -> Result<(), SubscriptionError> {
        Ok(())
    }

    async fn teardown_of(&self, _id: &Identifier) -> Result<SubscriptionTeardown, SubscriptionError> {
        record(&self.log, DeletionStep::TeardownCaptured);
        // clone: every call hands the caller its own copy of the fixed plan.
        Ok(self.teardown.clone())
    }

    async fn deleted(&self, _id: &Identifier, teardown: &SubscriptionTeardown) -> Result<(), SubscriptionError> {
        // clone: the log keeps what the hook was handed for the assertion.
        record(&self.log, DeletionStep::TornDown(teardown.clone()));
        Ok(())
    }
}

struct AcceptingEndpointValidator;

impl SubscriptionEndpointValidator for AcceptingEndpointValidator {
    fn validate(&self, _endpoint: &Url, _notifier_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal> {
        Ok(())
    }
}

fn uri(raw: &str) -> Identifier {
    raw.parse().expect("valid test URI")
}

fn captured_teardown() -> SubscriptionTeardown {
    SubscriptionTeardown {
        companion: Some(uri("urn:ngsi-ld:Subscription:companion")),
        forwarded: vec![ForwardedCopy {
            registration: uri("urn:ngsi-ld:ContextSourceRegistration:source"),
            remote_subscription: uri("urn:ngsi-ld:Subscription:remote"),
        }],
    }
}

fn service(log: &DeletionLog, rows_deleted: u64) -> SubscriptionService<CsrSubscriptionVariant> {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
    // clone: the store and the hook each hold the shared log.
    let store = DeletingStore {
        log: Arc::clone(log),
        rows_deleted,
    };
    let hook = RecordingHook {
        log: Arc::clone(log),
        teardown: captured_teardown(),
    };
    SubscriptionService::new(
        Arc::new(store),
        // clone: the service and the variant each own an engine handle.
        engine.clone(),
        CsrSubscriptionVariant::new(engine),
        None,
        Arc::new(AcceptingEndpointValidator),
    )
    .with_forwarding_hook(Arc::new(hook))
}

/// Clause 5.8.5.4: the forwarding mappings cascade with the subscription row,
/// so what the deletion must tear down remotely is read before the row goes and
/// handed to the hook unchanged after it has.
#[tokio::test]
async fn a_deletion_captures_its_teardown_before_the_row_and_hands_it_over_after() {
    let log = DeletionLog::default();
    let deleted = service(&log, 1).delete(&ContextTarget::Live, &uri("urn:ngsi-ld:Subscription:local")).await;
    assert!(deleted.is_ok(), "{deleted:?}");
    let steps = log.lock().expect("the deletion log is never poisoned").clone();
    assert_eq!(
        steps,
        vec![
            DeletionStep::TeardownCaptured,
            DeletionStep::RowDeleted,
            DeletionStep::TornDown(captured_teardown()),
        ]
    );
}

/// Clause 5.8.5: deleting an unknown subscription is `ResourceNotFound`, and
/// nothing is torn down for a subscription that did not exist.
#[tokio::test]
async fn deleting_an_unknown_subscription_tears_nothing_down() {
    let log = DeletionLog::default();
    let deleted = service(&log, 0).delete(&ContextTarget::Live, &uri("urn:ngsi-ld:Subscription:unknown")).await;
    assert!(matches!(deleted, Err(SubscriptionError::NotFound { .. })), "{deleted:?}");
    let steps = log.lock().expect("the deletion log is never poisoned").clone();
    assert_eq!(steps, vec![DeletionStep::TeardownCaptured, DeletionStep::RowDeleted]);
}
