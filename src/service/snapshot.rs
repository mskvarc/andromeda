use crate::{
    core::error::AndromedaError,
    http::error::{batch::BatchEntityError, problem::NgsiLdErrorType},
    repository::snapshot::repo::SnapshotRepository,
    service::{
        csource_forwarding::CsourceForwardingService,
        entity::query::EntityQueryService,
        snapshot_executor::SnapshotExecutor,
        snapshot_prepare::to_insert_params,
        temporal::TemporalService,
    },
    transport::{
        dispatch::{DeliveryResult, TransportDispatcher, build_delivery_request},
        endpoint::EndpointTransport,
    },
};
use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::SnapshotId;
use andromeda_ngsi_ld_model::{
    snapshot::{KeyValuePair, Snapshot, SnapshotInput, SnapshotPatch, SnapshotPriority, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use chrono::Utc;
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use sqlx::postgres::PgPool;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Service for managing NGSI-LD entity snapshots.
pub struct SnapshotService {
    repo: SnapshotRepository,
    transport: Option<Arc<TransportDispatcher>>,
    executor: Option<Arc<SnapshotExecutor>>,
    /// Bounds the number of concurrent snapshot executions so a burst of
    /// `POST /snapshots` calls cannot saturate the database pool.
    execution_semaphore: Arc<Semaphore>,
}

impl SnapshotService {
    /// Create a new snapshot service backed by the given connection pool.
    ///
    /// When `transport` is provided, snapshot status notifications are
    /// dispatched to snapshots that have an `endpoint` configured
    /// (ETSI GS CIM 009 clause 5.16.6).
    pub fn new(
        pool: &PgPool,
        transport: Option<Arc<TransportDispatcher>>,
        entity_query: Arc<EntityQueryService>,
        temporal: Arc<TemporalService>,
        forwarding: Arc<CsourceForwardingService>,
        snapshot_concurrency: usize,
    ) -> SnapshotService {
        let executor = Arc::new(SnapshotExecutor::new(pool, entity_query, temporal, forwarding));
        SnapshotService {
            repo: SnapshotRepository::new(pool),
            transport,
            executor: Some(executor),
            execution_semaphore: Arc::new(Semaphore::new(snapshot_concurrency.max(1))),
        }
    }

    /// Create a new snapshot from the given input parameters.
    pub async fn create(&self, input: SnapshotInput) -> Result<Snapshot, AndromedaError> {
        let id = match input.id {
            Some(snapshot_id) => snapshot_id,
            None => build_snapshot_id()?,
        };

        if self.repo.exists(&id).await? {
            return Err(AndromedaError::snapshot_already_exists(format!("Snapshot {} already exists", id)));
        }

        let priority = match input.snapshot_priority {
            Some(value) => SnapshotPriority::try_from(value).map_err(|e| AndromedaError::snapshot_validation(format!("{}", e)))?,
            None => SnapshotPriority::default(),
        };

        let now = Utc::now();
        let expires_at = compute_snapshot_expires_at(now, input.snapshot_lifetime.as_ref())?;

        let snapshot = Snapshot::builder()
            .id(id)
            .snapshot_queries(input.snapshot_queries)
            .snapshot_temporal_queries(input.snapshot_temporal_queries)
            .snapshot_lifetime(input.snapshot_lifetime)
            .snapshot_priority(priority)
            .endpoint(input.endpoint)
            .receiver_info(input.receiver_info)
            .status(SnapshotStatus::Preparing)
            .queries_details(None)
            .temporal_queries_details(None)
            .created_at(now)
            .modified_at(now)
            .expires_at(expires_at)
            .last_used_at(None)
            .build();

        self.repo.create(to_insert_params(&snapshot)?).await?;
        self.send_notification_if_configured(&snapshot).await;

        // Spawn background query execution (NGSI-LD 5.16.1.4). A semaphore
        // bounds concurrent executions so a burst of create requests cannot
        // saturate the database pool; the spawned task acquires a permit
        // before running and queues behind existing executions otherwise.
        if let Some(executor) = &self.executor {
            let executor = Arc::clone(executor);
            let semaphore = Arc::clone(&self.execution_semaphore);
            let snapshot_for_exec = snapshot.clone();
            tokio::spawn(async move {
                let _permit = match semaphore.acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => return,
                };
                executor.execute(&snapshot_for_exec).await;
            });
        }

        Ok(snapshot)
    }

    /// Retrieve a snapshot by its identifier, returning 404 if expired.
    pub async fn get(&self, id: &SnapshotId) -> Result<Snapshot, AndromedaError> {
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
    pub async fn update(&self, id: &SnapshotId, patch: SnapshotPatch) -> Result<Snapshot, AndromedaError> {
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
    pub async fn delete(&self, id: &SnapshotId) -> Result<(), AndromedaError> {
        let deleted = self.repo.delete(id).await?;
        if deleted == 0 {
            return Err(AndromedaError::snapshot_not_found(format!("Snapshot {} not found", id)));
        }
        Ok(())
    }

    /// Purge snapshots matching a typed filter (ETSI GS CIM 009 clause 5.16.7).
    ///
    /// Matches candidate identifiers, then deletes each row individually so
    /// per-row failures can be surfaced in a 207 Multi-Status response
    /// (clause 6.36.3.2). Returns a [`PurgeOutcome`] describing the result:
    ///
    /// - [`PurgeOutcome::NoMatch`] when no snapshots match (maps to 404).
    /// - [`PurgeOutcome::FullSuccess`] when every matched row was deleted
    ///   (maps to 204).
    /// - [`PurgeOutcome::Partial`] when at least one row succeeded and at
    ///   least one failed (maps to 207).
    pub async fn purge_by_query(&self, filter: &SnapshotPurgeFilter) -> Result<PurgeOutcome, AndromedaError> {
        let matched = self.repo.match_by_filter(filter).await?;
        if matched.is_empty() {
            return Ok(PurgeOutcome::NoMatch);
        }

        let mut successes = Vec::new();
        let mut failures = Vec::new();

        for id in matched {
            match self.repo.delete_one(&id).await {
                Ok(n) if n > 0 => successes.push(id),
                Ok(_) => failures.push(BatchEntityError::from_parse_error(
                    id.as_str(),
                    NgsiLdErrorType::ResourceNotFound,
                    "Snapshot removed concurrently before purge",
                )),
                Err(error) => failures.push(BatchEntityError::from_domain_error(id.as_str(), &error)),
            }
        }

        if failures.is_empty() {
            Ok(PurgeOutcome::FullSuccess)
        } else if successes.is_empty() {
            Ok(PurgeOutcome::AllFailed { failures })
        } else {
            Ok(PurgeOutcome::Partial { successes, failures })
        }
    }

    /// Validate a snapshot for header-based scoping (ETSI GS CIM 009 clause 5.5.15).
    ///
    /// The snapshot must exist, not be expired, and have a queryable status
    /// (`Success` or `Partial`). Touches `last_used_at` on success.
    pub async fn validate_for_scoping(&self, id: &SnapshotId) -> Result<Snapshot, AndromedaError> {
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
    pub async fn get_snapshot_entity(&self, snapshot_id: &SnapshotId, entity_id: &str) -> Result<Option<Value>, AndromedaError> {
        self.repo.get_entity(snapshot_id, entity_id).await
    }

    /// Query entities from a snapshot's frozen data with optional filtering and pagination.
    pub async fn query_snapshot_entities(
        &self,
        snapshot_id: &SnapshotId,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        self.repo.query_entities(snapshot_id, entity_type, entity_ids, limit, offset).await
    }

    /// Return distinct entity types present in a snapshot's frozen data
    /// (ETSI GS CIM 009 clause 5.7.5, scoped via 5.5.15).
    pub async fn snapshot_types(&self, snapshot_id: &SnapshotId) -> Result<Vec<String>, AndromedaError> {
        self.repo.snapshot_distinct_types(snapshot_id).await
    }

    /// Return `(entity_count, attribute_names)` for a type within a snapshot,
    /// or `None` if the type is absent (clause 5.7.6).
    pub async fn snapshot_type_info(&self, snapshot_id: &SnapshotId, type_name: &str) -> Result<Option<(i64, Vec<String>)>, AndromedaError> {
        self.repo.snapshot_type_info(snapshot_id, type_name).await
    }

    /// Return distinct attribute names present across a snapshot's frozen
    /// entities (clause 5.7.7).
    pub async fn snapshot_attributes(&self, snapshot_id: &SnapshotId) -> Result<Vec<String>, AndromedaError> {
        self.repo.snapshot_distinct_attributes(snapshot_id).await
    }

    /// Return `(entity_count, type_names)` for an attribute within a snapshot,
    /// or `None` if the attribute is absent (clause 5.7.8).
    pub async fn snapshot_attribute_info(&self, snapshot_id: &SnapshotId, attr_name: &str) -> Result<Option<(i64, Vec<String>)>, AndromedaError> {
        self.repo.snapshot_attribute_info(snapshot_id, attr_name).await
    }

    /// Retrieve a single temporal entity from a snapshot's frozen data.
    pub async fn get_snapshot_temporal_entity(&self, snapshot_id: &SnapshotId, entity_id: &str) -> Result<Option<Value>, AndromedaError> {
        self.repo.get_temporal_entity(snapshot_id, entity_id).await
    }

    /// Query temporal entities from a snapshot's frozen data with optional filtering and pagination.
    pub async fn query_snapshot_temporal_entities(
        &self,
        snapshot_id: &SnapshotId,
        entity_type: Option<&str>,
        entity_ids: Option<&[String]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        self.repo.query_temporal_entities(snapshot_id, entity_type, entity_ids, limit, offset).await
    }

    /// Clone an existing snapshot, producing a new snapshot with a fresh ID and expiration.
    pub async fn clone_snapshot(&self, source_id: &SnapshotId) -> Result<Snapshot, AndromedaError> {
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

/// Outcome of [`SnapshotService::purge_by_query`] (ETSI GS CIM 009 clause 6.36.3.2).
#[derive(Debug)]
pub enum PurgeOutcome {
    /// No snapshots matched the filter (maps to 404 ResourceNotFound).
    NoMatch,
    /// Every matched snapshot was deleted (maps to 204 No Content).
    FullSuccess,
    /// All matched snapshots failed to delete (still 207 with only errors).
    AllFailed {
        /// Per-snapshot error entries for the multi-status body.
        failures: Vec<BatchEntityError>,
    },
    /// Some succeeded and some failed (maps to 207 Multi-Status).
    Partial {
        /// Identifiers of successfully deleted snapshots.
        successes: Vec<SnapshotId>,
        /// Per-snapshot error entries for the multi-status body.
        failures: Vec<BatchEntityError>,
    },
}

fn build_snapshot_id() -> Result<SnapshotId, AndromedaError> {
    let value = format!("urn:ngsi-ld:Snapshot:{}", uuid::Uuid::new_v4());
    let id = value.parse::<SnapshotId>()?;
    Ok(id)
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
