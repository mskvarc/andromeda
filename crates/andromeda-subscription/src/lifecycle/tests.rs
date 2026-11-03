use crate::{
    csource::CsrSubscriptionVariant,
    deleted_members::DeletedMembers,
    endpoint_admission::SubscriptionEndpointValidator,
    endpoint_refusal::EndpointRefusal,
    entity::EntitySubscriptionVariant,
    error::SubscriptionError,
    filter_admission::SubscriptionFilterAdmission,
    forwarding_hook::{ForwardedCopy, SubscriptionForwardingHook, SubscriptionTeardown},
    lifecycle::{SubscriptionService, generate_id},
    notification_slot::{NotificationSlot, NotificationSlotGate},
    notifying_since::{NotifyingRestart, NotifyingSince, SinceRestart},
    outcome_tally::OutcomeTally,
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
    subscription_context::SubscriptionContext,
    subscription_validation_reason::SubscriptionValidationReason,
};
use andromeda_context_commit::{store_instant::StoreInstant, subscription_key::SubscriptionKey};
use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::processor::JsonLdEngine,
    error::JsonLdError,
    user_context_admission::{AdmittedContext, SubmittedContext, admit_user_context},
};
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    request_body::RequestBody,
    subscription::{Subscription, SubscriptionStatus},
    subscription_request::SubscriptionRequest,
};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_query::{
    cursor::PageCursor,
    filter::{complexity::FilterComplexityLimits, language_targeting::LanguageTargeting},
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
    page_window::{PageStart, PageWindow},
};
use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use iri_rs::IriBuf;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    env,
    fs,
    num::NonZeroUsize,
    process,
    sync::{
        Arc,
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use url::Url;

/// Limits no fixture reaches, with the distance band refused and the
/// specification's target languages.
fn filter_admission() -> SubscriptionFilterAdmission {
    SubscriptionFilterAdmission {
        geo_q: GeoQueryAdmission::new(
            GeometryCensusLimits {
                positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
                values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
            },
            NearDistanceBand::Refused,
        ),
        q_complexity: FilterComplexityLimits::new(NonZeroUsize::MIN.saturating_add(999), NonZeroUsize::MIN.saturating_add(15)),
        language_targeting: LanguageTargeting::Specification,
    }
}

#[test]
fn generated_subscription_identifiers_are_ngsi_ld_urns() {
    let identifier = generate_id();
    assert!(matches!(
        identifier,
        Ok(identifier) if identifier.as_str().starts_with("urn:ngsi-ld:Subscription:")
    ));
}

/// One observable step of a subscription deletion or expiry.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TeardownStep {
    /// The hook read what the deletion must tear down.
    TeardownCaptured,
    /// The store deleted the subscription row.
    RowDeleted,
    /// The hook was told the row is gone, with the teardown it was handed.
    TornDown(SubscriptionTeardown),
    /// The store transitioned the due subscriptions to expired.
    RowsExpired,
    /// The hook was told this subscription expired.
    Withdrawn(Identifier),
}

/// The order the steps happened in, shared by the store and the hook.
type TeardownLog = Arc<Mutex<Vec<TeardownStep>>>;

fn record(log: &TeardownLog, step: TeardownStep) {
    log.lock().expect("the teardown log is never poisoned").push(step);
}

/// A store whose only exercised operations are `delete`, which reports
/// `rows_deleted`, and `expire_due`, which transitions `expiring`; each logs
/// its step.
struct DeletingStore {
    log: TeardownLog,
    rows_deleted: u64,
    expiring: Vec<Identifier>,
}

fn unused(method: &str) -> SubscriptionError {
    panic!("{}", format!("DeletingStore::{method} is not exercised by the teardown tests"))
}

#[async_trait]
impl SubscriptionStore for DeletingStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Err(unused("get"))
    }

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused("list"))
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        record(&self.log, TeardownStep::RowDeleted);
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

    async fn record_outcomes(&self, _target: &ContextTarget, _id: &Identifier, _tally: OutcomeTally) -> Result<(), SubscriptionError> {
        Err(unused("record_outcomes"))
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        record(&self.log, TeardownStep::RowsExpired);
        Ok(self.expiring.clone()) // clone: every sweep hands back its own list
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(unused("find_due_interval_subscriptions"))
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(unused("next_interval_due"))
    }

    async fn record_interval_tick(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(unused("record_interval_tick"))
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
        _restart: NotifyingRestart,
    ) -> Result<u64, SubscriptionError> {
        Err(unused("update_subscription"))
    }
}

