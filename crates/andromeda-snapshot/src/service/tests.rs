use crate::{
    branch_condition::BranchCondition,
    error::SnapshotError,
    service::{SnapshotService, SnapshotServiceConfig},
    store_double::{ClaimHold, StoreDouble},
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    snapshot::{Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_clone::SnapshotCloneInput,
    snapshot_input::SnapshotInput,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::{sync::Arc, time::Duration};

const SNAPSHOT_ID: &str = "urn:ngsi-ld:Snapshot:scoped";

fn snapshot_id() -> Identifier {
    SNAPSHOT_ID.parse().expect("valid snapshot identifier")
}

fn service(condition: BranchCondition) -> SnapshotService {
    let now = Utc::now();
    let snapshot = Snapshot::builder()
        .id(snapshot_id())
        .snapshot_priority(SnapshotPriority::default())
        .status(SnapshotStatus::Success)
        .created_at(now)
        .modified_at(now)
        .expires_at(now + ChronoDuration::hours(1))
        .build();
    SnapshotService::new(
        Arc::new(StoreDouble::new(snapshot, condition, ClaimHold::Held)),
        None,
        None,
        SnapshotServiceConfig {
            default_lifetime: Duration::from_hours(1),
            maximum_lifetime: Duration::from_hours(24),
            scoped_operation_wait: Duration::from_millis(50),
            tenant_count_quota: None,
            tenant_byte_quota: None,
        },
    )
}

/// A Snapshot whose branch is behind the binary is refused for scoping with
/// an error naming it, not reported absent: it exists and is still reaped,
/// it just cannot be served until its branch upgrade succeeds.
#[tokio::test]
async fn a_quarantined_snapshot_is_refused_for_scoping_by_name() {
    let refused = service(BranchCondition::Quarantined).validate_for_scoping(&snapshot_id()).await;
    match refused {
        Err(SnapshotError::Quarantined { identifier }) => assert_eq!(identifier, snapshot_id()),
        other => panic!("a quarantined snapshot must be refused as quarantined, got {other:?}"),
    }
}

/// A current branch with a prepared status scopes the request.
#[tokio::test]
async fn a_current_prepared_snapshot_scopes_the_request() {
    let scoped = service(BranchCondition::Current).validate_for_scoping(&snapshot_id()).await;
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
    let service = service(BranchCondition::Current);
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
    let service = service(BranchCondition::Current);
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
