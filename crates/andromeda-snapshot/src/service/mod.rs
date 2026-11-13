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
    error::SnapshotError,
    preparation_start::PreparationStart,
    prepare::to_insert_params,
    query_validation::validate_snapshot_query_shapes,
    receiver_info_admission::validate_receiver_info,
    service::lifetime::effective_snapshot_lifetime,
    snapshot_invariant::SnapshotInvariant,
    snapshot_validation_reason::SnapshotValidationReason,
    status_delivery_signal::StatusDeliverySignal,
    store::SnapshotStore,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_spec::ContextSpec, user_context_admission::AdmittedContext};
use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, identifier::Identifier, ngsi_ld_urn::SnapshotId, time::system_instant::SystemInstant};
use andromeda_ngsi_ld_model::{
    clearable::Clearable,
    snapshot::{Snapshot, SnapshotStatus},
    snapshot_clone::SnapshotCloneInput,
    snapshot_input::SnapshotInput,
    snapshot_patch::SnapshotPatch,
};
use chrono::{DateTime, Utc};
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
    /// Starts a created or cloned Snapshot's preparation ahead of the
    /// scheduled sweep.
    preparation: Option<Arc<dyn PreparationStart>>,
    /// Wakes the durable status-delivery drain after a metadata update.
    delivery_signal: Option<Arc<dyn StatusDeliverySignal>>,
    /// Lifetime, wait and quota policy.
    config: SnapshotServiceConfig,
}