/// A hook that hands out a fixed teardown and logs every teardown step.
struct RecordingHook {
    log: TeardownLog,
    teardown: SubscriptionTeardown,
}

#[async_trait]
impl SubscriptionForwardingHook for RecordingHook {
    async fn created(&self, _subscription: &Subscription, _encountered_sources: &[ContextSourcePseudonym]) -> Result<(), SubscriptionError> {
        Ok(())
    }

    async fn updated(&self, _subscription: &Subscription, _deleted: &DeletedMembers) -> Result<(), SubscriptionError> {
        Ok(())
    }

    async fn teardown_of(&self, _id: &Identifier) -> Result<SubscriptionTeardown, SubscriptionError> {
        record(&self.log, TeardownStep::TeardownCaptured);
        // clone: every call hands the caller its own copy of the fixed plan.
        Ok(self.teardown.clone())
    }

    async fn deleted(&self, _id: &Identifier, teardown: &SubscriptionTeardown) -> Result<(), SubscriptionError> {
        // clone: the log keeps what the hook was handed for the assertion.
        record(&self.log, TeardownStep::TornDown(teardown.clone()));
        Ok(())
    }

    async fn expired(&self, id: &Identifier) -> Result<(), SubscriptionError> {
        record(&self.log, TeardownStep::Withdrawn(id.clone())); // clone: the log keeps the identifier for the assertion
        Ok(())
    }
}

struct AcceptingEndpointValidator;

