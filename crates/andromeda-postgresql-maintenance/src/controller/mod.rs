//! One bounded, fleet-serialized PostgreSQL maintenance controller pass.

mod heap_sampling;
mod index_probing;
mod probe_selection;

use crate::{
    action,
    btree_estimate,
    capability::{self, CapabilityStatus, MutationPrivilege},
    error::MaintenanceError,
    evidence::inspection_result::InspectionResult,
    fleet_observation,
    fleet_triage,
    freeze_report,
    inspection::{self, InvalidIndex},
    policy::{PolicyProfile, SafetyGates},
    state::{self, RegisteredTenant, target_record, tenant_inspection},
};
use andromeda_postgresql_context_store::{
    maintenance_config::{MaintenanceConfig, MaintenanceMode},
    schema_contract::{contract::current_tenant_schema_contract, schema_metadata::verify_schema_contract},
    tenant_database::DatabaseUrl,
};
use andromeda_postgresql_pool::pool_options::configure_pool_options;
use andromeda_telemetry::maintenance::{MaintenanceDecision, record_decision};
use chrono::Utc;
use sqlx::{PgConnection, PgPool, Postgres, pool::PoolConnection};
use std::{ops::ControlFlow, time::Duration};
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
    /// No active tenant is registered, or none is due a visit.
    NoTenant,
    /// The selected database could not be reached.
    TenantUnavailable,
    /// Normal tenant startup has not migrated the database to this binary's contract.
    SchemaBehind,
    /// The selected database lacks an enabled maintenance capability.
    CapabilityUnavailable,
    /// Cheap inspection found no target requiring a detailed probe.
    Idle,
    /// Detailed index probes completed and none queued or would act.
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
        let fleet = fleet_observation::observe_fleet(platform).await?;
        fleet.record_telemetry();
        let now = Utc::now();
        let records = tenant_inspection::inspection_records(platform).await?;
        let due = target_record::tenants_with_due_actions(platform, now).await?;
        let standings = fleet_triage::standings(records, &fleet, &due);
        let Some(tenant) = fleet_triage::select_tenant(&standings, now) else {
            return Ok(PassOutcome::NoTenant);
        };
        let observed = fleet.database(tenant.key);
        tenant_inspection::mark_started(platform, tenant, observed.map(|database| database.changes)).await?;
        let mut connection = match self.open_tenant_session(platform, tenant, cancellation).await? {
            ControlFlow::Continue(connection) => connection,
            ControlFlow::Break(outcome) => return Ok(outcome),
        };
        if let Some(database) = observed {
            freeze_report::report_freeze_pressure(&mut connection, tenant, database.freeze).await?;
        }
        let mutation = match self.config.mode {
            MaintenanceMode::Disabled | MaintenanceMode::Observe => MutationPrivilege::NotRequired,
            MaintenanceMode::Automatic => MutationPrivilege::Required,
        };
        let capability = capability::validate_capabilities(&mut connection, mutation).await?;
        if capability != CapabilityStatus::Ready {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::of_capability(capability)).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, capability = ?capability, decision = "capability-unavailable", "PostgreSQL maintenance pass deferred");
            return Ok(PassOutcome::CapabilityUnavailable);
        }

        let profile = PolicyProfile::from_profile(self.config.profile);
        let invalid = inspection::invalid_indexes(&mut connection).await?;
        let index_sizes = inspection::index_sizes(&mut connection).await?;
        let estimates = btree_estimate::btree_estimates(&mut connection).await?;
        let heap_samples = inspection::heap_samples(&mut connection, profile).await?;
        let pending_before_sampling = target_record::pending_action(platform, tenant).await?;
        let probe_states = target_record::index_probe_states(platform, tenant).await?;
        let sampling_outcome = heap_sampling::record_heap_samples(self.config.mode, platform, tenant, &heap_samples, profile).await?;

        for index in &invalid {
            if index.recovery_artifact().is_none() {
                warn!(tenant = tenant.log_name(), database = %tenant.database, target = index.name(), decision = "invalid-index-reported", "Invalid non-maintenance index requires operator investigation");
            }
        }
        if let Some(artifact) = invalid.iter().find_map(InvalidIndex::recovery_artifact) {
            return action::recover_artifact(self.config.mode, platform, &mut connection, tenant, artifact, profile, cancellation).await;
        }
        if let Some(target) = pending_before_sampling {
            return action::execute_pending(self.config.mode, platform, &mut connection, tenant, target, profile, cancellation).await;
        }

        let candidates = probe_selection::probe_order(&index_sizes, &probe_states, &estimates, profile, Utc::now());
        let outcome = index_probing::probe_indexes(self.config.mode, platform, &mut connection, tenant, &candidates, profile, cancellation)
            .await?
            .pass_outcome(sampling_outcome);
        if outcome != PassOutcome::Cancelled {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::of_pass(outcome)).await?;
        }
        Ok(outcome)
    }

    /// Connect to the selected tenant's database and configure one maintenance
    /// session, or break with the outcome that ends the pass: cancellation, an
    /// unreachable database, or a schema behind this binary's contract.
    async fn open_tenant_session(
        &self,
        platform: &mut PgConnection,
        tenant: &RegisteredTenant,
        cancellation: &CancellationToken,
    ) -> Result<ControlFlow<PassOutcome, PoolConnection<Postgres>>, MaintenanceError> {
        let tenant_url = self.direct_url.with_database_name(&tenant.database);
        let pool_result = tokio::select! {
            () = cancellation.cancelled() => return Ok(ControlFlow::Break(PassOutcome::Cancelled)),
            result = configure_pool_options(1, self.acquire_timeout).connect(tenant_url.as_str()) => result,
        };
        let tenant_pool = match pool_result {
            Ok(pool) => pool,
            Err(error) => {
                tenant_inspection::mark_tenant_failure(platform, tenant).await?;
                warn!(tenant = tenant.log_name(), database = %tenant.database, error = %error, decision = "tenant-unavailable", "PostgreSQL maintenance pass deferred");
                return Ok(ControlFlow::Break(PassOutcome::TenantUnavailable));
            }
        };
        let contract = current_tenant_schema_contract()?;
        if let Err(error) = verify_schema_contract(&tenant_pool, &contract).await {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::SchemaBehind).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, error = %error, decision = "schema-behind", "PostgreSQL maintenance does not migrate idle tenant schemas");
            tenant_pool.close().await;
            return Ok(ControlFlow::Break(PassOutcome::SchemaBehind));
        }
        let mut connection = tenant_pool.acquire().await?;
        connection.close_on_drop();
        inspection::configure_session(&mut connection).await?;
        Ok(ControlFlow::Continue(connection))
    }
}

/// Safety gates used while merely accumulating cheap evidence.
const fn idle_gates() -> SafetyGates {
    SafetyGates {
        active_backend_percent: 0,
        oldest_transaction_seconds: 0,
        held_horizon: None,
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
