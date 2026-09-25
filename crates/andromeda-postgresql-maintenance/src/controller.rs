//! One bounded, fleet-serialized PostgreSQL maintenance controller pass.

use crate::{
    action,
    candidate::{IndexTarget, MaintenanceTarget},
    error::MaintenanceError,
    evidence::inspection_result::InspectionResult,
    inspection::{self, CapabilityStatus, HeapSample, IndexSize, InvalidIndex, MutationPrivilege},
    policy::{DeferralReason, PolicyDecision, PolicyProfile, SafetyGates, decide, safety_deferral},
    state::{
        self,
        RegisteredTenant,
        target_record::{self, IndexProbeState},
        tenant_inspection,
    },
};
use andromeda_postgresql_context_store::{
    maintenance_config::{MaintenanceConfig, MaintenanceMode},
    schema_contract::{contract::current_tenant_schema_contract, schema_metadata::verify_schema_contract},
    tenant_database::DatabaseUrl,
};
use andromeda_postgresql_pool::configure_pool_options;
use andromeda_telemetry::maintenance::{MaintenanceDecision, record_decision};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool};
use std::time::Duration;
use strum::Display;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// Result of one bounded controller pass.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum PassOutcome {
    /// Configuration disables registration and execution.
    Disabled,
    /// Another broker replica owns the fleet lock.
    GlobalLockUnavailable,
    /// No active tenant is registered.
    NoTenant,
    /// The selected database could not be reached.
    TenantUnavailable,
    /// Normal tenant startup has not migrated the database to this binary's contract.
    SchemaBehind,
    /// The selected database lacks an enabled maintenance capability.
    CapabilityUnavailable,
    /// Cheap inspection found no target requiring a detailed probe.
    Idle,
    /// One physical index probe completed.
    Probed,
    /// A target accumulated enough consecutive evidence and was queued.
    Queued,
    /// Observe mode reported a mutation it would otherwise execute.
    WouldAct,
    /// A safety gate deferred detailed inspection or mutation.
    Deferred,
    /// One maintenance or recovery statement succeeded.
    Acted,
    /// A maintenance statement failed and remains retryable.
    ActionFailed,
    /// Cooperative cancellation stopped the pass and released its connections.
    Cancelled,
}

/// Process-scoped adaptive PostgreSQL controller.
pub struct MaintenanceController {
    /// Platform registry and durable-state pool. Reaches PostgreSQL directly,
    /// never through a pooler: a pass holds a session advisory lock and session
    /// `SET`s across a multi-statement sweep on one connection.
    platform_pool: PgPool,
    /// The straight-to-PostgreSQL base URL from which per-tenant database URLs
    /// are derived. Never a pooler, for the same session-state reason.
    direct_url: DatabaseUrl,
    /// Mode and fixed profile selection.
    config: MaintenanceConfig,
    /// How long a per-tenant inspection pool waits for its connection before it
    /// gives up, matching the request path's acquire deadline.
    acquire_timeout: Duration,
}

impl MaintenanceController {
    /// Construct a controller over the shared platform registry.
    #[must_use]
    pub const fn new(platform_pool: PgPool, direct_url: DatabaseUrl, config: MaintenanceConfig, acquire_timeout: Duration) -> MaintenanceController {
        MaintenanceController {
            platform_pool,
            direct_url,
            config,
            acquire_timeout,
        }
    }

    /// Execute one fair, fleet-serialized inspection pass.
    ///
    /// # Errors
    ///
    /// Returns [`MaintenanceError`] when the platform connection cannot be
    /// acquired, when the fleet-wide advisory lock cannot be taken or released,
    /// or when a statement of the selected tenant's inspection fails. A pass
    /// that finds no work, yields to another broker, or declines to act is
    /// reported through [`PassOutcome`], not through an error.
    pub async fn run(&self, cancellation: &CancellationToken) -> Result<PassOutcome, MaintenanceError> {
        let result = self.run_unrecorded(cancellation).await;
        if let Ok(outcome) = result.as_ref() {
            record_decision(metric_decision(*outcome));
        }
        result
    }

