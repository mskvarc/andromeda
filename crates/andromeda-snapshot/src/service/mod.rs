//! The Snapshot lifecycle service: create, read, update, delete and clone.
//!
//! Scoping a request to a Snapshot lives in [`scoping`], purging by query in
//! [`purge`], and the lifetime arithmetic both create and clone share in
//! [`lifetime`].

/// The lifetime a Snapshot is created or patched with.
mod lifetime;
/// Purging Snapshots by query (clause 5.16.7).
pub mod purge;
/// Resolving the Snapshot a request is scoped to (clause 5.5.15).
mod scoping;

use crate::{
    branch_condition::BranchCondition,
    error::SnapshotError,
    executor::{SnapshotPreparation, SnapshotPreparationOutcome},
    prepare::to_insert_params,
    query_validation::validate_snapshot_query_shapes,
    service::lifetime::{effective_snapshot_lifetime, iso8601_lifetime},
    status_delivery_signal::StatusDeliverySignal,
    store::SnapshotStore,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    snapshot::{Snapshot, SnapshotStatus},
    snapshot_clone::SnapshotCloneInput,
    snapshot_input::SnapshotInput,
    snapshot_patch::{MergePatchValue, SnapshotPatch},
};
use chrono::Utc;
use std::{sync::Arc, time::Duration};
use tracing::{info, warn};

/// Lifetime and tenant-quota policy for a [`SnapshotService`].
#[derive(Clone, Copy, Debug)]
pub struct SnapshotServiceConfig {
    /// Lifetime applied when a create request omits `snapshotLifetime`.
    pub default_lifetime: Duration,
    /// Upper bound a requested lifetime is capped to.
    pub maximum_lifetime: Duration,
    /// How long a scoped operation waits for a still-`preparing` Snapshot.
    pub scoped_operation_wait: Duration,
    /// Optional per-tenant Snapshot count quota.
    pub tenant_count_quota: Option<usize>,
    /// Optional per-tenant Snapshot byte quota.
    pub tenant_byte_quota: Option<u64>,
}

/// Service for managing NGSI-LD entity snapshots.
pub struct SnapshotService {
    /// The Snapshot store.
    repo: Arc<dyn SnapshotStore>,
    /// The bounded executor a create kicks.
    preparation: Option<Arc<SnapshotPreparation>>,
    /// Wakes the durable status-delivery drain.
    delivery_signal: Option<Arc<dyn StatusDeliverySignal>>,
    /// Lifetime, wait and quota policy.
    config: SnapshotServiceConfig,
}

impl SnapshotService {
    /// Create a new snapshot service backed by the given store. `preparation`
    /// is the bounded executor [`SnapshotService::prepare_now`] kicks so a
    /// freshly created snapshot does not wait for the next scheduled sweep.
    /// `delivery_signal` wakes the durable status-delivery drain after a
    /// create-time preparation or a metadata update enqueues notifications, so
    /// they are not left to the drain's idle fallback interval.
    pub const fn new(
        repo: Arc<dyn SnapshotStore>,
        preparation: Option<Arc<SnapshotPreparation>>,
        delivery_signal: Option<Arc<dyn StatusDeliverySignal>>,
        config: SnapshotServiceConfig,
    ) -> SnapshotService {
        SnapshotService {
            repo,
            preparation,
            delivery_signal,
            config,
        }
    }

    /// Start one preparation pass in the background. The scheduled sweep
    /// remains the guarantee that every `preparing` snapshot is executed;
    /// this only removes the sweep cadence as the floor on how soon a new
    /// snapshot becomes usable (clause 5.16.1.4 executes the queries on
    /// creation). A pass racing the sweep is harmless: `update_status` only
    /// advances a row that is still `preparing`.
    pub fn prepare_now(&self) {
        let Some(preparation) = &self.preparation else {
            return;
        };
        let preparation = Arc::clone(preparation);
        // clone: the spawned pass owns its own handle to the delivery signal,
        // which it raises once it has enqueued this creation's notifications.
        let delivery_signal = self.delivery_signal.clone();
        tokio::spawn(async move {
            match preparation.execute().await {
                Ok(SnapshotPreparationOutcome::Idle) => {}
                Ok(SnapshotPreparationOutcome::Prepared { count }) => {
                    info!(count, "snapshot preparation completed on create");
                    // The preparation committed its status rows before returning,
                    // so wake delivery now instead of leaving them to the drain's
                    // idle fallback (the scheduled sweep will not, having found no
                    // preparing Snapshot left to complete).
                    if let Some(delivery_signal) = &delivery_signal {
                        delivery_signal.signal();
                    }
                }
                Err(error) => warn!(%error, "snapshot preparation on create failed; the scheduled sweep will retry"),
            }
        });
    }

