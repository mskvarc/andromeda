use crate::{
    core::error::AndromedaError,
    domain::snapshot::{KeyValuePair, Snapshot, SnapshotInput, SnapshotPatch, SnapshotPriority, SnapshotStatus, validate_snapshot_urn},
    repository::snapshot::SnapshotRepository,
    transport::{
        dispatch::{DeliveryResult, TransportDispatcher, build_delivery_request},
        endpoint::EndpointTransport,
    },
};
use chrono::Utc;
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::sync::Arc;
use urn::Urn;

/// Service for managing NGSI-LD entity snapshots.
pub struct SnapshotService {
    repo: SnapshotRepository,
    transport: Option<Arc<TransportDispatcher>>,
}

impl SnapshotService {
    /// Create a new snapshot service backed by the given connection pool.
    ///
    /// When `transport` is provided, snapshot status notifications are
    /// dispatched to snapshots that have an `endpoint` configured
    /// (ETSI GS CIM 009 clause 5.16.6).
    pub fn new(pool: &PgPool, transport: Option<Arc<TransportDispatcher>>) -> SnapshotService {
        SnapshotService {
            repo: SnapshotRepository::new(pool),
            transport,
        }
    }

    /// Create a new snapshot from the given input parameters.
    pub async fn create(&self, input: SnapshotInput) -> Result<Snapshot, AndromedaError> {
        let id = match input.id {
            Some(snapshot_id) => snapshot_id,
            None => build_snapshot_id()?,
        };
        validate_snapshot_urn(&id).map_err(|e| AndromedaError::snapshot_validation(format!("Invalid snapshot id: {}", e)))?;

        if self.repo.exists(&id).await? {
            return Err(AndromedaError::snapshot_already_exists(format!("Snapshot {} already exists", id)));
        }

        let priority = match input.snapshot_priority {
            Some(value) => SnapshotPriority::try_from(value).map_err(|e| AndromedaError::snapshot_validation(format!("{}", e)))?,
            None => SnapshotPriority::default(),
        };

        let now = Utc::now();
        let expires_at = compute_snapshot_expires_at(now, input.snapshot_lifetime.as_ref())?;

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

        self.repo.create(&snapshot).await?;
        self.send_notification_if_configured(&snapshot).await;

        Ok(snapshot)
    }

    /// Retrieve a snapshot by its identifier, returning 404 if expired.
    pub async fn get(&self, id: &Urn) -> Result<Snapshot, AndromedaError> {
        let snapshot = self
            .repo
            .get(id)
            .await?
            .ok_or_else(|| AndromedaError::snapshot_not_found(format!("Snapshot {} not found", id)))?;

        if snapshot.expires_at < Utc::now() {
            if let Err(e) = self.repo.delete(id).await {
                tracing::error!(error = %e, snapshot_id = %id, "Failed to delete expired snapshot");
            }
            return Err(AndromedaError::snapshot_not_found(format!("Snapshot {} not found", id)));
        }

        // Touch last_used_at
        if let Err(e) = self.repo.touch_last_used(id).await {
            tracing::warn!(error = %e, snapshot_id = %id, "Failed to update snapshot last_used_at");
        }

        Ok(snapshot)
    }

    /// Partially update a snapshot's lifetime or priority.
    pub async fn update(&self, id: &Urn, patch: SnapshotPatch) -> Result<Snapshot, AndromedaError> {
        let existing = self.get(id).await?;

        let patch_priority = match patch.snapshot_priority {
            Some(value) => Some(SnapshotPriority::try_from(value).map_err(|e| AndromedaError::snapshot_validation(format!("{}", e)))?),
            None => None,
        };

        // Recalculate expiresAt if lifetime changed
        let new_expires_at = if let Some(ref lt) = patch.snapshot_lifetime {
            Some(compute_snapshot_expires_at(existing.created_at, Some(lt))?)
        } else {
            None
        };

        self.repo
            .update_lifetime_priority(id, patch.snapshot_lifetime.as_ref(), patch_priority, new_expires_at)
            .await?;

        let updated = self.get(id).await?;
        self.send_notification_if_configured(&updated).await;
        Ok(updated)
    }

    /// Delete a snapshot by its identifier.
    pub async fn delete(&self, id: &Urn) -> Result<(), AndromedaError> {
        let deleted = self.repo.delete(id).await?;
        if !deleted {
            return Err(AndromedaError::snapshot_not_found(format!("Snapshot {} not found", id)));
        }
        Ok(())
    }

    /// Delete all snapshots and return the number of rows removed.
    pub async fn purge(&self) -> Result<u64, AndromedaError> {
        self.repo.delete_all().await
    }