    /// Execute one pass before bounded-cardinality metric recording.
    async fn run_unrecorded(&self, cancellation: &CancellationToken) -> Result<PassOutcome, MaintenanceError> {
        if self.config.mode == MaintenanceMode::Disabled {
            return Ok(PassOutcome::Disabled);
        }
        let mut platform = self.platform_pool.acquire().await?;
        platform.close_on_drop();
        if !state::try_global_lock(&mut platform).await? {
            info!(
                decision = "deferred",
                reason = "global-lock-unavailable",
                "PostgreSQL maintenance pass deferred"
            );
            return Ok(PassOutcome::GlobalLockUnavailable);
        }
        let result = self.run_locked(&mut platform, cancellation).await;
        // A failed release outranks the pass result: the advisory lock is
        // fleet-wide state, so reporting an outcome while it may still be held
        // would hide the condition that blocks every other broker.
        state::release_global_lock(&mut platform).await?;
        result
    }

    /// Execute the selected tenant pass while the global session lock is held.
    async fn run_locked(&self, platform: &mut PgConnection, cancellation: &CancellationToken) -> Result<PassOutcome, MaintenanceError> {
        if cancellation.is_cancelled() {
            return Ok(PassOutcome::Cancelled);
        }
        let Some(tenant) = state::select_least_recently_inspected(platform).await? else {
            return Ok(PassOutcome::NoTenant);
        };
        tenant_inspection::mark_inspected(platform, &tenant, InspectionResult::Started).await?;
        let tenant_url = self.direct_url.with_db_name(&tenant.database);
        let pool_result = tokio::select! {
            () = cancellation.cancelled() => return Ok(PassOutcome::Cancelled),
            result = configure_pool_options(1, self.acquire_timeout).connect(tenant_url.as_str()) => result,
        };
        let tenant_pool = match pool_result {
            Ok(pool) => pool,
            Err(error) => {
                tenant_inspection::mark_tenant_failure(platform, &tenant).await?;
                warn!(tenant = tenant.log_name(), database = %tenant.database, error = %error, decision = "tenant-unavailable", "PostgreSQL maintenance pass deferred");
                return Ok(PassOutcome::TenantUnavailable);
            }
        };
        let contract = current_tenant_schema_contract()?;
        if let Err(error) = verify_schema_contract(&tenant_pool, &contract).await {
            tenant_inspection::mark_inspected(platform, &tenant, InspectionResult::SchemaBehind).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, error = %error, decision = "schema-behind", "PostgreSQL maintenance does not migrate idle tenant schemas");
            tenant_pool.close().await;
            return Ok(PassOutcome::SchemaBehind);
        }
        let mut connection = tenant_pool.acquire().await?;
        connection.close_on_drop();
        inspection::configure_session(&mut connection).await?;
        let mutation = match self.config.mode {
            MaintenanceMode::Disabled | MaintenanceMode::Observe => MutationPrivilege::NotRequired,
            MaintenanceMode::Automatic => MutationPrivilege::Required,
        };
        let capability = inspection::validate_capabilities(&mut connection, mutation).await?;
        if capability != CapabilityStatus::Ready {
            tenant_inspection::mark_inspected(platform, &tenant, InspectionResult::of_capability(capability)).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, capability = ?capability, decision = "capability-unavailable", "PostgreSQL maintenance pass deferred");
            return Ok(PassOutcome::CapabilityUnavailable);
        }

        let profile = PolicyProfile::from_profile(self.config.profile);
        let invalid = inspection::invalid_indexes(&mut connection).await?;
        let index_sizes = inspection::index_sizes(&mut connection).await?;
        let heap_samples = inspection::heap_samples(&mut connection, profile).await?;
        let pending_before_sampling = target_record::pending_action(platform, &tenant).await?;
        let probe_states = target_record::index_probe_states(platform, &tenant).await?;
        let sampling_outcome = self.record_heap_samples(platform, &tenant, &heap_samples, profile).await?;

        for index in &invalid {
            if index.recovery_artifact().is_none() {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = index.name(), decision = "invalid-index-reported", "Invalid non-maintenance index requires operator investigation");
            }
        }
        if let Some(artifact) = invalid.iter().find_map(InvalidIndex::recovery_artifact) {
            return action::recover_artifact(self.config.mode, platform, &mut connection, &tenant, artifact, profile, cancellation).await;
        }
        if let Some(target) = pending_before_sampling {
            return action::execute_pending(self.config.mode, platform, &mut connection, &tenant, target, profile, cancellation).await;
        }

        let Some(size) = highest_priority_probe_candidate(&index_sizes, &probe_states, profile, Utc::now()) else {
            let outcome = sampling_outcome.unwrap_or(PassOutcome::Idle);
            tenant_inspection::mark_inspected(platform, &tenant, InspectionResult::of_pass(outcome)).await?;
            return Ok(outcome);
        };
        let target = MaintenanceTarget::Index(size.target);
        let gates = inspection::safety_gates(&mut connection, target).await?;
        if let Some(reason) = safety_deferral(profile, gates) {
            info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), decision = "deferred", reason = ?reason, "PostgreSQL maintenance probe deferred");
            tenant_inspection::mark_inspected(platform, &tenant, InspectionResult::Deferred).await?;
            return Ok(PassOutcome::Deferred);
        }
        let measured = tokio::select! {
            () = cancellation.cancelled() => return Ok(PassOutcome::Cancelled),
            result = inspection::probe_index(&mut connection, size, profile) => result?,
        };
        let target_state = target_record::load_target_state(platform, &tenant, target).await?;
        let decision = decide(self.config.mode, profile, target_state, measured, idle_gates(), Utc::now());
        target_record::record_decision(platform, &tenant, target, Some(measured), decision).await?;
        let outcome = match decision {
            PolicyDecision::Queued => PassOutcome::Queued,
            PolicyDecision::WouldAct | PolicyDecision::Act => PassOutcome::WouldAct,
            PolicyDecision::Deferred(
                DeferralReason::RelationTooSmall
                | DeferralReason::ReclaimTooSmall
                | DeferralReason::BelowThreshold
                | DeferralReason::ConsecutiveSamples
                | DeferralReason::Cooldown
                | DeferralReason::ActiveBackendRatio
                | DeferralReason::LongTransaction
                | DeferralReason::ActiveMaintenance
                | DeferralReason::WaitingRelationLock,
            ) => sampling_outcome.unwrap_or(PassOutcome::Probed),
        };
        info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), relation_bytes = measured.relation_bytes, reclaimable_bytes = measured.reclaimable_bytes, degraded_percent = measured.degraded_percent, decision = ?decision, "PostgreSQL maintenance index probe completed");
        tenant_inspection::mark_inspected(platform, &tenant, InspectionResult::of_pass(outcome)).await?;
        Ok(outcome)
    }

    /// Persist cheap heap evidence without executing newly queued work in this pass.
    async fn record_heap_samples(
        &self,
        platform: &mut PgConnection,
        tenant: &RegisteredTenant,
        samples: &[HeapSample],
        profile: PolicyProfile,
    ) -> Result<Option<PassOutcome>, MaintenanceError> {
        let mut outcome = None;
        for sample in samples {
            let target = MaintenanceTarget::Heap(sample.target);
            let target_state = target_record::load_target_state(platform, tenant, target).await?;
            let decision = decide(self.config.mode, profile, target_state, sample.degradation, idle_gates(), Utc::now());
            target_record::record_decision(platform, tenant, target, Some(sample.degradation), decision).await?;
            info!(tenant = tenant.log_name(), database = %tenant.database, target = target.relation_name(), relation_bytes = sample.degradation.relation_bytes, reclaimable_bytes = sample.degradation.reclaimable_bytes, degraded_percent = sample.degradation.degraded_percent, decision = ?decision, "PostgreSQL maintenance heap sample completed");
            match decision {
                PolicyDecision::Queued => outcome = Some(PassOutcome::Queued),
                PolicyDecision::WouldAct if outcome.is_none() => outcome = Some(PassOutcome::WouldAct),
                PolicyDecision::Act
                | PolicyDecision::WouldAct
                | PolicyDecision::Deferred(
                    DeferralReason::RelationTooSmall
                    | DeferralReason::ReclaimTooSmall
                    | DeferralReason::BelowThreshold
                    | DeferralReason::ConsecutiveSamples
                    | DeferralReason::Cooldown
                    | DeferralReason::ActiveBackendRatio
                    | DeferralReason::LongTransaction
                    | DeferralReason::ActiveMaintenance
                    | DeferralReason::WaitingRelationLock,
                ) => {}
            }
        }
        Ok(outcome)
    }
}

