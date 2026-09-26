use crate::{
    branch_condition::BranchCondition,
    error::SnapshotError,
    service::{SnapshotService, SnapshotServiceConfig},
    store_double::{ClaimHold, StoreDouble},
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::snapshot::{Snapshot, SnapshotPriority, SnapshotStatus};
use chrono::{Duration as ChronoDuration, Utc};
use std::{sync::Arc, time::Duration};

const SNAPSHOT_ID: &str = "urn:ngsi-ld:Snapshot:scoped";

fn snapshot_id() -> Uri {
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
