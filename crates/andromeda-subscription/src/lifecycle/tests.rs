use crate::{
    csource::CsrSubscriptionVariant,
    endpoint_admission::SubscriptionEndpointValidator,
    endpoint_refusal::EndpointRefusal,
    entity::EntitySubscriptionVariant,
    error::SubscriptionError,
    forwarding_hook::{ForwardedCopy, SubscriptionForwardingHook, SubscriptionTeardown},
    lifecycle::{SubscriptionService, generate_id},
    notification_slot::{NotificationSlot, NotificationSlotGate},
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use andromeda_context_commit::subscription_key::SubscriptionKey;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::processor::JsonLdEngine,
    user_context_admission::{SubmittedContext, admit_user_context},
};
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    subscription::{Subscription, SubscriptionStatus},
};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_query::{
    cursor::PageCursor,
    page_window::{PageStart, PageWindow},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
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
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
            values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
        },
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
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
            values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
        },
    );
    let updated = service
        .update(&ContextTarget::Live, &uri("urn:ngsi-ld:Subscription:stored"), fragment, &context)
        .await;
    assert!(updated.is_ok(), "{updated:?}");
    written
        .lock()
        .expect("the written context is never poisoned")
        .clone() // clone: the lock guard is released before the answer is returned.
        .expect("the update wrote the subscription back")
}

/// Clauses 5.8.2.4 and 5.5.8: an update whose fragment names no
/// `jsonldContext` leaves the member the subscriber chose, even though the
/// request itself names a context. Initialising it from the applicable
/// `@context` is a clause 5.8.1.4 creation step, and this inline context
/// would have needed a hosted wrapper there.
#[tokio::test]
async fn an_update_naming_a_request_context_keeps_the_stored_jsonld_context() {
    let context = ContextSpec::from_body(json!({ "speed": "https://example.com/speed" }));

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
        GeometryCensusLimits {
            positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
            values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
        },
    );
    let listing = service
        .list_for_response(&ContextTarget::Live, window, &ContextSpec::none())
        .await
        .expect("the store lists");
    let ids = listing
        .subscriptions
        .iter()
        .filter_map(|subscription| subscription.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
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