/// Pick the least-recently-probed eligible index, breaking ties by size.
fn highest_priority_probe_candidate(sizes: &[IndexSize], states: &[IndexProbeState], profile: PolicyProfile, now: DateTime<Utc>) -> Option<IndexSize> {
    sizes
        .iter()
        .copied()
        .filter(|size| size.bytes >= profile.minimum_relation_bytes && IndexTarget::PROBEABLE.contains(&size.target))
        .filter(|size| {
            states
                .iter()
                .find(|state| state.target == size.target)
                .and_then(|state| state.cooldown_until)
                .is_none_or(|cooldown_until| cooldown_until <= now)
        })
        .min_by(|left, right| {
            let left_sampled = states.iter().find(|state| state.target == left.target).and_then(|state| state.last_sampled_at);
            let right_sampled = states.iter().find(|state| state.target == right.target).and_then(|state| state.last_sampled_at);
            left_sampled.cmp(&right_sampled).then_with(|| right.bytes.cmp(&left.bytes))
        })
}

/// Safety gates used while merely accumulating cheap evidence.
const fn idle_gates() -> SafetyGates {
    SafetyGates {
        active_backend_percent: 0,
        oldest_transaction_seconds: 0,
        active_maintenance: false,
        waiting_relation_lock: false,
    }
}

