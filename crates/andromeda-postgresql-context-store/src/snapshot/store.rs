//! `SnapshotStore` trait implementation for `SnapshotRepository`.
//!
//! The implementation dispatches to the statement modules by concern: the
//! control record ([`crate::snapshot::lifecycle`]), the captured rows
//! ([`crate::snapshot::content`]) and the summaries over them
//! ([`crate::snapshot::introspection`]).

use crate::snapshot::{
    clone_context::{promote_context_clone, promote_staged_capture},
    lifecycle::{
        create_snapshot,
        delete_expired_snapshots,
        delete_snapshot,
        match_snapshots_by_filter,
        read_snapshot,
        resolve_snapshot_staging_target,
        resolve_snapshot_target,
        snapshot_exists,
        touch_last_used,
        update_snapshot_metadata,
        update_status,
    },
    preparation_claim::{next_preparing_snapshot, renew_preparation_lease},
    quota::enforce_snapshot_quotas,
    repo::SnapshotRepository,
};
use andromeda_context_space::snapshot_target::SnapshotTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_snapshot::{
    error::SnapshotError,
    store::{InsertSnapshotParams, SnapshotPreparationJob, SnapshotStore},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use uuid::Uuid;

#[async_trait]
impl SnapshotStore for SnapshotRepository {
    async fn create(&self, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
        create_snapshot(self.pool(), params).await
    }

    async fn get(&self, id: &Uri) -> Result<Option<Snapshot>, SnapshotError> {
        read_snapshot(self.pool(), id).await
    }

    async fn exists(&self, id: &Uri) -> Result<bool, SnapshotError> {
        snapshot_exists(self.pool(), id).await
    }

    async fn target(&self, id: &Uri) -> Result<Option<SnapshotTarget>, SnapshotError> {
        resolve_snapshot_target(self.pool(), id).await
    }

    async fn staging_target(&self, id: &Uri) -> Result<Option<SnapshotTarget>, SnapshotError> {
        resolve_snapshot_staging_target(self.pool(), id).await
    }

    async fn next_preparing(&self, claimed_by: Uuid, lease_until: DateTime<Utc>) -> Result<Option<SnapshotPreparationJob>, SnapshotError> {
        next_preparing_snapshot(self.pool(), claimed_by, lease_until).await
    }

    async fn renew_preparation_lease(&self, id: &Uri, claimed_by: Uuid, lease_until: DateTime<Utc>) -> Result<bool, SnapshotError> {
        renew_preparation_lease(self.pool(), id, claimed_by, lease_until).await
    }

    async fn update_metadata(
        &self,
        id: &Uri,
        lifetime: Option<&Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        endpoint: Option<Option<&url::Url>>,
        receiver_info: Option<Option<Value>>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> Result<u64, SnapshotError> {
        update_snapshot_metadata(self.pool(), id, lifetime, priority, endpoint, receiver_info, new_expires_at).await
    }

    async fn update_status(
        &self,
        id: &Uri,
        claimed_by: Uuid,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<u64, SnapshotError> {
        update_status(self.pool(), id, claimed_by, status, queries_details, temporal_queries_details).await
    }

    async fn promote_capture(
        &self,
        id: &Uri,
        claimed_by: Uuid,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<(), SnapshotError> {
        promote_staged_capture(self.pool(), id, claimed_by, status, queries_details, temporal_queries_details).await
    }

    async fn delete(&self, id: &Uri) -> Result<u64, SnapshotError> {
        delete_snapshot(self.pool(), id).await
    }

    async fn match_by_filter(&self, filter: &SnapshotPurgeFilter) -> Result<Vec<Uri>, SnapshotError> {
        match_snapshots_by_filter(self.pool(), filter).await
    }

    async fn delete_one(&self, id: &Uri) -> Result<u64, SnapshotError> {
        delete_snapshot(self.pool(), id).await
    }

    async fn promote_clone(&self, source_id: &Uri, target_id: &Uri, claimed_by: Uuid) -> Result<(), SnapshotError> {
        promote_context_clone(self.pool(), source_id, target_id, claimed_by).await
    }

    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
        delete_expired_snapshots(self.pool(), cutoff).await
    }

    async fn enforce_quotas(&self, count_quota: Option<usize>, byte_quota: Option<u64>) -> Result<Vec<Uri>, SnapshotError> {
        enforce_snapshot_quotas(self.pool(), count_quota, byte_quota).await
    }

    async fn touch_last_used(&self, id: &Uri) -> Result<(), SnapshotError> {
        touch_last_used(self.pool(), id).await
    }
}
