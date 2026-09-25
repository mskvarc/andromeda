use crate::{
    branch_condition::BranchCondition,
    error::SnapshotError,
    service::{SnapshotService, SnapshotServiceConfig},
    store::{InsertSnapshotParams, SnapshotPreparationJob, SnapshotStore},
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::snapshot_target::SnapshotTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures::future::BoxFuture;
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use url::Url;

const SNAPSHOT_ID: &str = "urn:ngsi-ld:Snapshot:scoped";

/// A store holding one successfully prepared Snapshot whose branch is in
/// `condition`; every operation scoping does not use fails loudly.
struct OneSnapshotStore {
    snapshot: Snapshot,
    condition: BranchCondition,
}

fn unused<T: Send + 'static>(method: &'static str) -> BoxFuture<'static, Result<T, SnapshotError>> {
    Box::pin(async move {
        Err(SnapshotError::internal(format!(
            "OneSnapshotStore::{method} is not exercised by the scoping tests"
        )))
    })
}

impl SnapshotStore for OneSnapshotStore {
    fn get<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<Snapshot>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let found = (self.snapshot.id == *id).then(|| self.snapshot.clone()); // clone: the port hands out an owned Snapshot
        Box::pin(async move { Ok(found) })
    }

    fn branch_condition<'life0, 'life1, 'async_trait>(&'life0 self, id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<BranchCondition>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        let condition = (self.snapshot.id == *id).then_some(self.condition);
        Box::pin(async move { Ok(condition) })
    }

    fn touch_last_used<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        Box::pin(async { Ok(()) })
    }

    fn delete<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("delete")
    }

    fn create<'life0, 'life1, 'async_trait>(&'life0 self, _params: InsertSnapshotParams<'life1>) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("create")
    }

    fn exists<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<bool, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("exists")
    }

    fn target<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<SnapshotTarget>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("target")
    }

    fn staging_target<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<Option<SnapshotTarget>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("staging_target")
    }

    fn next_preparing<'life0, 'async_trait>(
        &'life0 self,
        _claimed_by: LeaseHolder,
        _lease_until: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<Option<SnapshotPreparationJob>, SnapshotError>>
    where
        'life0: 'async_trait,
    {
        unused("next_preparing")
    }

    fn renew_preparation_lease<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _id: &'life1 Uri,
        _claimed_by: LeaseHolder,
        _lease_until: DateTime<Utc>,
    ) -> BoxFuture<'async_trait, Result<bool, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("renew_preparation_lease")
    }

    fn update_metadata<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        _id: &'life1 Uri,
        _lifetime: Option<&'life2 Iso8601Duration>,
        _priority: Option<SnapshotPriority>,
        _endpoint: Option<Option<&'life3 Url>>,
        _receiver_info: Option<Option<Value>>,
        _new_expires_at: Option<DateTime<Utc>>,
    ) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
    {
        unused("update_metadata")
    }

    fn update_status<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        _id: &'life1 Uri,
        _claimed_by: LeaseHolder,
        _status: SnapshotStatus,
        _queries_details: Option<&'life2 [ExecutionResultDetails]>,
        _temporal_queries_details: Option<&'life3 [ExecutionResultDetails]>,
    ) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
    {
        unused("update_status")
    }

    fn promote_capture<'life0, 'life1, 'life2, 'life3, 'async_trait>(
        &'life0 self,
        _id: &'life1 Uri,
        _claimed_by: LeaseHolder,
        _status: SnapshotStatus,
        _queries_details: Option<&'life2 [ExecutionResultDetails]>,
        _temporal_queries_details: Option<&'life3 [ExecutionResultDetails]>,
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
        'life3: 'async_trait,
    {
        unused("promote_capture")
    }

    fn match_by_filter<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _filter: &'life1 SnapshotPurgeFilter,
    ) -> BoxFuture<'async_trait, Result<Vec<Uri>, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("match_by_filter")
    }

    fn delete_one<'life0, 'life1, 'async_trait>(&'life0 self, _id: &'life1 Uri) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
    {
        unused("delete_one")
    }

    fn promote_clone<'life0, 'life1, 'life2, 'async_trait>(
        &'life0 self,
        _source_id: &'life1 Uri,
        _target_id: &'life2 Uri,
        _claimed_by: LeaseHolder,
    ) -> BoxFuture<'async_trait, Result<(), SnapshotError>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        'life2: 'async_trait,
    {
        unused("promote_clone")
    }

    fn delete_expired<'life0, 'async_trait>(&'life0 self, _cutoff: DateTime<Utc>) -> BoxFuture<'async_trait, Result<u64, SnapshotError>>
    where
        'life0: 'async_trait,
    {
        unused("delete_expired")
    }

    fn enforce_quotas<'life0, 'async_trait>(
        &'life0 self,
        _count_quota: Option<usize>,
        _byte_quota: Option<u64>,
    ) -> BoxFuture<'async_trait, Result<Vec<Uri>, SnapshotError>>
    where
        'life0: 'async_trait,
    {
        unused("enforce_quotas")
    }
}

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
        Arc::new(OneSnapshotStore { snapshot, condition }),
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