    /// Create a new snapshot from the given input parameters.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Validation`] when both query collections are
    /// absent, when a query occupies the wrong collection, or when the requested
    /// lifetime is not positive; [`SnapshotError::AlreadyExists`] when the
    /// requested identifier is already taken; [`SnapshotError::Internal`] when a
    /// generated identifier, the configured default lifetime or the request
    /// context cannot be materialised; and the store's own failure when the
    /// record cannot be persisted or the tenant quotas cannot be enforced.
    pub async fn create(&self, input: SnapshotInput, context: ContextSpec) -> Result<Snapshot, SnapshotError> {
        if input.snapshot_queries.is_none() && input.snapshot_temporal_queries.is_none() {
            return Err(SnapshotError::validation(
                "At least one of snapshotQueries or snapshotTemporalQueries must be present",
            ));
        }
        // Reject a malformed query shape before persistence (clause 5.16.1), so
        // an unparseable `q`, geometry or `timerel` is a 400 rather than a
        // snapshot created 201 that only fails when its queries execute.
        validate_snapshot_query_shapes(input.snapshot_queries.as_deref(), input.snapshot_temporal_queries.as_deref())?;

        let id = match input.id {
            Some(snapshot_id) => snapshot_id,
            None => build_snapshot_id()?,
        };

        if self.repo.exists(&id).await? {
            return Err(SnapshotError::already_exists(&id));
        }

        let priority = input.snapshot_priority.unwrap_or_default();

        let now = Utc::now();
        let requested_lifetime = match input.snapshot_lifetime {
            Some(lifetime) => lifetime,
            None => iso8601_lifetime(self.config.default_lifetime)?,
        };
        let (snapshot_lifetime, expires_at) = effective_snapshot_lifetime(now, &requested_lifetime, self.config.maximum_lifetime)?;

        let snapshot = Snapshot::builder()
            .id(id)
            .snapshot_queries(input.snapshot_queries)
            .snapshot_temporal_queries(input.snapshot_temporal_queries)
            .snapshot_lifetime(Some(snapshot_lifetime))
            .snapshot_priority(priority)
            .endpoint(input.endpoint)
            .receiver_info(input.receiver_info)
            .status(SnapshotStatus::Preparing)
            .queries_details(None)
            .temporal_queries_details(None)
            .created_at(now)
            .modified_at(now)
            .expires_at(expires_at)
            .last_used_at(Some(now))
            .build();

        let query_context = context
            .to_context_value()
            .ok_or_else(|| SnapshotError::internal("Snapshot request context could not be materialized"))?;
        self.repo.create(to_insert_params(&snapshot, query_context)?).await?;
        self.enforce_quotas().await?;

        Ok(snapshot)
    }

    /// Retrieve a snapshot by its identifier, returning 404 if expired.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::NotFound`] when no record exists or the record
    /// has expired, and the store's own failure when the read does not complete.
    /// Failing to delete an expired record or to touch `last_used_at` is logged
    /// rather than returned.
    pub async fn get(&self, id: &Identifier) -> Result<Snapshot, SnapshotError> {
        let snapshot = self.repo.get(id).await?.ok_or_else(|| SnapshotError::not_found(id))?;

        if snapshot.expires_at < Utc::now() {
            if let Err(e) = self.repo.delete(id).await {
                tracing::error!(error = %e, snapshot_id = %id, "Failed to delete expired snapshot");
            }
            return Err(SnapshotError::not_found(id));
        }

        // Touch last_used_at
        if let Err(e) = self.repo.touch_last_used(id).await {
            tracing::warn!(error = %e, snapshot_id = %id, "Failed to update snapshot last_used_at");
        }

        Ok(snapshot)
    }

    /// Partially update a snapshot's lifetime or priority.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::NotFound`] when the snapshot is absent or
    /// expired, [`SnapshotError::Validation`] when the patch changes the
    /// identifier or either query collection or supplies a non-positive
    /// lifetime, [`SnapshotError::Representation`] when the patched receiver
    /// info cannot be serialised, and the store's own failure when the metadata
    /// write does not complete.
    pub async fn update(&self, id: &Identifier, patch: SnapshotPatch) -> Result<Snapshot, SnapshotError> {
        self.get(id).await?;

        if patch.id.as_ref().is_some_and(|patch_id| patch_id != id) {
            return Err(SnapshotError::validation("Snapshot id is immutable"));
        }
        if !matches!(patch.snapshot_queries, MergePatchValue::Absent) || !matches!(patch.snapshot_temporal_queries, MergePatchValue::Absent) {
            return Err(SnapshotError::validation("Snapshot query definitions are immutable"));
        }

        let patch_priority = patch.snapshot_priority;
        let endpoint = match &patch.endpoint {
            MergePatchValue::Absent => None,
            MergePatchValue::Null => Some(None),
            MergePatchValue::Value(endpoint) => Some(Some(endpoint)),
        };
        let receiver_info = match &patch.receiver_info {
            MergePatchValue::Absent => None,
            MergePatchValue::Null => Some(None),
            MergePatchValue::Value(receiver_info) => Some(Some(serde_json::to_value(receiver_info)?)),
        };

        // Recalculate expiresAt from patch time and persist the capped effective lifetime.
        let effective_lifetime = patch
            .snapshot_lifetime
            .as_ref()
            .map(|lifetime| effective_snapshot_lifetime(Utc::now(), lifetime, self.config.maximum_lifetime))
            .transpose()?;
        let persisted_lifetime = effective_lifetime.as_ref().map(|(lifetime, _)| lifetime);
        let new_expires_at = effective_lifetime.as_ref().map(|(_, expires_at)| *expires_at);

        self.repo
            .update_metadata(id, persisted_lifetime, patch_priority, endpoint, receiver_info, new_expires_at)
            .await?;

        // A metadata update commits a fresh status notification for an
        // endpoint-bearing Snapshot, outside the preparation sweep that would
        // otherwise wake delivery, so signal the drain directly.
        if let Some(delivery_signal) = &self.delivery_signal {
            delivery_signal.signal();
        }

        self.get(id).await
    }

