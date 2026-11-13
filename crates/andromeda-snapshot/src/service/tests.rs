use crate::{
    error::SnapshotError,
    preparation_start::PreparationStart,
    service::{SnapshotService, SnapshotServiceConfig},
    snapshot_validation_reason::SnapshotValidationReason,
    store_double::{ClaimHold, QuotaCheck, StoreDouble},
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    request_body::RequestBody,
    snapshot::{Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_clone::SnapshotCloneInput,
    snapshot_input::SnapshotInput,
    snapshot_patch::SnapshotPatch,
};
use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use url::Url;

const SNAPSHOT_ID: &str = "urn:ngsi-ld:Snapshot:scoped";

fn snapshot_id() -> Identifier {
    SNAPSHOT_ID.parse().expect("valid snapshot identifier")
}

fn service() -> SnapshotService {
    service_holding(stored_snapshot(None, None))
}

fn stored_snapshot(endpoint: Option<&str>, receiver_info: Option<Vec<KeyValuePair>>) -> Snapshot {
    let now = Utc::now();
    Snapshot::builder()
        .id(snapshot_id())
        .snapshot_priority(SnapshotPriority::default())
        .status(SnapshotStatus::Success)
        .endpoint(endpoint.and_then(|raw| Url::parse(raw).ok()))
        .receiver_info(receiver_info)
        .created_at(now)
        .modified_at(now)
        .expires_at(now + ChronoDuration::hours(1))
        .build()
}

fn bad_pair() -> KeyValuePair {
    KeyValuePair {
        key: "Bad Name".into(),
        value: "x".into(),
    }
}

fn service_holding(snapshot: Snapshot) -> SnapshotService {
    service_over(StoreDouble::new(snapshot, ClaimHold::Held))
}

fn service_over(store: StoreDouble) -> SnapshotService {
    SnapshotService::new(Arc::new(store), None, None, service_config())
}

fn service_config() -> SnapshotServiceConfig {
    SnapshotServiceConfig {
        default_lifetime: Duration::from_hours(1),
        maximum_lifetime: Duration::from_hours(24),
        scoped_operation_wait: Duration::from_millis(50),
        tenant_count_quota: None,
        tenant_byte_quota: None,
    }
}

/// Records every Snapshot whose preparation was started.
#[derive(Default)]
struct StartedPreparations(Mutex<Vec<Identifier>>);

impl PreparationStart for StartedPreparations {
    fn start(&self, snapshot: &Identifier) {
        self.0.lock().expect("the record is not poisoned").push(snapshot.clone());
    }
}

#[test]
fn preparing_a_created_snapshot_starts_its_preparation_through_the_port() {
    let started = Arc::new(StartedPreparations::default());
    let service = SnapshotService::new(
        Arc::new(StoreDouble::new(stored_snapshot(None, None), ClaimHold::Held)),
        Some(Arc::clone(&started) as Arc<dyn PreparationStart>),
        None,
        service_config(),
    );

    service.prepare_created(&snapshot_id());

    pretty_assertions::assert_eq!(*started.0.lock().expect("the record is not poisoned"), vec![snapshot_id()]);
}

#[tokio::test]
async fn a_prepared_snapshot_scopes_the_request() {
    let scoped = service().validate_for_scoping(&snapshot_id()).await;
    assert!(matches!(&scoped, Ok(snapshot) if snapshot.id == snapshot_id()), "{scoped:?}");
}

/// The microseconds of an instant's sub-second part are all it carries.
fn is_whole_microseconds(instant: DateTime<Utc>) -> bool {
    instant.timestamp_subsec_nanos().is_multiple_of(1_000)
}

/// A created Snapshot answers with the `createdAt` and `modifiedAt` a later
/// read returns, at the microsecond the stored record keeps.
#[tokio::test]
async fn a_created_snapshot_answers_with_the_system_times_a_read_returns() {
    let service = service();
    let created = service
        .create(
            serde_json::from_str::<SnapshotInput>(
                r#"{"type": "Snapshot", "id": "urn:ngsi-ld:Snapshot:created", "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]}"#,
            )
            .expect("a Snapshot body"),
            AdmittedContext::core_only(),
        )
        .await
        .expect("a created snapshot");
    let read = service.get(&created.id).await.expect("the created snapshot reads back");
    assert!(
        is_whole_microseconds(created.created_at) && is_whole_microseconds(created.modified_at),
        "{created:?}"
    );
    pretty_assertions::assert_eq!((read.created_at, read.modified_at), (created.created_at, created.modified_at));
}

/// A clone is stamped like a created Snapshot.
#[tokio::test]
async fn a_cloned_snapshot_answers_with_the_system_times_a_read_returns() {
    let service = service();
    let cloned = service
        .clone_snapshot(
            &snapshot_id(),
            serde_json::from_str::<SnapshotCloneInput>(r#"{"type": "Snapshot", "id": "urn:ngsi-ld:Snapshot:cloned"}"#).expect("a clone body"),
        )
        .await
        .expect("a cloned snapshot");
    let read = service.get(&cloned.id).await.expect("the clone reads back");
    assert!(
        is_whole_microseconds(cloned.created_at) && is_whole_microseconds(cloned.modified_at),
        "{cloned:?}"
    );
    pretty_assertions::assert_eq!((read.created_at, read.modified_at), (cloned.created_at, cloned.modified_at));
}

/// The Snapshot is committed before the quota check runs, so a check that
/// fails cannot undo the create: answering an error would invite a retry that
/// creates a second Snapshot.
#[tokio::test]
async fn a_create_whose_quota_check_fails_answers_the_created_snapshot() {
    let service = service_over(StoreDouble::new(stored_snapshot(None, None), ClaimHold::Held).with_quota_check(QuotaCheck::Fails));
    let created = service
        .create(
            serde_json::from_str::<SnapshotInput>(
                r#"{"type": "Snapshot", "id": "urn:ngsi-ld:Snapshot:created", "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]}"#,
            )
            .expect("a Snapshot body"),
            AdmittedContext::core_only(),
        )
        .await
        .expect("the create succeeded");
    assert!(service.get(&created.id).await.is_ok(), "{created:?}");
}

#[tokio::test]
async fn a_clone_whose_quota_check_fails_answers_the_clone() {
    let service = service_over(StoreDouble::new(stored_snapshot(None, None), ClaimHold::Held).with_quota_check(QuotaCheck::Fails));
    let cloned = service
        .clone_snapshot(
            &snapshot_id(),
            serde_json::from_str::<SnapshotCloneInput>(r#"{"type": "Snapshot", "id": "urn:ngsi-ld:Snapshot:cloned"}"#).expect("a clone body"),
        )
        .await
        .expect("the clone succeeded");
    assert!(service.get(&cloned.id).await.is_ok(), "{cloned:?}");
}

async fn create_with(endpoint: &str, receiver_info: &str) -> Result<Snapshot, SnapshotError> {
    let body = format!(
        r#"{{"type": "Snapshot", "snapshotQueries": [{{"type": "Query", "entities": [{{"type": "Vehicle"}}]}}], "endpoint": "{endpoint}", "receiverInfo": {receiver_info}}}"#
    );
    service()
        .create(
            serde_json::from_str::<SnapshotInput>(&body).expect("a Snapshot body"),
            AdmittedContext::core_only(),
        )
        .await
}

fn patch(body: &str) -> SnapshotPatch {
    SnapshotPatch::from_request_text(body.as_bytes(), ContextReading::keyword_spellings()).expect("a Snapshot fragment")
}

#[tokio::test]
async fn creating_a_snapshot_with_a_bad_receiver_info_key_on_an_http_endpoint_is_refused() {
    let refused = create_with("http://example.com/s", r#"[{"key": "Bad Name", "value": "x"}]"#).await;
    assert!(
        matches!(refused, Err(SnapshotError::Validation(SnapshotValidationReason::ReceiverInfo(_)))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn creating_a_snapshot_with_a_bad_receiver_info_value_on_an_http_endpoint_is_refused() {
    let refused = create_with("http://example.com/s", r#"[{"key": "X-A", "value": "a\nb"}]"#).await;
    assert!(
        matches!(refused, Err(SnapshotError::Validation(SnapshotValidationReason::ReceiverInfo(_)))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn creating_a_snapshot_with_the_same_receiver_info_on_an_mqtt_endpoint_is_accepted() {
    let created = create_with("mqtt://broker:1883/status", r#"[{"key": "Bad Name", "value": "x"}]"#).await;
    assert!(created.is_ok(), "{created:?}");
}

#[tokio::test]
async fn creating_a_snapshot_with_a_good_receiver_info_on_an_http_endpoint_is_accepted() {
    let created = create_with("http://example.com/s", r#"[{"key": "Authorization", "value": "Bearer abc"}]"#).await;
    assert!(created.is_ok(), "{created:?}");
}

#[tokio::test]
async fn patching_a_bad_receiver_info_onto_a_stored_http_endpoint_is_refused() {
    let service = service_holding(stored_snapshot(Some("http://example.com/s"), None));
    let refused = service
        .update(&snapshot_id(), patch(r#"{"receiverInfo": [{"key": "Bad Name", "value": "x"}]}"#))
        .await;
    assert!(
        matches!(refused, Err(SnapshotError::Validation(SnapshotValidationReason::ReceiverInfo(_)))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn patching_the_endpoint_to_http_while_a_bad_receiver_info_is_stored_is_refused() {
    let service = service_holding(stored_snapshot(Some("mqtt://broker:1883/status"), Some(vec![bad_pair()])));
    let refused = service.update(&snapshot_id(), patch(r#"{"endpoint": "http://example.com/s"}"#)).await;
    assert!(
        matches!(refused, Err(SnapshotError::Validation(SnapshotValidationReason::ReceiverInfo(_)))),
        "{refused:?}"
    );
}