impl SubscriptionEndpointValidator for AcceptingEndpointValidator {
    fn validate(&self, _endpoint: &Url, _notifier_info: Option<&[KeyValuePair]>, _receiver_info: Option<&[KeyValuePair]>) -> Result<(), EndpointRefusal> {
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

fn service(log: &TeardownLog, rows_deleted: u64) -> SubscriptionService<CsrSubscriptionVariant> {
    expiring_service(log, rows_deleted, Vec::new())
}

fn expiring_service(log: &TeardownLog, rows_deleted: u64, expiring: Vec<Identifier>) -> SubscriptionService<CsrSubscriptionVariant> {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
    // clone: the store and the hook each hold the shared log.
    let store = DeletingStore {
        log: Arc::clone(log),
        rows_deleted,
        expiring,
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
        filter_admission(),
    )
    .with_forwarding_hook(Arc::new(hook))
}

/// Clause 5.8.5.4: the forwarding mappings cascade with the subscription row,
/// so what the deletion must tear down remotely is read before the row goes and
/// handed to the hook unchanged after it has.
#[tokio::test]
async fn a_deletion_captures_its_teardown_before_the_row_and_hands_it_over_after() {
    let log = TeardownLog::default();
    let deleted = service(&log, 1).delete(&ContextTarget::Live, &uri("urn:ngsi-ld:Subscription:local")).await;
    assert!(deleted.is_ok(), "{deleted:?}");
    let steps = log.lock().expect("the teardown log is never poisoned").clone();
    assert_eq!(
        steps,
        vec![
            TeardownStep::TeardownCaptured,
            TeardownStep::RowDeleted,
            TeardownStep::TornDown(captured_teardown()),
        ]
    );
}

/// Clause 5.8.5: deleting an unknown subscription is `ResourceNotFound`, and
/// nothing is torn down for a subscription that did not exist.
#[tokio::test]
async fn deleting_an_unknown_subscription_tears_nothing_down() {
    let log = TeardownLog::default();
    let deleted = service(&log, 0).delete(&ContextTarget::Live, &uri("urn:ngsi-ld:Subscription:unknown")).await;
    assert!(matches!(deleted, Err(SubscriptionError::NotFound { .. })), "{deleted:?}");
    let steps = log.lock().expect("the teardown log is never poisoned").clone();
    assert_eq!(steps, vec![TeardownStep::TeardownCaptured, TeardownStep::RowDeleted]);
}

/// Clause 5.8.1.4: an expired subscription sends no notification, so each one
/// the sweep transitions is withdrawn from the context sources after its row
/// is marked expired, as a deletion would be.
#[tokio::test]
async fn an_expiry_withdraws_every_expired_subscription_after_the_transition() {
    let log = TeardownLog::default();
    let expiring = vec![uri("urn:ngsi-ld:Subscription:first"), uri("urn:ngsi-ld:Subscription:second")];
    // clone: the store hands the list back and the assertion compares against it.
    let expired = expiring_service(&log, 0, expiring.clone()).expire_due(&ContextTarget::Live, Utc::now()).await;
    assert_eq!(expired.ok(), Some(expiring));
    let steps = log.lock().expect("the teardown log is never poisoned").clone();
    assert_eq!(
        steps,
        vec![
            TeardownStep::RowsExpired,
            TeardownStep::Withdrawn(uri("urn:ngsi-ld:Subscription:first")),
            TeardownStep::Withdrawn(uri("urn:ngsi-ld:Subscription:second")),
        ]
    );
}

fn snapshot_target() -> ContextTarget {
    ContextTarget::Snapshot(SnapshotTarget::published(
        SnapshotKey::try_from(3).expect("a positive key"),
        uri("urn:ngsi-ld:Snapshot:branch"),
    ))
}

/// A Snapshot branch takes no part in federation, so its expiry withdraws
/// nothing from the context sources.
#[tokio::test]
async fn an_expiry_in_a_snapshot_withdraws_nothing() {
    let log = TeardownLog::default();
    let expired = expiring_service(&log, 0, vec![uri("urn:ngsi-ld:Subscription:first")])
        .expire_due(&snapshot_target(), Utc::now())
        .await;
    assert!(expired.is_ok(), "{expired:?}");
    let steps = log.lock().expect("the teardown log is never poisoned").clone();
    assert_eq!(steps, vec![TeardownStep::RowsExpired]);
}

/// The core context release a subscriber chose explicitly for its
/// notifications.
const CHOSEN_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

/// The `jsonldContext` an update wrote, recorded by [`UpdatingStore`].
type WrittenContext = Arc<Mutex<Option<Option<IriBuf>>>>;

/// The status an update wrote, recorded by [`UpdatingStore`].
type WrittenStatus = Arc<Mutex<Option<SubscriptionStatus>>>;

/// What an update restarted, recorded by [`UpdatingStore`].
type WrittenRestart = Arc<Mutex<Option<NotifyingRestart>>>;

/// A store holding one subscription, which records the `jsonldContext`, the
/// status and the restarted instants an update writes back.
struct UpdatingStore {
    stored: Subscription,
    written: WrittenContext,
    status: WrittenStatus,
    restart: WrittenRestart,
}

#[async_trait]
impl SubscriptionStore for UpdatingStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Ok(Some(StoredSubscription {
            key: SubscriptionKey::try_from(1_i64).expect("a positive key"),
            subscription: self.stored.clone(), // clone: every read hands back its own copy of the stored resource.
            notifying_since: since_creation(),
        }))
    }

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused("list"))
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        Err(unused("delete"))
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

    async fn record_outcomes(&self, _target: &ContextTarget, _id: &Identifier, _tally: OutcomeTally) -> Result<(), SubscriptionError> {
        Err(unused("record_outcomes"))
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Err(unused("expire_due"))
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(unused("find_due_interval_subscriptions"))
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(unused("next_interval_due"))
    }

    async fn record_interval_tick(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(unused("record_interval_tick"))
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
        fields: &SubscriptionFields<'_>,
        status: SubscriptionStatus,
        restart: NotifyingRestart,
    ) -> Result<u64, SubscriptionError> {
        // clone: the record keeps the written IRI for the assertion.
        *self.written.lock().expect("the written context is never poisoned") = Some(fields.jsonld_context.cloned());
        *self.status.lock().expect("the written status is never poisoned") = Some(status);
        *self.restart.lock().expect("the written restart is never poisoned") = Some(restart);
        Ok(1)
    }
}