    /// Delete a snapshot by its identifier.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::NotFound`] when no row was removed, and the
    /// store's own failure when the deletion does not complete.
    pub async fn delete(&self, id: &Identifier) -> Result<(), SnapshotError> {
        let deleted = self.repo.delete(id).await?;
        if deleted == 0 {
            return Err(SnapshotError::not_found(id));
        }
        Ok(())
    }

    /// Commit an asynchronous clone job and return its preparing control resource.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::NotFound`] when the source snapshot is absent or
    /// expired, [`SnapshotError::Quarantined`] when the source's stored branch
    /// is behind this binary, [`SnapshotError::AlreadyExists`] when the requested clone
    /// identifier is already taken, [`SnapshotError::Validation`] when the
    /// resolved lifetime is not positive, [`SnapshotError::Internal`] when a
    /// generated identifier or the configured default lifetime cannot be
    /// materialised, and the store's own failure when the clone job cannot be
    /// persisted or the tenant quotas cannot be enforced.
    pub async fn clone_snapshot(&self, source_id: &Identifier, input: SnapshotCloneInput) -> Result<Snapshot, SnapshotError> {
        let source = self.get(source_id).await?;
        // A quarantined source cannot be copied column for column into a
        // branch built by this binary, so the job is refused before it exists.
        match self.repo.branch_condition(source_id).await? {
            Some(BranchCondition::Current) => {}
            Some(BranchCondition::Quarantined) => return Err(SnapshotError::quarantined(source_id)),
            None => return Err(SnapshotError::not_found(source_id)),
        }

        let new_id = match input.id {
            Some(id) => id,
            None => build_snapshot_id()?,
        };
        if self.repo.exists(&new_id).await? {
            return Err(SnapshotError::already_exists(&new_id));
        }

        let requested_lifetime = match input.snapshot_lifetime.or(source.snapshot_lifetime) {
            Some(lifetime) => lifetime,
            None => iso8601_lifetime(self.config.default_lifetime)?,
        };
        let now = Utc::now();
        let (lifetime, expires_at) = effective_snapshot_lifetime(now, &requested_lifetime, self.config.maximum_lifetime)?;
        let cloned = Snapshot::builder()
            .id(new_id)
            .snapshot_queries(source.snapshot_queries)
            .snapshot_temporal_queries(source.snapshot_temporal_queries)
            .snapshot_lifetime(Some(lifetime))
            .snapshot_priority(input.snapshot_priority.unwrap_or(source.snapshot_priority))
            .endpoint(input.endpoint.or(source.endpoint))
            .receiver_info(input.receiver_info.or(source.receiver_info))
            .status(SnapshotStatus::Preparing)
            .queries_details(source.queries_details)
            .temporal_queries_details(source.temporal_queries_details)
            .created_at(now)
            .modified_at(now)
            .expires_at(expires_at)
            .last_used_at(Some(now))
            .build();
        let query_context = serde_json::json!([andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL]);
        let mut params = to_insert_params(&cloned, query_context)?;
        params.clone_source_id = Some(source_id);
        self.repo.create(params).await?;
        self.enforce_quotas().await?;

        Ok(cloned)
    }

    /// Evict Snapshots beyond the tenant quotas, logging each victim.
    async fn enforce_quotas(&self) -> Result<(), SnapshotError> {
        let evicted = self.repo.enforce_quotas(self.config.tenant_count_quota, self.config.tenant_byte_quota).await?;
        for id in evicted {
            info!(snapshot_id = %id, "Snapshot evicted by tenant resource quota");
        }
        Ok(())
    }
}

/// A fresh broker-minted Snapshot identifier.
fn build_snapshot_id() -> Result<Identifier, SnapshotError> {
    let value = format!("urn:ngsi-ld:Snapshot:{}", uuid::Uuid::new_v4());
    value
        .parse()
        .map_err(|error| SnapshotError::internal(format!("Generated snapshot id is not a valid URI: {error}")))
}

#[cfg(test)]
mod tests;
