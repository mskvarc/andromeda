use crate::{
    csource::CsrSubscriptionVariant,
    deleted_members::DeletedMembers,
    endpoint_admission::SubscriptionEndpointValidator,
    endpoint_refusal::EndpointRefusal,
    entity::EntitySubscriptionVariant,
    error::SubscriptionError,
    forwarding_hook::{ForwardedCopy, SubscriptionForwardingHook, SubscriptionTeardown},
    lifecycle::{SubscriptionService, generate_id},
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
    subscription_context::SubscriptionContext,
    subscription_validation_reason::SubscriptionValidationReason,
};
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_context_space::context_target::ContextTarget;
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
    filter::language_targeting::LanguageTargeting,
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
    page_window::{PageStart, PageWindow},
};
use andromeda_ngsi_ld_representation::core_term_aliases::CoreTermAliases;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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

/// Limits no fixture reaches, with the distance band refused.
fn geo_q_admission() -> GeoQueryAdmission {
    GeoQueryAdmission::new(
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
            values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
        },
        NearDistanceBand::Refused,
    )
}

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

    async fn list(&self, _target: &ContextTarget, _window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
        Err(unused("list"))
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

    async fn record_outcome(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _outcome: NotificationOutcome,
        _record: OutcomeRecord,
    ) -> Result<(), SubscriptionError> {
        Err(unused("record_outcome"))
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

    async fn updated(&self, _subscription: &Subscription, _deleted: &DeletedMembers, _context: &ContextSpec) -> Result<(), SubscriptionError> {
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
        geo_q_admission(),
        LanguageTargeting::Specification,
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

/// The core context release a subscriber chose explicitly for its
/// notifications.
const CHOSEN_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";

/// The `jsonldContext` an update wrote, recorded by [`UpdatingStore`].
type WrittenContext = Arc<Mutex<Option<Option<IriBuf>>>>;

/// A store holding one subscription, which records the `jsonldContext` an
/// update writes back.
struct UpdatingStore {
    stored: Subscription,
    written: WrittenContext,
}

#[async_trait]
impl SubscriptionStore for UpdatingStore {
    async fn get(&self, _target: &ContextTarget, _id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
        Ok(Some(StoredSubscription {
            key: SubscriptionKey::try_from(1_i64).expect("a positive key"),
            subscription: self.stored.clone(), // clone: every read hands back its own copy of the stored resource.
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

    async fn record_outcome(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _outcome: NotificationOutcome,
        _record: OutcomeRecord,
    ) -> Result<(), SubscriptionError> {
        Err(unused("record_outcome"))
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
        _status: SubscriptionStatus,
    ) -> Result<u64, SubscriptionError> {
        // clone: the record keeps the written IRI for the assertion.
        *self.written.lock().expect("the written context is never poisoned") = Some(fields.jsonld_context.cloned());
        Ok(1)
    }
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
    };
    let service = SubscriptionService::new(
        Arc::new(store),
        // clone: the service and the variant each own an engine handle.
        engine.clone(),
        EntitySubscriptionVariant::new(engine),
        None,
        Arc::new(AcceptingEndpointValidator),
        geo_q_admission(),
        LanguageTargeting::Specification,
    );
    let request = SubscriptionRequest::from_request_text(fragment.to_string().as_bytes(), CoreTermAliases::keyword_spellings()).expect("a readable fragment");
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
    };
    let service = SubscriptionService::new(
        Arc::new(store),
        // clone: the service and the variant each own an engine handle.
        engine.clone(),
        EntitySubscriptionVariant::new(engine),
        None,
        Arc::new(AcceptingEndpointValidator),
        geo_q_admission(),
        LanguageTargeting::Specification,
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
        CoreTermAliases::keyword_spellings(),
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
            CoreTermAliases::keyword_spellings(),
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
    let csource = service(&DeletionLog::default(), 0)
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
        CoreTermAliases::keyword_spellings(),
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

    async fn record_outcome(
        &self,
        _target: &ContextTarget,
        _id: &Identifier,
        _outcome: NotificationOutcome,
        _record: OutcomeRecord,
    ) -> Result<(), SubscriptionError> {
        Err(unused("record_outcome"))
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
        geo_q_admission(),
        LanguageTargeting::Specification,
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
