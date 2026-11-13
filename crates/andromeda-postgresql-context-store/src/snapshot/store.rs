//! `SnapshotStore` trait implementation for `SnapshotRepository`.
//!
//! The implementation dispatches to the statement modules by concern: creation,
//! the control record and its preparation state, reaping, preparation claims,
//! publication and quotas.

use crate::snapshot::{
    clone_context::{promote_context_clone, promote_staged_branch},
    creation::create_snapshot,
    metadata_change::MetadataChange,
    preparation_claim::{claim_created_snapshot, next_preparing_snapshot, release_preparation, renew_preparation_lease, restart_preparation},
    published_targets::read_published_targets,
    quota::enforce_snapshot_quotas,
    reap::{delete_expired_snapshots, delete_snapshot},
    record::{
        read_purge_candidates,
        read_snapshot,
        resolve_snapshot_staging_target,
        resolve_snapshot_target,
        snapshot_exists,
        touch_last_used,
        update_snapshot_metadata,
        update_status,
    },
    repo::SnapshotRepository,
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::snapshot_target::SnapshotTarget;
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier};
use andromeda_ngsi_ld_model::{
    key_value_pair::KeyValuePair,
    snapshot::{ExecutionResultDetails, Snapshot, SnapshotPriority, SnapshotStatus},
    snapshot_filter::PurgeCandidate,
};
use andromeda_snapshot::{
    error::SnapshotError,
    store::{InsertSnapshotParams, SnapshotPreparationJob, SnapshotStore},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::time::Duration;

#[async_trait]
impl SnapshotStore for SnapshotRepository {
    async fn create(&self, params: InsertSnapshotParams<'_>) -> Result<(), SnapshotError> {
        create_snapshot(self.pool(), params).await
    }

    async fn get(&self, id: &Identifier) -> Result<Option<Snapshot>, SnapshotError> {
        read_snapshot(self.pool(), id).await
    }

    async fn exists(&self, id: &Identifier) -> Result<bool, SnapshotError> {
        snapshot_exists(self.pool(), id).await
    }

    async fn target(&self, id: &Identifier) -> Result<Option<SnapshotTarget>, SnapshotError> {
        resolve_snapshot_target(self.pool(), id).await
    }

    async fn published_targets(&self) -> Result<Vec<SnapshotTarget>, SnapshotError> {
        read_published_targets(self.pool()).await
    }

    async fn staging_target(&self, id: &Identifier) -> Result<Option<SnapshotTarget>, SnapshotError> {
        resolve_snapshot_staging_target(self.pool(), id).await
    }

    async fn next_preparing(&self, claimed_by: LeaseHolder, lease: Duration) -> Result<Option<SnapshotPreparationJob>, SnapshotError> {
        next_preparing_snapshot(self.pool(), claimed_by, lease).await
    }

    async fn claim_created(&self, id: &Identifier, claimed_by: LeaseHolder, lease: Duration) -> Result<Option<SnapshotPreparationJob>, SnapshotError> {
        claim_created_snapshot(self.pool(), id, claimed_by, lease).await
    }

    async fn renew_preparation_lease(&self, id: &Identifier, claimed_by: LeaseHolder, lease: Duration) -> Result<bool, SnapshotError> {
        renew_preparation_lease(self.pool(), id, claimed_by, lease).await
    }

    async fn restart_preparation(&self, id: &Identifier, claimed_by: LeaseHolder, lease: Duration) -> Result<bool, SnapshotError> {
        restart_preparation(self.pool(), id, claimed_by, lease).await
    }

    async fn release_preparation(&self, id: &Identifier, claimed_by: LeaseHolder) -> Result<(), SnapshotError> {
        release_preparation(self.pool(), id, claimed_by).await
    }

    async fn update_metadata(
        &self,
        id: &Identifier,
        lifetime: Option<&Iso8601Duration>,
        priority: Option<SnapshotPriority>,
        endpoint: Option<Option<&url::Url>>,
        receiver_info: Option<Option<Vec<KeyValuePair>>>,
        new_expires_at: Option<DateTime<Utc>>,
    ) -> Result<u64, SnapshotError> {
        update_snapshot_metadata(
            self.pool(),
            id,
            lifetime,
            priority,
            MetadataChange::from(endpoint),
            MetadataChange::from(receiver_info),
            new_expires_at,
        )
        .await
    }

    async fn update_status(
        &self,
        id: &Identifier,
        claimed_by: LeaseHolder,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<u64, SnapshotError> {
        update_status(self.pool(), id, claimed_by, status, queries_details, temporal_queries_details).await
    }

    async fn promote_capture(
        &self,
        id: &Identifier,
        claimed_by: LeaseHolder,
        status: SnapshotStatus,
        queries_details: Option<&[ExecutionResultDetails]>,
        temporal_queries_details: Option<&[ExecutionResultDetails]>,
    ) -> Result<(), SnapshotError> {
        promote_staged_branch(self.pool(), id, claimed_by, status, queries_details, temporal_queries_details).await
    }

    async fn delete(&self, id: &Identifier) -> Result<u64, SnapshotError> {
        delete_snapshot(self.pool(), id).await
    }

    async fn purge_candidates(&self) -> Result<Vec<PurgeCandidate>, SnapshotError> {
        read_purge_candidates(self.pool()).await
    }

    async fn delete_one(&self, id: &Identifier) -> Result<u64, SnapshotError> {
        delete_snapshot(self.pool(), id).await
    }

    async fn promote_clone(&self, source_id: &Identifier, target_id: &Identifier, claimed_by: LeaseHolder) -> Result<(), SnapshotError> {
        promote_context_clone(self.pool(), source_id, target_id, claimed_by).await
    }

    async fn delete_expired(&self, cutoff: DateTime<Utc>) -> Result<u64, SnapshotError> {
        delete_expired_snapshots(self.pool(), cutoff).await
    }

    async fn enforce_quotas(&self, count_quota: Option<usize>, byte_quota: Option<u64>) -> Result<Vec<Identifier>, SnapshotError> {
        enforce_snapshot_quotas(self.pool(), count_quota, byte_quota).await
    }

    async fn touch_last_used(&self, id: &Identifier) -> Result<(), SnapshotError> {
        touch_last_used(self.pool(), id).await
    }
}
