//! One bounded, fleet-serialized PostgreSQL maintenance controller pass.

mod branch_sampling;
mod catalog_sampling;
mod density_sampling;
mod index_probing;
mod probe_notices;
mod probe_selection;
mod visit_branches;

use crate::{
    action,
    autovacuum_health,
    branch::Branch,
    branch_selection,
    capability,
    controller::{probe_notices::ProbeNotices, visit_branches::VisitBranches},
    error::MaintenanceError,
    evidence::inspection_result::InspectionResult,
    expected_deployment::ExpectedDeployment,
    extension_observation,
    fleet_observation,
    fleet_triage,
    freeze_report,
    inspection::{self, InvalidIndex},
    policy::{PolicyProfile, SafetyGates},
    replication_slot,
    rowstore_rebuild,
    state::{self, RegisteredTenant, branch_rows, open_breach, pending_action, tenant_inspection},
    statement_statistics,
    timescale_observation,
};
use andromeda_configuration::schema::database::maintenance::{MaintenanceConfig, MaintenanceMode};
use andromeda_postgresql_context_store::{
    schema_contract::{contract::current_tenant_schema_contract, schema_metadata::verify_schema_contract},
    tenant_database::DatabaseUrl,
};
use andromeda_postgresql_pool::pool_options::configure_pool_options;
use andromeda_postgresql_schema::branch_schema_version::BranchSchemaVersion;
use andromeda_telemetry::maintenance::{MaintenanceDecision, record_decision};
use chrono::Utc;
use sqlx::{PgConnection, PgPool, Postgres, pool::PoolConnection};
use std::{iter, ops::ControlFlow, time::Duration};
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
    /// Cheap inspection found no target requiring a detailed probe.
    Idle,
    /// Detailed index probes completed and none queued or would act.
    Probed,
    /// A target accumulated enough consecutive evidence and was queued.
    Queued,
    /// A branch decided under observe mode reported a mutation automatic
    /// mode would execute there.
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
    /// What the observations compare the cluster against.
    expected: ExpectedDeployment,
    /// The highest branch migration this binary embeds. A Snapshot branch
    /// behind it is quarantined and left out of every visit.
    branch_version: BranchSchemaVersion,
    /// The tenants whose missing `pgstattuple` this process has warned about.
    probe_notices: ProbeNotices,
}

impl MaintenanceController {
    /// Construct a controller over the shared platform registry.
    #[must_use]
    pub fn new(
        platform_pool: PgPool,
        direct_url: DatabaseUrl,
        config: MaintenanceConfig,
        acquire_timeout: Duration,
        expected: ExpectedDeployment,
        branch_version: BranchSchemaVersion,
    ) -> MaintenanceController {
        MaintenanceController {
            platform_pool,
            direct_url,
            config,
            acquire_timeout,
            expected,
            branch_version,
            probe_notices: ProbeNotices::default(),
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
        fleet.report_unvisited(&self.expected.tenant_database_prefix);
        replication_slot::observe_replication_slots(platform).await?;
        autovacuum_health::observe_workers(platform).await?;
        let now = Utc::now();
        let records = tenant_inspection::inspection_records(platform).await?;
        let due = pending_action::tenants_with_due_actions(platform, now).await?;
        let breaches = open_breach::open_breaches(platform).await?;
        let standings = fleet_triage::standings(records, &fleet, &due, &breaches);
        let qualifying_duration = PolicyProfile::from_profile(self.config.profile).qualifying_duration;
        let Some(tenant) = fleet_triage::select_tenant(&standings, qualifying_duration, now) else {
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
        timescale_observation::report_timescale_state(&mut connection, tenant, self.expected.columnstore_conversion, Utc::now()).await?;
        extension_observation::report_extension_updates(&mut connection, tenant).await?;
        statement_statistics::report_statement_statistics(&mut connection, tenant).await?;
        let capability = capability::probe_capability(&mut connection).await?;
        self.probe_notices.report(tenant, capability);
        let profile = PolicyProfile::from_profile(self.config.profile);
        let published = inspection::published_branches(&mut connection).await?;
        branch_rows::prune_vanished_branches(platform, tenant, &published).await?;
        let snapshots = branch_selection::lock_branches(&mut connection, tenant, self.branch_version).await?;
        let branches = iter::once(Branch::Live).chain(snapshots.into_iter().map(Branch::Snapshot)).collect::<Vec<_>>();
        let branches = visit_branches::decide_branches(self.config.mode, &mut connection, tenant, &branches).await?;
        let invalid = inspection::invalid_indexes(&mut connection).await?;
        for branch in branches.observed() {
            action::withdraw_observed(platform, tenant, branch).await?;
        }
        let pending_before_sampling = pending_action::first_branch_action(platform, tenant, branches.acting()).await?;
        let sampling = branch_sampling::sample_branches(platform, &mut connection, tenant, &branches, profile).await?;

        report_invalid_indexes(tenant, &invalid, &branches);
        let branch_artifact = invalid
            .iter()
            .filter(|index| branches.contains(index.branch()))
            .find_map(InvalidIndex::recovery_artifact);
        let artifact = match branch_artifact {
            Some(artifact) => Some(artifact),
            None => rowstore_rebuild::chunk_artifacts(&mut connection).await?.into_iter().next(),
        };
        if let Some(artifact) = artifact {
            let mode = branches.mode(artifact.name.branch());
            return action::recover_artifact(mode, platform, &mut connection, tenant, artifact, profile, cancellation).await;
        }
        if let Some(row) = pending_before_sampling {
            let mode = branches.mode(row.branch());
            return action::execute_pending(mode, platform, &mut connection, tenant, row, profile, cancellation).await;
        }

        let candidates = capability.probe_candidates(sampling.probe_candidates);
        let outcome = index_probing::probe_indexes(&branches, platform, &mut connection, tenant, &candidates, profile, cancellation)
            .await?
            .pass_outcome(sampling.flagged);
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
        let mut connection = tenant_pool.acquire().await?;
        connection.close_on_drop();
        if let Err(error) = verify_schema_contract(&mut connection, &contract).await {
            tenant_inspection::mark_inspected(platform, tenant, InspectionResult::SchemaBehind).await?;
            warn!(tenant = tenant.log_name(), database = %tenant.database, error = %error, decision = "schema-behind", "PostgreSQL maintenance does not migrate idle tenant schemas");
            drop(connection);
            tenant_pool.close().await;
            return Ok(ControlFlow::Break(PassOutcome::SchemaBehind));
        }
        inspection::configure_session(&mut connection).await?;
        Ok(ControlFlow::Continue(connection))
    }
}

/// Log every invalid index the visit leaves alone: one no allowlisted index
/// left behind, and a recovery artifact in a Snapshot branch the visit does
/// not work in, which a visit that does will drop.
fn report_invalid_indexes(tenant: &RegisteredTenant, invalid: &[InvalidIndex], branches: &VisitBranches) {
    for index in invalid {
        let branch = index.branch();
        match index.recovery_artifact() {
            None => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = index.name(), decision = "invalid-index-reported", "Invalid non-maintenance index requires operator investigation");
            }
            Some(artifact) if !branches.contains(branch) => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, target = index.name(), artifact_kind = ?artifact.kind, decision = "branch-artifact-reported", "Concurrent-index recovery artifact in a Snapshot branch this visit does not work in reported");
            }
            Some(_) => {}
        }
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
