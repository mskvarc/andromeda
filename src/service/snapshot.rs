use crate::{
    domain::snapshot::{ExecutionResultDetails, Snapshot, SnapshotInput, SnapshotPatch, SnapshotStatus},
    error::AndromedaError,
    repository::snapshot::SnapshotRepository,
    service::entity_map::parse_iso8601_duration,
};
use chrono::Utc;
use sqlx::postgres::PgPool;

pub struct SnapshotService {
    repo: SnapshotRepository,
}

impl SnapshotService {
    pub fn new(pool: &PgPool) -> Self {
        Self {
            repo: SnapshotRepository::new(pool),
        }
    }

    pub async fn create(&self, input: SnapshotInput) -> Result<Snapshot, AndromedaError> {
        let id = input.id.unwrap_or_else(|| format!("urn:ngsi-ld:Snapshot:{}", uuid::Uuid::new_v4()));

        if self.repo.exists(&id).await.map_err(AndromedaError::from)? {
            return Err(AndromedaError::already_exists(format!("Snapshot {} already exists", id)));
        }

        let priority = input.snapshot_priority.unwrap_or(5);
        if !(1..=10).contains(&priority) {
            return Err(AndromedaError::bad_request_data("snapshotPriority must be between 1 and 10"));
        }

        let lifetime = input.snapshot_lifetime.as_deref().or(Some("PT1H"));
        let duration = parse_iso8601_duration(lifetime)?;
        let now = Utc::now();
        let expires_at = now + duration;

        let snapshot = Snapshot {
            id,
            snapshot_queries: input.snapshot_queries,
            snapshot_temporal_queries: input.snapshot_temporal_queries,
            snapshot_lifetime: input.snapshot_lifetime,
            snapshot_priority: priority,
            endpoint: input.endpoint,
            receiver_info: input.receiver_info,
            status: SnapshotStatus::Preparing,
            queries_details: None,
            temporal_queries_details: None,
            created_at: now,
            modified_at: now,
            expires_at,
            last_used_at: None,
        };

        self.repo.create(&snapshot).await.map_err(AndromedaError::from)?;

        Ok(snapshot)
    }

    pub async fn get(&self, id: &str) -> Result<Snapshot, AndromedaError> {
        let snapshot = self
            .repo
            .get(id)
            .await
            .map_err(AndromedaError::from)?
            .ok_or_else(|| AndromedaError::not_found(format!("Snapshot {} not found", id)))?;

        if snapshot.expires_at < Utc::now() {
            let _ = self.repo.delete(id).await;
            return Err(AndromedaError::not_found(format!("Snapshot {} not found", id)));
        }

        // Touch last_used_at
        let _ = self.repo.touch_last_used(id).await;

        Ok(snapshot)
    }

    pub async fn update(&self, id: &str, patch: SnapshotPatch) -> Result<Snapshot, AndromedaError> {
        let existing = self.get(id).await?;

        if let Some(p) = patch.snapshot_priority
            && !(1..=10).contains(&p)
        {
            return Err(AndromedaError::bad_request_data("snapshotPriority must be between 1 and 10"));
        }

        // Recalculate expiresAt if lifetime changed
        let new_expires_at = if let Some(ref lt) = patch.snapshot_lifetime {
            let duration = parse_iso8601_duration(Some(lt))?;
            Some(existing.created_at + duration)
        } else {
            None
        };

        self.repo
            .update_lifetime_priority(id, patch.snapshot_lifetime.as_deref(), patch.snapshot_priority, new_expires_at)
            .await
            .map_err(AndromedaError::from)?;

        self.get(id).await
    }

    pub async fn delete(&self, id: &str) -> Result<(), AndromedaError> {
        let deleted = self.repo.delete(id).await.map_err(AndromedaError::from)?;
        if !deleted {
            return Err(AndromedaError::not_found(format!("Snapshot {} not found", id)));
        }
        Ok(())
    }

    pub async fn purge(&self) -> Result<u64, AndromedaError> {
        self.repo.delete_all().await.map_err(AndromedaError::from)
    }

    pub async fn clone_snapshot(&self, source_id: &str) -> Result<Snapshot, AndromedaError> {
        let source = self.get(source_id).await?;

        let new_id = format!("urn:ngsi-ld:Snapshot:{}", uuid::Uuid::new_v4());

        let lifetime = source.snapshot_lifetime.as_deref().or(Some("PT1H"));
        let duration = parse_iso8601_duration(lifetime)?;
        let new_expires_at = Utc::now() + duration;

        let cloned = self
            .repo
            .clone_snapshot(source_id, &new_id, new_expires_at)
            .await
            .map_err(AndromedaError::from)?;

        Ok(cloned)
    }

    pub async fn update_status(
        &self,
        id: &str,
        status: SnapshotStatus,
        queries_details: Option<Vec<ExecutionResultDetails>>,
        temporal_queries_details: Option<Vec<ExecutionResultDetails>>,
    ) -> Result<(), AndromedaError> {
        self.repo
            .update_status(id, &status, queries_details.as_deref(), temporal_queries_details.as_deref())
            .await
            .map_err(AndromedaError::from)?;
        Ok(())
    }

    pub fn repo(&self) -> &SnapshotRepository {
        &self.repo
    }
}
