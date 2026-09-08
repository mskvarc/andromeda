use crate::{
    error::SnapshotError,
    executor::{SnapshotPreparation, SnapshotPreparationOutcome},
    prepare::to_insert_params,
    query_validation::validate_snapshot_query_shapes,
    status::{ScopingEligibility, scoping_eligibility},
    store::SnapshotStore,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    snapshot::{Snapshot, SnapshotStatus},
    snapshot_clone::SnapshotCloneInput,
    snapshot_filter::SnapshotPurgeFilter,
    snapshot_input::SnapshotInput,
    snapshot_patch::{MergePatchValue, SnapshotPatch},
};
use chrono::Utc;
use iso8601_duration::Duration as Iso8601Duration;
use std::{sync::Arc, time::Duration};
use tokio::time::{MissedTickBehavior, interval, timeout};
use tracing::{info, warn};

/// Service for managing NGSI-LD entity snapshots.
pub struct SnapshotService {
    repo: Arc<dyn SnapshotStore>,
    preparation: Option<Arc<SnapshotPreparation>>,
    default_lifetime: Duration,
    maximum_lifetime: Duration,
    scoped_operation_wait: Duration,
    tenant_count_quota: Option<usize>,
    tenant_byte_quota: Option<u64>,
}

impl SnapshotService {
    /// Create a new snapshot service backed by the given store. `preparation`
    /// is the bounded executor [`SnapshotService::prepare_now`] kicks so a
    /// freshly created snapshot does not wait for the next scheduled sweep.
    pub const fn new(
        repo: Arc<dyn SnapshotStore>,
        preparation: Option<Arc<SnapshotPreparation>>,
        default_lifetime: Duration,
        maximum_lifetime: Duration,
        scoped_operation_wait: Duration,
        tenant_count_quota: Option<usize>,
        tenant_byte_quota: Option<u64>,
    ) -> SnapshotService {
        SnapshotService {
            repo,
            preparation,
            default_lifetime,
            maximum_lifetime,
            scoped_operation_wait,
            tenant_count_quota,
            tenant_byte_quota,
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
        tokio::spawn(async move {
            match preparation.execute().await {
                Ok(SnapshotPreparationOutcome::Idle) => {}
                Ok(SnapshotPreparationOutcome::Prepared { count }) => info!(count, "snapshot preparation completed on create"),
                Err(error) => warn!(%error, "snapshot preparation on create failed; the scheduled sweep will retry"),
            }
        });
    }

    /// Create a new snapshot from the given input parameters.
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
            None => iso8601_lifetime(self.default_lifetime)?,
        };
        let (snapshot_lifetime, expires_at) = effective_snapshot_lifetime(now, &requested_lifetime, self.maximum_lifetime)?;

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
    pub async fn get(&self, id: &Uri) -> Result<Snapshot, SnapshotError> {
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
    pub async fn update(&self, id: &Uri, patch: SnapshotPatch) -> Result<Snapshot, SnapshotError> {
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
            .map(|lifetime| effective_snapshot_lifetime(Utc::now(), lifetime, self.maximum_lifetime))
            .transpose()?;
        let persisted_lifetime = effective_lifetime.as_ref().map(|(lifetime, _)| lifetime);
        let new_expires_at = effective_lifetime.as_ref().map(|(_, expires_at)| *expires_at);

        self.repo
            .update_metadata(id, persisted_lifetime, patch_priority, endpoint, receiver_info, new_expires_at)
            .await?;

        self.get(id).await
    }

    /// Delete a snapshot by its identifier.
    pub async fn delete(&self, id: &Uri) -> Result<(), SnapshotError> {
        let deleted = self.repo.delete(id).await?;
        if deleted == 0 {
            return Err(SnapshotError::not_found(id));
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
    pub async fn purge_by_query(&self, filter: &SnapshotPurgeFilter) -> Result<PurgeOutcome, SnapshotError> {
        let matched = self.repo.match_by_filter(filter).await?;
        if matched.is_empty() {
            return Ok(PurgeOutcome::NoMatch);
        }

        let mut successes = Vec::new();
        let mut failures = Vec::new();

        for id in matched {
            match self.repo.delete_one(&id).await {
                Ok(n) if n > 0 => successes.push(id),
                Ok(_) => failures.push(SnapshotPurgeFailure {
                    error: SnapshotError::not_found(&id),
                    identifier: id,
                }),
                Err(error) => failures.push(SnapshotPurgeFailure { identifier: id, error }),
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
    /// The snapshot must exist, not be expired, and have a queryable status:
    /// `Success`, `Partial`, or `Empty` (clause 5.2.40: an empty snapshot is a
    /// prepared one whose queries matched nothing, so it scopes to an empty
    /// dataset rather than to "no snapshot"). Touches `last_used_at` on success.
    pub async fn validate_for_scoping(&self, id: &Uri) -> Result<Snapshot, SnapshotError> {
        let snapshot = self.repo.get(id).await?.ok_or_else(|| SnapshotError::not_found(id))?;

        if snapshot.expires_at < Utc::now() {
            if let Err(e) = self.repo.delete(id).await {
                tracing::error!(error = %e, snapshot_id = %id, "Failed to delete expired snapshot");
            }
            return Err(SnapshotError::not_found(id));
        }

        let snapshot = match scoping_eligibility(snapshot.status) {
            ScopingEligibility::Usable => snapshot,
            ScopingEligibility::Preparing => self.wait_for_terminal(id).await?,
        };

        if let Err(e) = self.repo.touch_last_used(id).await {
            tracing::warn!(error = %e, snapshot_id = %id, "Failed to update snapshot last_used_at");
        }

        Ok(snapshot)
    }

    /// Resolve and validate the typed branch target for a scoped operation.
    pub async fn resolve_context_target(&self, id: &Uri) -> Result<ContextTarget, SnapshotError> {
        self.validate_for_scoping(id).await?;
        let target = self.repo.target(id).await?.ok_or_else(|| SnapshotError::not_found(id))?;
        Ok(ContextTarget::Snapshot(target))
    }

    /// Wait until capture or clone promotion leaves `preparing` without treating status as absence.
    async fn wait_for_terminal(&self, id: &Uri) -> Result<Snapshot, SnapshotError> {
        let wait = async {
            let mut poll = interval(Duration::from_millis(25));
            poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
            loop {
                poll.tick().await;
                let snapshot = self.repo.get(id).await?.ok_or_else(|| SnapshotError::not_found(id))?;
                if snapshot.expires_at < Utc::now() {
                    return Err(SnapshotError::not_found(id));
                }
                match scoping_eligibility(snapshot.status) {
                    ScopingEligibility::Usable => return Ok(snapshot),
                    ScopingEligibility::Preparing => {}
                }
            }
        };
        timeout(self.scoped_operation_wait, wait)
            .await
            .map_err(|_| SnapshotError::preparation_timeout(id))?
    }

    /// Commit an asynchronous clone job and return its preparing control resource.
    pub async fn clone_snapshot(&self, source_id: &Uri, input: SnapshotCloneInput) -> Result<Snapshot, SnapshotError> {
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
            None => iso8601_lifetime(self.default_lifetime)?,
        };
        let now = Utc::now();
        let (lifetime, expires_at) = effective_snapshot_lifetime(now, &requested_lifetime, self.maximum_lifetime)?;
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

    async fn enforce_quotas(&self) -> Result<(), SnapshotError> {
        let evicted = self.repo.enforce_quotas(self.tenant_count_quota, self.tenant_byte_quota).await?;
        for id in evicted {
            info!(snapshot_id = %id, "Snapshot evicted by tenant resource quota");
        }
        Ok(())
    }
}

/// Outcome of [`SnapshotService::purge_by_query`] (ETSI GS CIM 009 clause 6.36.3.2).
#[derive(Debug)]
pub enum PurgeOutcome {
    /// No snapshots matched the filter (maps to 404 `ResourceNotFound`).
    NoMatch,
    /// Every matched snapshot was deleted (maps to 204 No Content).
    FullSuccess,
    /// All matched snapshots failed to delete (still 207 with only errors).
    AllFailed {
        /// Per-snapshot error entries for the multi-status body.
        failures: Vec<SnapshotPurgeFailure>,
    },
    /// Some succeeded and some failed (maps to 207 Multi-Status).
    Partial {
        /// Identifiers of successfully deleted snapshots.
        successes: Vec<Uri>,
        /// Per-snapshot error entries for the multi-status body.
        failures: Vec<SnapshotPurgeFailure>,
    },
}

/// One policy-owned failure produced while purging multiple snapshots.
#[derive(Debug)]
pub struct SnapshotPurgeFailure {
    /// Snapshot whose deletion failed.
    pub identifier: Uri,
    /// Focused reason for the failure.
    pub error: SnapshotError,
}

fn build_snapshot_id() -> Result<Uri, SnapshotError> {
    let value = format!("urn:ngsi-ld:Snapshot:{}", uuid::Uuid::new_v4());
    value
        .parse()
        .map_err(|error| SnapshotError::internal(format!("Generated snapshot id is not a valid URI: {error}")))
}

fn effective_snapshot_lifetime(
    starts_at: chrono::DateTime<Utc>,
    lifetime: &Iso8601Duration,
    maximum_lifetime: Duration,
) -> Result<(Iso8601Duration, chrono::DateTime<Utc>), SnapshotError> {
    let expires_at = starts_at + *lifetime;
    let maximum_expires_at = starts_at
        + chrono::Duration::from_std(maximum_lifetime)
            .map_err(|error| SnapshotError::internal(format!("Configured maximum Snapshot lifetime is invalid: {error}")))?;
    if expires_at <= starts_at {
        Err(SnapshotError::validation("snapshotLifetime must be positive"))
    } else if expires_at > maximum_expires_at {
        Ok((iso8601_lifetime(maximum_lifetime)?, maximum_expires_at))
    } else {
        Ok((*lifetime, expires_at))
    }
}

fn iso8601_lifetime(duration: Duration) -> Result<Iso8601Duration, SnapshotError> {
    let total_seconds = duration.as_secs();
    let days = total_seconds / 86_400;
    let after_days = total_seconds % 86_400;
    let hours = after_days / 3_600;
    let after_hours = after_days % 3_600;
    let minutes = after_hours / 60;
    let seconds = after_hours % 60;
    let value = if days > 0 {
        format!("P{days}DT{hours}H{minutes}M{seconds}S")
    } else {
        format!("PT{hours}H{minutes}M{seconds}S")
    };
    value
        .parse()
        .map_err(|error| SnapshotError::internal(format!("Configured default Snapshot lifetime is invalid: {error:?}")))
}
