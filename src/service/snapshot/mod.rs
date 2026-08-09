/// Ingest-time validation of a snapshot's stored query shapes.
pub mod query_validation;
/// The snapshot store port, and the parameter shapes it takes.
pub mod store;

use crate::{
    error::AndromedaError,
    http::error::batch::entity_error,
    service::{
        snapshot::{
            query_validation::validate_snapshot_query_shapes,
            store::{SnapshotAttributeInfo, SnapshotStore, SnapshotTypeInfo},
        },
        snapshot_executor::SnapshotExecutor,
        snapshot_prepare::to_insert_params,
        temporal::TemporalService,
    },
};
use andromeda_delivery::{dispatch::TransportDispatcher, endpoint::transport::EndpointTransport, request::NotificationDelivery};
use andromeda_entity::query::EntityQueryService;
use andromeda_federation::FederationService;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    snapshot::{Snapshot, SnapshotInput, SnapshotPatch, SnapshotStatus},
    snapshot_filter::SnapshotPurgeFilter,
};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use chrono::Utc;
use iso8601_duration::Duration as Iso8601Duration;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Service for managing NGSI-LD entity snapshots.
pub struct SnapshotService {
    repo: Arc<dyn SnapshotStore>,
    transport: Option<Arc<TransportDispatcher>>,
    executor: Option<Arc<SnapshotExecutor>>,
    /// Bounds the number of concurrent snapshot executions so a burst of
    /// `POST /snapshots` calls cannot saturate the database pool.
    execution_semaphore: Arc<Semaphore>,
}

impl SnapshotService {
    /// Create a new snapshot service backed by the given store.
    ///
    /// When `transport` is provided, snapshot status notifications are
    /// dispatched to snapshots that have an `endpoint` configured
    /// (ETSI GS CIM 009 clause 5.16.6).
    pub fn new(
        repo: Arc<dyn SnapshotStore>,
        transport: Option<Arc<TransportDispatcher>>,
        entity_query: Arc<EntityQueryService>,
        temporal: Arc<TemporalService>,
        forwarding: Arc<FederationService>,
        snapshot_concurrency: usize,
    ) -> SnapshotService {
        let executor = Arc::new(SnapshotExecutor::new(Arc::clone(&repo), entity_query, temporal, forwarding));
        SnapshotService {
            repo,
            transport,
            executor: Some(executor),
            execution_semaphore: Arc::new(Semaphore::new(snapshot_concurrency.max(1))),
        }
    }