/// The instants of a subscription created at the epoch and never restarted.
fn since_creation() -> NotifyingSince {
    let created = StoreInstant::new(DateTime::<Utc>::UNIX_EPOCH);
    NotifyingSince {
        active: created,
        selected: created,
    }
}

/// What an update of `fragment` restarts of a stored subscription standing
/// at `status` with no `expiresAt`.
async fn restart_written_after_update(status: SubscriptionStatus, fragment: Value) -> NotifyingRestart {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
    let mut stored = subscription_with_chosen_context();
    stored.lifecycle.standing.status = status;
    let written = WrittenRestart::default();
    let store = UpdatingStore {
        stored,
        written: WrittenContext::default(),
        status: WrittenStatus::default(),
        restart: Arc::clone(&written),
    };
    let service = SubscriptionService::new(
        Arc::new(store),
        // clone: the service and the variant each own an engine handle.
        engine.clone(),
        EntitySubscriptionVariant::new(engine),
        None,
        Arc::new(AcceptingEndpointValidator),
        filter_admission(),
    );
    let request = SubscriptionRequest::from_request_text(fragment.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a readable fragment");
    let updated = service
        .update(
            &ContextTarget::Live,
            &uri("urn:ngsi-ld:Subscription:stored"),
            request,
            &AdmittedContext::core_only(),
        )
        .await;
    assert!(updated.is_ok(), "{updated:?}");
    let recorded = *written.lock().expect("the written restart is never poisoned");
    recorded.expect("the update wrote the subscription back")
}

/// Table 5.2.12-1 `isActive`: a paused subscription set active again may be
/// notified only of changes committed from then on, so the update restarts
/// its active instant and leaves its selection alone.
#[tokio::test]
async fn reactivating_a_paused_subscription_restarts_its_active_instant() {
    let restart = restart_written_after_update(SubscriptionStatus::Paused, json!({"isActive": true})).await;
    assert_eq!(
        restart,
        NotifyingRestart {
            active: SinceRestart::Restarted,
            selected: SinceRestart::Kept,
        }
    );
}

#[tokio::test]
async fn pausing_or_retuning_delivery_restarts_nothing() {
    for fragment in [json!({"isActive": false}), json!({"throttling": 5})] {
        let restart = restart_written_after_update(SubscriptionStatus::Active, fragment).await;
        assert_eq!(
            restart,
            NotifyingRestart {
                active: SinceRestart::Kept,
                selected: SinceRestart::Kept,
            }
        );
    }
}

/// `entities` decides which changes trigger a notification (clause 5.8.1.4),
/// so a new selector restarts the selected instant of an active subscription.
#[tokio::test]
async fn a_new_entity_selector_restarts_the_selected_instant() {
    let restart = restart_written_after_update(SubscriptionStatus::Active, json!({"entities": [{"type": "https://example.com/Truck"}]})).await;
    assert_eq!(
        restart,
        NotifyingRestart {
            active: SinceRestart::Kept,
            selected: SinceRestart::Restarted,
        }
    );
}

/// An entity subscription whose subscriber chose [`CHOSEN_CONTEXT`] for its
/// notifications.
fn subscription_with_chosen_context() -> Subscription {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:Subscription:stored",
        "type": "Subscription",
        "entities": [{ "type": "https://example.com/Vehicle" }],
        "notification": { "endpoint": { "uri": "http://receiver.example.com/notify" } },
        "jsonldContext": CHOSEN_CONTEXT,
        "status": "active",
        "createdAt": "2026-01-01T00:00:00Z",
        "modifiedAt": "2026-01-01T00:00:00Z"
    }))
    .expect("the fixture is a valid subscription")
}

/// Update the stored subscription with `fragment` under `context`, answering
/// the `jsonldContext` the update wrote back.
async fn written_context_after_update(fragment: Value, context: &ContextSpec) -> Option<IriBuf> {
    written_after_update(fragment, context).await.expect("the update wrote the subscription back")
}