/// Map a pass result to the telemetry crate's stable label domain.
const fn metric_decision(outcome: PassOutcome) -> MaintenanceDecision {
    match outcome {
        PassOutcome::Disabled => MaintenanceDecision::Disabled,
        PassOutcome::GlobalLockUnavailable => MaintenanceDecision::GlobalLockUnavailable,
        PassOutcome::NoTenant => MaintenanceDecision::NoTenant,
        PassOutcome::TenantUnavailable => MaintenanceDecision::TenantUnavailable,
        PassOutcome::SchemaBehind => MaintenanceDecision::SchemaBehind,
        PassOutcome::CapabilityUnavailable => MaintenanceDecision::CapabilityUnavailable,
        PassOutcome::Idle => MaintenanceDecision::Idle,
        PassOutcome::Probed => MaintenanceDecision::Probed,
        PassOutcome::Queued => MaintenanceDecision::Queued,
        PassOutcome::WouldAct => MaintenanceDecision::WouldAct,
        PassOutcome::Deferred => MaintenanceDecision::Deferred,
        PassOutcome::Acted => MaintenanceDecision::Acted,
        PassOutcome::ActionFailed => MaintenanceDecision::ActionFailed,
        PassOutcome::Cancelled => MaintenanceDecision::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::IndexTarget,
        controller::highest_priority_probe_candidate,
        inspection::IndexSize,
        policy::PolicyProfile,
        state::target_record::IndexProbeState,
    };
    use andromeda_postgresql_context_store::maintenance_config::MaintenanceProfile;
    use chrono::{Duration, Utc};
    use pretty_assertions::assert_eq;

    #[test]
    fn the_trigram_gin_is_never_selected_for_direct_probe() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let sizes = [
            IndexSize {
                target: IndexTarget::IdxCavAttrStringTrgm,
                bytes: profile.minimum_relation_bytes * 10,
            },
            IndexSize {
                target: IndexTarget::IdxCavAttrString,
                bytes: profile.minimum_relation_bytes,
            },
        ];
        assert_eq!(
            highest_priority_probe_candidate(&sizes, &[], profile, Utc::now()).map(|size| size.target),
            Some(IndexTarget::IdxCavAttrString)
        );
    }

    #[test]
    fn detailed_probes_rotate_and_skip_successful_cooldowns() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let sizes = [
            IndexSize {
                target: IndexTarget::IdxCavAttrNumber,
                bytes: profile.minimum_relation_bytes * 2,
            },
            IndexSize {
                target: IndexTarget::IdxCavAttrString,
                bytes: profile.minimum_relation_bytes,
            },
            IndexSize {
                target: IndexTarget::IdxCavAttrBool,
                bytes: profile.minimum_relation_bytes,
            },
        ];
        let states = [
            IndexProbeState {
                target: IndexTarget::IdxCavAttrNumber,
                last_sampled_at: None,
                cooldown_until: Some(now + Duration::hours(1)),
            },
            IndexProbeState {
                target: IndexTarget::IdxCavAttrString,
                last_sampled_at: Some(now - Duration::minutes(1)),
                cooldown_until: None,
            },
            IndexProbeState {
                target: IndexTarget::IdxCavAttrBool,
                last_sampled_at: Some(now - Duration::minutes(2)),
                cooldown_until: None,
            },
        ];
        assert_eq!(
            highest_priority_probe_candidate(&sizes, &states, profile, now).map(|size| size.target),
            Some(IndexTarget::IdxCavAttrBool)
        );
    }
}