    /// Create a new snapshot from the given input parameters.
    pub async fn create(&self, input: SnapshotInput) -> Result<Snapshot, AndromedaError> {
        // Reject a malformed query shape before persistence (clause 5.16.1), so
        // an unparseable `q`, geometry or `timerel` is a 400 rather than a
        // snapshot created 201 that only fails when its queries execute.
        validate_snapshot_query_shapes(input.snapshot_queries.as_deref(), input.snapshot_temporal_queries.as_deref())?;

        let id = match input.id {
            Some(snapshot_id) => snapshot_id,
            None => build_snapshot_id()?,
        };

        if self.repo.exists(&id).await? {
            return Err(AndromedaError::snapshot_already_exists(format!("Snapshot {} already exists", id)));
        }

        let priority = input.snapshot_priority.unwrap_or_default();

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
                executor.execute(snapshot_for_exec).await;
            });
        }

        Ok(snapshot)
    }

    /// Retrieve a snapshot by its identifier, returning 404 if expired.
    pub async fn get(&self, id: &Uri) -> Result<Snapshot, AndromedaError> {
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
    pub async fn update(&self, id: &Uri, patch: SnapshotPatch) -> Result<Snapshot, AndromedaError> {
        let existing = self.get(id).await?;

        let patch_priority = patch.snapshot_priority;

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
    pub async fn delete(&self, id: &Uri) -> Result<(), AndromedaError> {
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
                Ok(_) => failures.push(BatchEntityError::new(
                    id.clone(), // clone: the failure result owns the snapshot identifier
                    NgsiLdErrorType::ResourceNotFound,
                    "Snapshot removed concurrently before purge",
                )),
                Err(error) => failures.push(entity_error(id, &error)),
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
    pub async fn validate_for_scoping(&self, id: &Uri) -> Result<Snapshot, AndromedaError> {
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
    pub async fn get_snapshot_entity(&self, snapshot_id: &Uri, entity_id: &Uri) -> Result<Option<Value>, AndromedaError> {
        self.repo.get_entity(snapshot_id, entity_id).await
    }

    /// Query entities from a snapshot's frozen data with optional filtering and pagination.
    pub async fn query_snapshot_entities(
        &self,
        snapshot_id: &Uri,
        entity_type: Option<&TypeExprRaw>,
        entity_ids: Option<&[Uri]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        self.repo.query_entities(snapshot_id, entity_type, entity_ids, limit, offset).await
    }

    /// Return distinct entity types present in a snapshot's frozen data
    /// (ETSI GS CIM 009 clause 5.7.5, scoped via 5.5.15).
    pub async fn snapshot_types(&self, snapshot_id: &Uri) -> Result<Vec<EntityTypeName>, AndromedaError> {
        self.repo.snapshot_distinct_types(snapshot_id).await
    }

    /// Return `(entity_count, attribute_names)` for a type within a snapshot,
    /// or `None` if the type is absent (clause 5.7.6).
    pub async fn snapshot_type_info(&self, snapshot_id: &Uri, type_name: &EntityTypeName) -> Result<Option<SnapshotTypeInfo>, AndromedaError> {
        self.repo.snapshot_type_info(snapshot_id, type_name).await
    }

    /// Return distinct attribute names present across a snapshot's frozen
    /// entities (clause 5.7.7).
    pub async fn snapshot_attributes(&self, snapshot_id: &Uri) -> Result<Vec<AttributeName>, AndromedaError> {
        self.repo.snapshot_distinct_attributes(snapshot_id).await
    }

    /// Return `(entity_count, type_names)` for an attribute within a snapshot,
    /// or `None` if the attribute is absent (clause 5.7.8).
    pub async fn snapshot_attribute_info(&self, snapshot_id: &Uri, attr_name: &AttributeName) -> Result<Option<SnapshotAttributeInfo>, AndromedaError> {
        self.repo.snapshot_attribute_info(snapshot_id, attr_name).await
    }

    /// Retrieve a single temporal entity from a snapshot's frozen data.
    pub async fn get_snapshot_temporal_entity(&self, snapshot_id: &Uri, entity_id: &Uri) -> Result<Option<Value>, AndromedaError> {
        self.repo.get_temporal_entity(snapshot_id, entity_id).await
    }

    /// Query temporal entities from a snapshot's frozen data with optional filtering and pagination.
    pub async fn query_snapshot_temporal_entities(
        &self,
        snapshot_id: &Uri,
        entity_type: Option<&TypeExprRaw>,
        entity_ids: Option<&[Uri]>,
        limit: i64,
        offset: i64,
    ) -> Result<(Vec<Value>, i64), AndromedaError> {
        self.repo.query_temporal_entities(snapshot_id, entity_type, entity_ids, limit, offset).await
    }

    /// Clone an existing snapshot, producing a new snapshot with a fresh ID and expiration.
    pub async fn clone_snapshot(&self, source_id: &Uri) -> Result<Snapshot, AndromedaError> {
        let source = self.get(source_id).await?;

        let new_id = build_snapshot_id()?;

        let new_expires_at = compute_snapshot_expires_at(Utc::now(), source.snapshot_lifetime.as_ref())?;

        let cloned = self.repo.clone_snapshot(source_id, &new_id, new_expires_at).await?;

        self.send_notification_if_configured(&cloned).await;

        Ok(cloned)
    }

    /// Dispatch a snapshot notification to the snapshot's endpoint, if configured.
    ///
    /// Per ETSI GS CIM 009 clause 5.16.6, notifications are sent when the
    /// `endpoint` member is present. Delivery is fire-and-forget: errors are
    /// logged but never propagated.
    async fn send_notification_if_configured(&self, snapshot: &Snapshot) {
        let endpoint = match snapshot.endpoint.as_ref() {
            Some(uri) => uri,
            None => return,
        };

        let transport = match self.transport.as_ref() {
            Some(t) => t,
            None => return,
        };

        let notification = snapshot.build_notification();

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

        // The snapshot notification has no subscription; its own id is the
        // routing and logging label the delivery carries. clone: the conversion
        // consumes the id and the snapshot is borrowed here.
        let subscription_id = snapshot.id.clone();
        let request = (NotificationDelivery {
            payload: &payload,
            endpoint_uri: endpoint,
            accept: None,
            receiver_info: snapshot.receiver_info.as_deref(),
            jsonld_context: None,
            tenant: None,
            subscription_id: &subscription_id,
        })
        .assemble();

        if transport.deliver(&EndpointTransport::Http, &request).await.is_delivered() {
            tracing::debug!(snapshot_id = %snapshot.id, "Snapshot notification delivered");
        } else {
            tracing::warn!(snapshot_id = %snapshot.id, "Snapshot notification delivery failed");
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
        successes: Vec<Uri>,
        /// Per-snapshot error entries for the multi-status body.
        failures: Vec<BatchEntityError>,
    },
}

fn build_snapshot_id() -> Result<Uri, AndromedaError> {
    let value = format!("urn:ngsi-ld:Snapshot:{}", uuid::Uuid::new_v4());
    value
        .parse()
        .map_err(|error| AndromedaError::internal(format!("Generated snapshot id is not a valid URI: {error}")))
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