/// Update the stored subscription with `fragment` under `context`, answering
/// the `jsonldContext` of the write the store was asked for, or nothing when
/// the update asked for no write.
async fn written_after_update(fragment: Value, context: &ContextSpec) -> Option<Option<IriBuf>> {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
    // clone: the admission consumes the spec, and the caller keeps its own
    let context = admit_user_context(&engine, SubmittedContext::from(context.clone()))
        .await
        .expect("the context is admitted");
    let written = WrittenContext::default();
    let store = UpdatingStore {
        stored: subscription_with_chosen_context(),
        written: Arc::clone(&written),
        status: WrittenStatus::default(),
        restart: WrittenRestart::default(),
    };
    let service = SubscriptionService::new(
        Arc::new(store),
        // clone: the service and the variant each own an engine handle.
        engine.clone(),
        EntitySubscriptionVariant::new(engine),
        None,
        Arc::new(AcceptingEndpointValidator),
        filter_admission(),
    );
    let request = SubscriptionRequest::from_request_text(fragment.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a readable fragment");
    let updated = service
        .update(&ContextTarget::Live, &uri("urn:ngsi-ld:Subscription:stored"), request, &context)
        .await;
    assert!(updated.is_ok(), "{updated:?}");
    written.lock().expect("the written context is never poisoned").clone() // clone: the lock guard is released before the answer is returned.
}

/// Clause 5.5.8 acts only for each member of the Fragment, so the empty
/// Fragment modifies nothing and the store is asked to write nothing.
#[tokio::test]
async fn the_empty_fragment_writes_nothing() {
    assert_eq!(written_after_update(json!({}), &ContextSpec::none()).await, None);
}

/// The status an update of `fragment` writes to a subscription whose row still
/// reads `status` although its `expiresAt` has passed, as it does between the
/// due instant and the sweep that persists the expiry.
async fn status_written_after_update(status: SubscriptionStatus, fragment: Value) -> SubscriptionStatus {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
    let mut stored = subscription_with_chosen_context();
    stored.lifecycle.standing.status = status;
    stored.lifecycle.standing.expires_at = Some(Utc::now() - Duration::hours(1));
    let written = WrittenStatus::default();
    let store = UpdatingStore {
        stored,
        written: WrittenContext::default(),
        status: Arc::clone(&written),
        restart: WrittenRestart::default(),
    };
    let service = SubscriptionService::new(
        Arc::new(store),
        // clone: the service and the variant each own an engine handle.
        engine.clone(),
        EntitySubscriptionVariant::new(engine),
        None,
        Arc::new(AcceptingEndpointValidator),
        filter_admission(),
    );
    let request = SubscriptionRequest::from_request_text(fragment.to_string().as_bytes(), ContextReading::keyword_spellings()).expect("a readable fragment");
    let updated = service
        .update(
            &ContextTarget::Live,
            &uri("urn:ngsi-ld:Subscription:stored"),
            request,
            &AdmittedContext::core_only(),
        )
        .await;
    assert!(updated.is_ok(), "{updated:?}");
    let recorded = *written.lock().expect("the written status is never poisoned");
    recorded.expect("the update wrote the status")
}

/// Clause 5.8.2.4: removing `expiresAt` leaves an expired subscription
/// expired, also while its row still reads `active` or `paused` because the
/// sweep has not yet persisted the expiry.
#[tokio::test]
async fn removing_expires_at_keeps_a_subscription_past_its_expiry_expired() {
    for stored in [SubscriptionStatus::Active, SubscriptionStatus::Paused, SubscriptionStatus::Expired] {
        let status = status_written_after_update(stored, json!({ "expiresAt": "urn:ngsi-ld:null" })).await;
        assert_eq!(status, SubscriptionStatus::Expired, "a row reading {stored:?}");
    }
}

/// Clause 5.8.2.4: a future `expiresAt` reactivates a subscription past its
/// expiry whichever status its row reads.
#[tokio::test]
async fn a_future_expires_at_reactivates_a_subscription_past_its_expiry() {
    for stored in [SubscriptionStatus::Active, SubscriptionStatus::Paused, SubscriptionStatus::Expired] {
        let status = status_written_after_update(stored, json!({ "expiresAt": "2099-01-01T00:00:00Z" })).await;
        assert_eq!(status, SubscriptionStatus::Active, "a row reading {stored:?}");
    }
}

/// Clause 5.8.2.4 checks a fragment against the data types and restrictions
/// of Table 5.2.12-1, not its cardinalities, so a fragment without `type` is
/// applied.
#[tokio::test]
async fn an_update_fragment_without_its_type_is_applied() {
    assert!(
        written_after_update(json!({ "subscriptionName": "renamed" }), &ContextSpec::none())
            .await
            .is_some()
    );
}

/// A fragment holding only its `type` names no member to apply either.
#[tokio::test]
async fn a_fragment_holding_only_its_type_writes_nothing() {
    assert_eq!(written_after_update(json!({ "type": "Subscription" }), &ContextSpec::none()).await, None);
}

/// Clauses 5.8.2.4 and 5.5.8: an update whose fragment names no
/// `jsonldContext` leaves the member the subscriber chose, even though the
/// request itself names a context. Initialising it from the applicable
/// `@context` is a clause 5.8.1.4 creation step, and this inline context
/// would have needed a hosted wrapper there.
#[tokio::test]
async fn an_update_naming_a_request_context_keeps_the_stored_jsonld_context() {
    let context = ContextSpec::from_body(JsonLiteral::admit(&json!({ "speed": "https://example.com/speed" }).to_string()).expect("an admissible literal"));

    let written = written_context_after_update(json!({ "q": "speed>1" }), &context).await;

    assert_eq!(written.as_ref().map(IriBuf::as_str), Some(CHOSEN_CONTEXT));
}

/// A fragment that names a `jsonldContext` replaces the stored one (clause
/// 5.5.8).
#[tokio::test]
async fn an_update_naming_a_jsonld_context_replaces_the_stored_one() {
    // An embedded release, so the offline engine can validate it.
    let replacement = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";

    let written = written_context_after_update(json!({ "jsonldContext": replacement }), &ContextSpec::none()).await;

    assert_eq!(written.as_ref().map(IriBuf::as_str), Some(replacement));
}

/// A hosted context declaring a JSON-LD Scoped Context on `speed`.
const SCOPED_CONTEXT: &str = "https://example.com/scoped.jsonld";

/// Makes every temporary context file name unique, so engines built by tests
/// running in parallel never write or remove each other's files.
static CONTEXT_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A service over the stored subscription whose engine resolves
/// [`SCOPED_CONTEXT`] and nothing else, with what its store was asked to write.
fn service_serving_a_scoped_context() -> (SubscriptionService<EntitySubscriptionVariant>, WrittenContext) {
    let sequence = CONTEXT_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = env::temp_dir().join(format!("andromeda-subscription-scoped-context-{}-{sequence}.jsonld", process::id()));
    fs::write(
        &path,
        r#"{"@context": {"speed": {"@id": "https://example.com/speed", "@context": {"unitCode": "https://example.com/rewritten"}}}}"#,
    )
    .expect("the temporary directory is writable");
    let engine = JsonLdEngine::builder()
        .allow_remote(false)
        .preload_contexts(HashMap::from([(Url::parse(SCOPED_CONTEXT).expect("a valid URL"), path.clone())])) // clone: the removal below needs the path
        .build()
        .expect("engine builds");
    fs::remove_file(&path).expect("the temporary context is removable");
    let written = WrittenContext::default();
    let store = UpdatingStore {
        stored: subscription_with_chosen_context(),
        written: Arc::clone(&written),
        status: WrittenStatus::default(),
        restart: WrittenRestart::default(),
    };
    let service = SubscriptionService::new(
        Arc::new(store),
        // clone: the service and the variant each own an engine handle.
        engine.clone(),
        EntitySubscriptionVariant::new(engine),
        None,
        Arc::new(AcceptingEndpointValidator),
        filter_admission(),
    );
    (service, written)
}

fn assert_scoped_refusal(error: &SubscriptionError) {
    assert!(
        matches!(error, SubscriptionError::JsonLd(JsonLdError::ScopedUserContext { term }) if term.as_str() == "speed"),
        "{error:?}"
    );
}

/// Clause 5.5.7: the context a `jsonldContext` names is a user `@context`,
/// which shall not contain JSON-LD Scoped Contexts.
#[tokio::test]
async fn a_subscription_created_with_a_scoped_jsonld_context_is_refused() {
    let (service, _) = service_serving_a_scoped_context();
    let request = SubscriptionRequest::from_request_text(
        json!({
            "type": "Subscription",
            "entities": [{ "type": "https://example.com/Vehicle" }],
            "notification": { "endpoint": { "uri": "http://receiver.example.com/notify" } },
            "jsonldContext": SCOPED_CONTEXT
        })
        .to_string()
        .as_bytes(),
        ContextReading::keyword_spellings(),
    )
    .expect("a readable subscription");
    let error = service
        .create(&ContextTarget::Live, request, SubscriptionContext::CoreOnly, &[])
        .await
        .expect_err("clause 5.5.7 refuses it");
    assert_scoped_refusal(&error);
}

/// Table 5.2.12-1 gives `type` cardinality 1: clause 5.8.1.4 refuses a create
/// body without it, and clause 5.11.2.4 applies the same check to a Context
/// Source Registration Subscription. Neither store is asked to write.
#[tokio::test]
async fn a_subscription_created_without_its_type_is_refused_on_both_kinds() {
    let body = || {
        SubscriptionRequest::from_request_text(
            json!({
                "entities": [{ "type": "https://example.com/Vehicle" }],
                "notification": { "endpoint": { "uri": "http://receiver.example.com/notify" } }
            })
            .to_string()
            .as_bytes(),
            ContextReading::keyword_spellings(),
        )
        .expect("a readable subscription")
    };
    let (entity_service, written) = service_serving_a_scoped_context();
    let entity = entity_service.create(&ContextTarget::Live, body(), SubscriptionContext::CoreOnly, &[]).await;
    assert!(
        matches!(entity, Err(SubscriptionError::Validation(SubscriptionValidationReason::TypeRequired))),
        "{entity:?}"
    );
    assert_eq!(*written.lock().expect("the written context is never poisoned"), None);
    let csource = service(&TeardownLog::default(), 0)
        .create(&ContextTarget::Live, body(), SubscriptionContext::CoreOnly, &[])
        .await;
    assert!(
        matches!(csource, Err(SubscriptionError::Validation(SubscriptionValidationReason::TypeRequired))),
        "{csource:?}"
    );
}

#[tokio::test]
async fn an_update_naming_a_scoped_jsonld_context_is_refused_and_writes_nothing() {
    let (service, written) = service_serving_a_scoped_context();
    let request = SubscriptionRequest::from_request_text(
        json!({ "jsonldContext": SCOPED_CONTEXT }).to_string().as_bytes(),
        ContextReading::keyword_spellings(),
    )
    .expect("a readable fragment");
    let error = service
        .update(
            &ContextTarget::Live,
            &uri("urn:ngsi-ld:Subscription:stored"),
            request,
            &AdmittedContext::core_only(),
        )
        .await
        .expect_err("clause 5.5.7 refuses it");
    assert_scoped_refusal(&error);
    assert_eq!(*written.lock().expect("the written context is never poisoned"), None);
}

/// A store holding five entity subscriptions, `urn:ngsi-ld:Subscription:1` to
/// `:5`, which lists the page a window cuts from them in identifier order, as
/// the PostgreSQL store does.
struct ListingStore {
    stored: Vec<Subscription>,
}

impl ListingStore {
    fn of_five() -> ListingStore {
        let stored = (1..=5)
            .map(|index| {
                serde_json::from_value(json!({
                    "id": format!("urn:ngsi-ld:Subscription:{index}"),
                    "type": "Subscription",
                    "entities": [{ "type": "https://example.com/Vehicle" }],
                    "notification": { "endpoint": { "uri": "http://receiver.example.com/notify" } },
                    "status": "active",
                    "createdAt": "2026-01-01T00:00:00Z",
                    "modifiedAt": "2026-01-01T00:00:00Z"
                }))
                .expect("the fixture is a valid subscription")
            })
            .collect();
        ListingStore { stored }
    }
}

#[async_trait]
impl SubscriptionStore for ListingStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Err(unused("get"))
    }

    async fn list(&self, _target: &ContextTarget, window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        let total = i64::try_from(self.stored.len()).expect("five subscriptions");
        // clone: every listing hands back its own copies of the stored resources.
        Ok((window.cut(self.stored.clone(), |subscription| subscription.id.as_str()), total))
    }

    async fn delete(&self, _target: &ContextTarget, _id: &Identifier) -> Result<u64, SubscriptionError> {
        Err(unused("delete"))
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

    async fn record_outcomes(&self, _target: &ContextTarget, _id: &Identifier, _tally: OutcomeTally) -> Result<(), SubscriptionError> {
        Err(unused("record_outcomes"))
    }

    async fn expire_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
        Err(unused("expire_due"))
    }

    async fn find_due_interval_subscriptions(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Vec<StoredSubscription>, SubscriptionError> {
        Err(unused("find_due_interval_subscriptions"))
    }

    async fn next_interval_due(&self, _target: &ContextTarget, _cutoff: DateTime<Utc>) -> Result<Option<DateTime<Utc>>, SubscriptionError> {
        Err(unused("next_interval_due"))
    }

    async fn record_interval_tick(&self, _target: &ContextTarget, _id: &Identifier, _cutoff: DateTime<Utc>) -> Result<(), SubscriptionError> {
        Err(unused("record_interval_tick"))
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
        _restart: NotifyingRestart,
    ) -> Result<u64, SubscriptionError> {
        Err(unused("update_subscription"))
    }
}