impl SnapshotService {
    /// Create a new snapshot service backed by the given store. `preparation`
    /// is what [`SnapshotService::prepare_created`] starts a freshly created
    /// snapshot's preparation on, so it does not wait for the next scheduled
    /// sweep. `delivery_signal` wakes the durable status-delivery drain after a
    /// metadata update enqueues notifications, so they are not left to the
    /// drain's idle fallback interval.
    pub const fn new(
        repo: Arc<dyn SnapshotStore>,
        preparation: Option<Arc<dyn PreparationStart>>,
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

    /// Start the preparation of the Snapshot `id` just created or cloned. The
    /// scheduled sweep remains the guarantee that every `preparing` snapshot
    /// is executed; this only removes the sweep cadence as the floor on how
    /// soon a new snapshot becomes usable (clause 5.16.1.4 executes the queries
    /// on creation). A pass racing the sweep is harmless: only one worker
    /// holds a preparation's claim at a time.
    pub fn prepare_created(&self, id: &Identifier) {
        if let Some(preparation) = &self.preparation {
            preparation.start(id);
        }
    }

    /// Create a new snapshot from the given input parameters.
    ///
    /// # Errors
    ///
    /// Returns [`SnapshotError::Validation`] when both query collections are
    /// absent, when a query occupies the wrong collection, when an `http`
    /// endpoint carries a `receiverInfo` entry that is not an HTTP header field,
    /// or when the requested lifetime is not positive; [`SnapshotError::AlreadyExists`] when the
    /// requested identifier is already taken; [`SnapshotError::Internal`] when a
    /// generated identifier, the configured default lifetime or the request
    /// context cannot be materialised; and the store's own failure when the
    /// record cannot be persisted.
    pub async fn create(&self, input: SnapshotInput, context: AdmittedContext) -> Result<Snapshot, SnapshotError> {
        if input.snapshot_queries.is_none() && input.snapshot_temporal_queries.is_none() {
            return Err(SnapshotValidationReason::QueryCollectionsMissing.into());
        }
        // Reject a malformed query shape before persistence (clause 5.16.1), so
        // an unparseable `q`, geometry or `timerel` is a 400 rather than a
        // snapshot created 201 that only fails when its queries execute.
        validate_snapshot_query_shapes(input.snapshot_queries.as_deref(), input.snapshot_temporal_queries.as_deref())?;
        validate_receiver_info(input.endpoint.as_ref(), input.receiver_info.as_deref())?;

        let id = match input.id {
            Some(snapshot_id) => snapshot_id,
            None => build_snapshot_id()?,
        };

        if self.repo.exists(&id).await? {
            return Err(SnapshotError::already_exists(&id));
        }

        let priority = input.snapshot_priority.unwrap_or_default();

        let now = system_now();
        let requested_lifetime = match input.snapshot_lifetime {
            Some(lifetime) => lifetime,
            None => Iso8601Duration::from_std(self.config.default_lifetime),
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

        let query_context = JsonLiteral::admit(context.resolved_context().text()).map_err(SnapshotInvariant::QueryContextText)?;
        self.repo.create(to_insert_params(&snapshot, query_context)).await?;
        self.enforce_quotas(&snapshot.id).await;

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
    /// identifier or either query collection, supplies a non-positive
    /// lifetime or leaves an `http` endpoint with a `receiverInfo` entry that is
    /// not an HTTP header field, and the store's own failure when the metadata write does not
    /// complete.
    pub async fn update(&self, id: &Identifier, patch: SnapshotPatch) -> Result<Snapshot, SnapshotError> {
        let stored = self.get(id).await?;

        if patch.id.as_ref().is_some_and(|patch_id| patch_id != id) {
            return Err(SnapshotValidationReason::IdImmutable.into());
        }
        if !patch.snapshot_queries.is_absent() || !patch.snapshot_temporal_queries.is_absent() {
            return Err(SnapshotValidationReason::QueryDefinitionsImmutable.into());
        }

        let merged_endpoint = match &patch.endpoint {
            Clearable::Absent => stored.endpoint.as_ref(),
            Clearable::NgsiLdNull => None,
            Clearable::Set(endpoint) => Some(endpoint),
        };
        let merged_receiver_info = match &patch.receiver_info {
            Clearable::Absent => stored.receiver_info.as_deref(),
            Clearable::NgsiLdNull => None,
            Clearable::Set(receiver_info) => Some(receiver_info.as_slice()),
        };
        validate_receiver_info(merged_endpoint, merged_receiver_info)?;

        let patch_priority = patch.snapshot_priority;
        let endpoint = match &patch.endpoint {
            Clearable::Absent => None,
            Clearable::NgsiLdNull => Some(None),
            Clearable::Set(endpoint) => Some(Some(endpoint)),
        };
        let receiver_info = match patch.receiver_info {
            Clearable::Absent => None,
            Clearable::NgsiLdNull => Some(None),
            Clearable::Set(receiver_info) => Some(Some(receiver_info)),
        };

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
    /// expired, [`SnapshotError::AlreadyExists`] when the requested clone
    /// identifier is already taken, [`SnapshotError::Validation`] when the
    /// resolved lifetime is not positive, [`SnapshotError::Internal`] when a
    /// generated identifier or the configured default lifetime cannot be
    /// materialised, and the store's own failure when the clone job cannot be
    /// persisted.
    pub async fn clone_snapshot(&self, source_id: &Identifier, input: SnapshotCloneInput) -> Result<Snapshot, SnapshotError> {
        let source = self.get(source_id).await?;

        let new_id = match input.id {
            Some(id) => id,
            None => build_snapshot_id()?,
        };
        if self.repo.exists(&new_id).await? {
            return Err(SnapshotError::already_exists(&new_id));
        }

        let requested_lifetime = match input.snapshot_lifetime.or(source.snapshot_lifetime) {
            Some(lifetime) => lifetime,
            None => Iso8601Duration::from_std(self.config.default_lifetime),
        };
        let now = system_now();
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
        let query_context = JsonLiteral::admit(ContextSpec::core_only().resolved_context().text()).map_err(SnapshotInvariant::QueryContextText)?;
        let mut params = to_insert_params(&cloned, query_context);
        params.clone_source_id = Some(source_id);
        self.repo.create(params).await?;
        self.enforce_quotas(&cloned.id).await;

        Ok(cloned)
    }

    /// Evict Snapshots beyond the tenant quotas, logging each victim.
    ///
    /// The Snapshot `created` is committed before the check runs, so a check
    /// that fails is logged rather than answered: the create or clone itself
    /// succeeded, and the next create or clone checks again.
    async fn enforce_quotas(&self, created: &Identifier) {
        match self.repo.enforce_quotas(self.config.tenant_count_quota, self.config.tenant_byte_quota).await {
            Ok(evicted) => {
                for id in evicted {
                    info!(snapshot_id = %id, "Snapshot evicted by tenant resource quota");
                }
            }
            Err(error) => warn!(snapshot_id = %created, %error, "Snapshot quota enforcement after a create or clone failed"),
        }
    }
}

/// The instant a Snapshot is stamped with, at the microsecond the stored
/// record keeps, so the Snapshot a create or clone answers with carries the
/// same `createdAt` and `modifiedAt` a later read returns.
fn system_now() -> DateTime<Utc> {
    SystemInstant::new(Utc::now()).get()
}

/// A fresh broker-minted Snapshot identifier.
fn build_snapshot_id() -> Result<Identifier, SnapshotError> {
    SnapshotId::mint()
        .map(Identifier::from)
        .map_err(|error| SnapshotInvariant::MintedIdentifier(error).into())
}

#[cfg(test)]
mod tests;