    /// Validate a snapshot for header-based scoping (ETSI GS CIM 009 clause 5.5.15).
    ///
    /// The snapshot must exist, not be expired, and have a queryable status
    /// (`Success` or `Partial`). Touches `last_used_at` on success.
    pub async fn validate_for_scoping(&self, id: &Urn) -> Result<Snapshot, AndromedaError> {
        let snapshot = self
            .repo
            .get(id)
            .await?
            .ok_or_else(|| AndromedaError::snapshot_not_found(format!("Snapshot {} not found", id)))?;

        if snapshot.expires_at < Utc::now() {
            if let Err(e) = self.repo.delete(id).await {
                tracing::error!(error = %e, snapshot_id = %id, "Failed to delete expired snapshot");
            }
            return Err(AndromedaError::snapshot_not_found(format!("Snapshot {} not found", id)));
        }

        match snapshot.status {
            SnapshotStatus::Success | SnapshotStatus::Partial => {}
            SnapshotStatus::Preparing | SnapshotStatus::Empty | SnapshotStatus::Failure => {
                return Err(AndromedaError::snapshot_not_found(format!(
                    "Snapshot {} is not available for querying (status: {})",
                    id, snapshot.status
                )));
            }
        }

        if let Err(e) = self.repo.touch_last_used(id).await {
            tracing::warn!(error = %e, snapshot_id = %id, "Failed to update snapshot last_used_at");
        }

        Ok(snapshot)
    }

    /// Retrieve a single entity from a snapshot's frozen data.
    pub async fn get_snapshot_entity(&self, snapshot_id: &Urn, entity_id: &str) -> Result<Option<Value>, AndromedaError> {
        self.repo.get_entity(snapshot_id, entity_id).await
    }

    /// Query entities from a snapshot's frozen data with optional filtering and pagination.
    pub async fn query_snapshot_entities(
        &self,
        snapshot_id: &Urn,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        self.repo.query_entities(snapshot_id, entity_type, entity_ids, limit, offset).await
    }

    /// Retrieve a single temporal entity from a snapshot's frozen data.
    pub async fn get_snapshot_temporal_entity(&self, snapshot_id: &Urn, entity_id: &str) -> Result<Option<Value>, AndromedaError> {
        self.repo.get_temporal_entity(snapshot_id, entity_id).await
    }

    /// Query temporal entities from a snapshot's frozen data with optional filtering and pagination.
    pub async fn query_snapshot_temporal_entities(
        &self,
        snapshot_id: &Urn,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        self.repo.query_temporal_entities(snapshot_id, entity_type, entity_ids, limit, offset).await
    }

    /// Clone an existing snapshot, producing a new snapshot with a fresh ID and expiration.
    pub async fn clone_snapshot(&self, source_id: &Urn) -> Result<Snapshot, AndromedaError> {
        let source = self.get(source_id).await?;

        let new_id = build_snapshot_id()?;

        let new_expires_at = compute_snapshot_expires_at(Utc::now(), source.snapshot_lifetime.as_ref())?;

        let cloned = self.repo.clone_snapshot(source_id, &new_id, new_expires_at).await?;

        self.send_notification_if_configured(&cloned).await;

        Ok(cloned)
    }

    /// Dispatch a [`SnapshotNotification`] to the snapshot's endpoint, if configured.
    ///
    /// Per ETSI GS CIM 009 clause 5.16.6, notifications are sent when the
    /// `endpoint` member is present. Delivery is fire-and-forget: errors are
    /// logged but never propagated.
    async fn send_notification_if_configured(&self, snapshot: &Snapshot) {
        let endpoint = match snapshot.endpoint.as_ref() {
            Some(uri) => uri.as_str(),
            None => return,
        };

        let transport = match self.transport.as_ref() {
            Some(t) => t,
            None => return,
        };

        let notification = match snapshot.build_notification() {
            Ok(value) => value,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    snapshot_id = %snapshot.id,
                    "Failed to build snapshot notification payload"
                );
                return;
            }
        };

        let payload = match serde_json::to_value(&notification) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    snapshot_id = %snapshot.id,
                    "Failed to serialize snapshot notification"
                );
                return;
            }
        };

        let receiver_info_values = receiver_info_to_values(snapshot.receiver_info.as_deref());
        let request = match build_delivery_request(&payload, endpoint, None, receiver_info_values.as_deref(), None, None, snapshot.id.as_str()) {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    snapshot_id = %snapshot.id,
                    "Failed to build snapshot notification delivery request"
                );
                return;
            }
        };

        match transport.deliver(&EndpointTransport::Http, &request).await {
            DeliveryResult::Delivered => {
                tracing::debug!(snapshot_id = %snapshot.id, "Snapshot notification delivered");
            }
            DeliveryResult::Failed => {
                tracing::warn!(snapshot_id = %snapshot.id, "Snapshot notification delivery failed");
            }
        }
    }
}

fn build_snapshot_id() -> Result<Urn, AndromedaError> {
    let value = format!("urn:ngsi-ld:Snapshot:{}", uuid::Uuid::new_v4());
    value
        .parse::<Urn>()
        .map_err(|e| AndromedaError::internal(format!("Failed to build snapshot id: {}", e)))
}

fn compute_snapshot_expires_at(created_at: chrono::DateTime<Utc>, lifetime: Option<&Iso8601Duration>) -> Result<chrono::DateTime<Utc>, AndromedaError> {
    let parsed = match lifetime {
        Some(value) => *value,
        None => "PT1H"
            .parse::<Iso8601Duration>()
            .map_err(|e| AndromedaError::internal(format!("Invalid default snapshotLifetime: {:?}", e)))?,
    };
    Ok(created_at + parsed)
}

fn receiver_info_to_values(receiver_info: Option<&[KeyValuePair]>) -> Option<Vec<Value>> {
    receiver_info.map(|values| values.iter().map(|item| serde_json::json!({ "key": item.key, "value": item.value })).collect())
}