/// The identifiers, total and boundary one listing page answers with.
async fn listed(window: &PageWindow) -> (Vec<String>, i64, Option<(Identifier, Identifier)>) {
    let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
    let service = SubscriptionService::new(
        Arc::new(ListingStore::of_five()),
        // clone: the service and the variant each own an engine handle.
        engine.clone(),
        EntitySubscriptionVariant::new(engine),
        None,
        Arc::new(AcceptingEndpointValidator),
        filter_admission(),
    );
    let listing = service
        .list_for_response(&ContextTarget::Live, window, &ContextSpec::none())
        .await
        .expect("the store lists");
    let ids = listing.subscriptions.iter().map(|subscription| subscription.id.as_str().to_owned()).collect();
    (ids, listing.total, listing.boundary)
}

fn subscription_ids(indices: &[u8]) -> Vec<String> {
    indices.iter().map(|index| format!("urn:ngsi-ld:Subscription:{index}")).collect()
}

/// Clause 5.5.9.2: an offset listing is one page of the identifier-ordered
/// collection, with the whole collection's total and its boundary.
#[tokio::test]
async fn an_offset_listing_answers_its_page_total_and_boundary() {
    let (ids, total, boundary) = listed(&PageWindow::offset(2, 0)).await;
    assert_eq!(ids, subscription_ids(&[1, 2]));
    assert_eq!(total, 5);
    assert_eq!(boundary, Some((uri("urn:ngsi-ld:Subscription:1"), uri("urn:ngsi-ld:Subscription:2"))));
}

/// Clause 5.5.9.1: the first page of a cursor walk starts the collection.
#[tokio::test]
async fn a_fresh_cursor_listing_starts_the_collection() {
    let (ids, _, _) = listed(&PageWindow {
        limit: 2,
        start: PageStart::Cursor(None),
    })
    .await;
    assert_eq!(ids, subscription_ids(&[1, 2]));
}

/// A backward cursor answers the page before its boundary, ascending, and
/// the boundary it reports is that page's own.
#[tokio::test]
async fn a_backward_cursor_listing_answers_the_page_before_its_boundary() {
    let (ids, total, boundary) = listed(&PageWindow {
        limit: 2,
        start: PageStart::Cursor(Some(PageCursor::Before(uri("urn:ngsi-ld:Subscription:4")))),
    })
    .await;
    assert_eq!(ids, subscription_ids(&[2, 3]));
    assert_eq!(total, 5);
    assert_eq!(boundary, Some((uri("urn:ngsi-ld:Subscription:2"), uri("urn:ngsi-ld:Subscription:3"))));
}
