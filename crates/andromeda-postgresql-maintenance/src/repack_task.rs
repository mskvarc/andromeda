//! The task that runs the repacks the passes queue, one at a time across the
//! fleet.
//!
//! A repack runs for minutes or hours. Run inside a pass, it would hold the
//! pass's fleet lock all that time and keep every other tenant from its
//! vacuums, rebuilds and freeze reports. A pass therefore only decides and
//! queues a repack on its target row, and this task, on any replica, takes a
//! fleet lock of its own and runs one queued repack, while the passes go on
//! visiting every tenant. Before it runs, the repack is judged again on a
//! fresh sample, against its gates measured then. The task goes through the
//! repacks due in order and runs the first that gets past that: a repack
//! held back where it is, by a gate, an unreachable database or a table
//! another run holds, stays queued without holding back the others.

use crate::{
    action,
    action_measurement::ActionComparison,
    action_policy::{ActionEnding, action_verdict, cooldown_after},
    branch::{Branch, BranchTarget},
    branch_selection,
    candidate::MaintenanceTarget,
    capability,
    error::MaintenanceError,
    evidence::action_result::ActionResult,
    fleet_observation,
    freeze_policy::FreezePressure,
    heap_free_space,
    policy::{DeferralReason, PolicyDecision, PolicyProfile},
    repack::{self, RepackAbandonment, RepackOutcome, RepackSubject, RunSessions},
    repack_context::{self, RepackInputs},
    repack_policy::{RepackSample, decide_repack, repack_mode},
    replication_slot,
    safety_gate_inspection::{self, GateScope},
    state::{
        self,
        FleetLock,
        FleetLockTaken,
        RegisteredTenant,
        pending_action::{self, QueuedRepack},
        target_record,
        tenant_inspection,
    },
};
use andromeda_configuration::schema::database::maintenance::{MaintenanceConfig, MaintenanceMode};
use andromeda_postgresql_context_store::schema_contract::{contract::current_tenant_schema_contract, schema_metadata::verify_schema_contract};
use andromeda_postgresql_pool::{database_url::DatabaseUrl, pool_options::configure_pool_options};
use andromeda_retry::retry_number::RetryNumber;
use andromeda_telemetry::maintenance::{MaintenanceAction, MaintenanceActionOutcome, record_action};
use chrono::Utc;
use sqlx::{PgConnection, PgPool};
use std::{
    num::NonZeroU32,
    time::{Duration, Instant},
};
use strum::{Display, EnumIter};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// How many sessions a run opens to its tenant database: the main session
/// and the guard.
pub const RUN_SESSIONS: u32 = 2;

/// What one execution of the task came to.
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub enum RepackTaskOutcome {
    /// The configuration runs no repack.
    Observing,
    /// Another broker's task holds the repack fleet lock.
    NotClaimed,
    /// No repack is queued and due.
    NothingQueued,
    /// The repack's tenant is no longer registered.
    Unregistered,
    /// The tenant database could not be reached.
    TenantUnavailable,
    /// The tenant database is behind the binary's schema contract.
    SchemaBehind,
    /// The repack no longer qualifies and was withdrawn.
    Lapsed,
    /// A gate held the repack back; it stays queued.
    Deferred,
    /// The extension or the role cannot run the repack; it was withdrawn.
    Withheld,
    /// The table was repacked.
    Repacked,
    /// The run gave up and cleaned up; the repack backs off.
    Abandoned,
    /// A statement of the run failed; the repack backs off.
    Failed,
    /// Cooperative cancellation stopped the run.
    Cancelled,
}

/// What an execution does after one queued repack came to an outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NextStep {
    /// The repack was held back before it ran; the next one due gets its
    /// turn.
    NextRepack,
    /// The execution is over.
    Done,
}

impl RepackTaskOutcome {
    /// What the execution does after a queued repack came to this outcome.
    pub(crate) const fn next_step(self) -> NextStep {
        match self {
            RepackTaskOutcome::NothingQueued
            | RepackTaskOutcome::Unregistered
            | RepackTaskOutcome::TenantUnavailable
            | RepackTaskOutcome::SchemaBehind
            | RepackTaskOutcome::Lapsed
            | RepackTaskOutcome::Deferred
            | RepackTaskOutcome::Withheld => NextStep::NextRepack,
            RepackTaskOutcome::Observing
            | RepackTaskOutcome::NotClaimed
            | RepackTaskOutcome::Repacked
            | RepackTaskOutcome::Abandoned
            | RepackTaskOutcome::Failed
            | RepackTaskOutcome::Cancelled => NextStep::Done,
        }
    }
}

/// The process-scoped repack task.
pub struct RepackTask {
    /// Platform registry and durable-state pool, reaching PostgreSQL
    /// directly: the task holds a session advisory lock for the whole run.
    platform_pool: PgPool,
    /// The straight-to-PostgreSQL base URL tenant database URLs derive from.
    direct_url: DatabaseUrl,
    /// The controller's configuration, the repack policy and size among it.
    config: MaintenanceConfig,
    /// How long a tenant session waits for its connection.
    acquire_timeout: Duration,
}

impl RepackTask {
    /// A task over the shared platform registry.
    #[must_use]
    pub const fn new(platform_pool: PgPool, direct_url: DatabaseUrl, config: MaintenanceConfig, acquire_timeout: Duration) -> RepackTask {
        RepackTask {
            platform_pool,
            direct_url,
            config,
            acquire_timeout,
        }
    }

    /// Take the repack fleet lock and run the first queued repack due that
    /// gets past its preparation.
    ///
    /// # Errors
    ///
    /// Returns [`MaintenanceError`] when the platform connection cannot be
    /// acquired, the fleet lock cannot be taken or released, or a platform
    /// statement fails. A repack that is deferred, abandoned or fails inside
    /// the tenant database is reported through [`RepackTaskOutcome`].
    pub async fn run(&self, cancellation: &CancellationToken) -> Result<RepackTaskOutcome, MaintenanceError> {
        if repack_mode(self.config.mode, self.config.repack) != MaintenanceMode::Automatic {
            return Ok(RepackTaskOutcome::Observing);
        }
        let mut platform = self.platform_pool.acquire().await?;
        platform.close_on_drop();
        if state::try_fleet_lock(&mut platform, FleetLock::Repack).await? == FleetLockTaken::HeldElsewhere {
            return Ok(RepackTaskOutcome::NotClaimed);
        }
        let outcome = self.run_first_due(&mut platform, cancellation).await;
        state::release_fleet_lock(&mut platform, FleetLock::Repack).await?;
        outcome
    }

    /// Go through the queued repacks due, in order, until one runs.
    async fn run_first_due(&self, platform: &mut PgConnection, cancellation: &CancellationToken) -> Result<RepackTaskOutcome, MaintenanceError> {
        let mut outcome = RepackTaskOutcome::NothingQueued;
        for queued in pending_action::due_repacks(&mut *platform, Utc::now()).await? {
            if cancellation.is_cancelled() {
                return Ok(RepackTaskOutcome::Cancelled);
            }
            outcome = self.run_claimed(&mut *platform, queued, cancellation).await?;
            match outcome.next_step() {
                NextStep::NextRepack => {}
                NextStep::Done => break,
            }
        }
        Ok(outcome)
    }

    /// Run the claimed repack against its tenant database.
    async fn run_claimed(
        &self,
        platform: &mut PgConnection,
        queued: QueuedRepack,
        cancellation: &CancellationToken,
    ) -> Result<RepackTaskOutcome, MaintenanceError> {
        let records = tenant_inspection::inspection_records(&mut *platform).await?;
        let Some(tenant) = records.into_iter().map(|record| record.tenant).find(|tenant| tenant.key == queued.tenant) else {
            return Ok(RepackTaskOutcome::Unregistered);
        };
        let row = BranchTarget::new(queued.branch, MaintenanceTarget::Repack(queued.heap))?;
        let url = self.direct_url.with_database_name(&tenant.database);
        let pool = match configure_pool_options(RUN_SESSIONS, self.acquire_timeout).connect(url.as_str()).await {
            Ok(pool) => pool,
            Err(error) => {
                warn!(tenant = tenant.log_name(), database = %tenant.database, error = %error, decision = "tenant-unavailable", "PostgreSQL repack deferred");
                return Ok(RepackTaskOutcome::TenantUnavailable);
            }
        };
        let outcome = self.run_in_tenant(platform, &pool, &tenant, row, queued, cancellation).await;
        pool.close().await;
        outcome
    }

    /// Prepare, judge again and run the repack of `row` in the tenant's
    /// database.
    async fn run_in_tenant(
        &self,
        platform: &mut PgConnection,
        pool: &PgPool,
        tenant: &RegisteredTenant,
        row: BranchTarget,
        queued: QueuedRepack,
        cancellation: &CancellationToken,
    ) -> Result<RepackTaskOutcome, MaintenanceError> {
        let mut sessions = repack::open_sessions(pool).await?;
        if verify_schema_contract(sessions.main(), &current_tenant_schema_contract()?).await.is_err() {
            return Ok(RepackTaskOutcome::SchemaBehind);
        }
        if let Branch::Snapshot(key) = queued.branch
            && !branch_selection::lock_branch(sessions.main(), tenant, key).await?
        {
            return Ok(RepackTaskOutcome::Deferred);
        }
        let profile = PolicyProfile::from_profile(self.config.profile);
        let Some(reading) = heap_free_space::heap_free_space(sessions.main(), queued.branch, queued.heap).await? else {
            action::withdraw_lapsed(platform, tenant, row, DeferralReason::RelationTooSmall).await?;
            return Ok(RepackTaskOutcome::Lapsed);
        };
        let inputs = self.inputs(platform, sessions.main(), tenant).await?;
        let sample = RepackSample::of_reading(reading, profile);
        if let Err(reason) = sample.qualification(profile, inputs.max_relation_bytes()) {
            action::withdraw_lapsed(platform, tenant, row, reason).await?;
            return Ok(RepackTaskOutcome::Lapsed);
        }
        if repack_context::repack_decision_mode(sessions.main(), row, MaintenanceMode::Automatic, inputs).await? != MaintenanceMode::Automatic {
            let reason = repack_context::withheld_reason(inputs.capability);
            warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = queued.heap.relation_name(), decision = "capability-unavailable", reason, "PostgreSQL repack withdrawn: the extension or the role cannot run it");
            pending_action::withdraw_action(platform, tenant, row).await?;
            return Ok(RepackTaskOutcome::Withheld);
        }
        let now = Utc::now();
        let context = repack_context::repack_context(sessions.main(), row, reading.relid, inputs, now).await?;
        let gates = safety_gate_inspection::safety_gates(sessions.main(), GateScope::Target(row)).await?;
        let state = target_record::load_target_state(platform, tenant, row).await?;
        let decision = decide_repack(MaintenanceMode::Automatic, profile, state, sample, context, gates, now);
        target_record::record_decision(platform, tenant, row, Some(sample.degradation), decision, state.breach, now).await?;
        match decision {
            PolicyDecision::Act => {}
            PolicyDecision::Queued | PolicyDecision::WouldAct | PolicyDecision::Deferred(_) => {
                info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = queued.heap.relation_name(), decision = ?decision, "PostgreSQL repack deferred");
                return Ok(RepackTaskOutcome::Deferred);
            }
        }
        let subject = RepackSubject {
            tenant,
            row,
            heap: queued.heap,
            relid: reading.relid,
        };
        self.run_repack(platform, sessions, subject, profile, state.consecutive_failures, cancellation)
            .await
    }

    /// The inputs a repack in the tenant's database is decided with.
    async fn inputs(&self, platform: &mut PgConnection, connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<RepackInputs, MaintenanceError> {
        let fleet = fleet_observation::observe_fleet(&mut *platform).await?;
        Ok(RepackInputs {
            policy: self.config.repack,
            max_size: self.config.max_repack_size,
            capability: capability::repack_capability(connection).await?,
            retained_wal_bytes: replication_slot::retained_wal(platform).await?,
            freeze: fleet.database(tenant.key).map_or(FreezePressure::Normal, |database| database.freeze.pressure()),
        })
    }

    /// Run the repack and record how it ended, a failure backing off from
    /// `consecutive_failures` earlier ones.
    async fn run_repack(
        &self,
        platform: &mut PgConnection,
        sessions: RunSessions,
        subject: RepackSubject<'_>,
        profile: PolicyProfile,
        consecutive_failures: u32,
        cancellation: &CancellationToken,
    ) -> Result<RepackTaskOutcome, MaintenanceError> {
        let RepackSubject { tenant, row, heap, .. } = subject;
        let started = Instant::now();
        let outcome = repack::run(platform, sessions, subject, cancellation).await?;
        let duration = started.elapsed();
        let failed = |result: ActionResult| {
            let consecutive_failures = RetryNumber::new(NonZeroU32::MIN.saturating_add(consecutive_failures));
            (
                result,
                cooldown_after(profile, row.target(), ActionEnding::Failed { consecutive_failures }, Utc::now()),
            )
        };
        match outcome {
            RepackOutcome::Repacked { before_bytes, after_bytes } => {
                let comparison = ActionComparison::Repack { before_bytes, after_bytes };
                record_action(MaintenanceAction::Repack, MaintenanceActionOutcome::Ok, duration, comparison.effect());
                let verdict = action_verdict(profile, row.target(), comparison);
                let cooldown_until = cooldown_after(profile, row.target(), ActionEnding::Completed(verdict), Utc::now());
                target_record::record_action_completion(platform, tenant, row, verdict, cooldown_until, comparison).await?;
                info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = heap.relation_name(), before_bytes, after_bytes, reclaimed_bytes = before_bytes.saturating_sub(after_bytes), verdict = ?verdict, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "acted", "PostgreSQL repack completed");
                Ok(RepackTaskOutcome::Repacked)
            }
            RepackOutcome::Abandoned { reason, cleanup } => {
                record_action(MaintenanceAction::Repack, MaintenanceActionOutcome::Error, duration, None);
                let Some(result) = abandonment_result(reason) else {
                    info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = heap.relation_name(), reason = <&'static str>::from(reason), decision = "deferred", "PostgreSQL repack deferred");
                    return Ok(RepackTaskOutcome::Deferred);
                };
                let (result, cooldown_until) = failed(result);
                target_record::record_action_failure(platform, tenant, row, result, cooldown_until).await?;
                warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = heap.relation_name(), reason = <&'static str>::from(reason), cleanup = ?cleanup, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "repack-abandoned", "PostgreSQL repack abandoned");
                Ok(RepackTaskOutcome::Abandoned)
            }
            RepackOutcome::Failed { error, cleanup } => {
                record_action(MaintenanceAction::Repack, MaintenanceActionOutcome::Error, duration, None);
                let (result, cooldown_until) = failed(ActionResult::Failed);
                target_record::record_action_failure(platform, tenant, row, result, cooldown_until).await?;
                warn!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = heap.relation_name(), error = %error, cleanup = ?cleanup, cooldown_until = %cooldown_until, duration_seconds = duration.as_secs_f64(), decision = "action-failed", "PostgreSQL repack failed");
                Ok(RepackTaskOutcome::Failed)
            }
            RepackOutcome::Cancelled { cleanup } => {
                record_action(MaintenanceAction::Repack, MaintenanceActionOutcome::Cancelled, duration, None);
                info!(tenant = tenant.log_name(), database = %tenant.database, branch = ?row.branch(), target = heap.relation_name(), cleanup = ?cleanup, decision = "cancelled", "PostgreSQL repack cancelled");
                Ok(RepackTaskOutcome::Cancelled)
            }
        }
    }
}

/// The result an abandoned run records, `None` for one that never started
/// because another run holds the table or left its trigger: the repack stays
/// queued without a back-off, and the run's owner or recovery deals with the
/// table first.
const fn abandonment_result(reason: RepackAbandonment) -> Option<ActionResult> {
    match reason {
        RepackAbandonment::LockWait => Some(ActionResult::RepackAbandonedLockWait),
        RepackAbandonment::NotConverging => Some(ActionResult::RepackNotConverging),
        RepackAbandonment::Yielded => Some(ActionResult::RepackYielded),
        RepackAbandonment::InvalidIndex | RepackAbandonment::ScriptAbsent(_) => Some(ActionResult::Failed),
        RepackAbandonment::TableLocked | RepackAbandonment::ConflictingTrigger => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        evidence::action_result::ActionResult,
        repack::RepackAbandonment,
        repack_task::{NextStep, RepackTaskOutcome, abandonment_result},
    };
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn a_repack_held_back_before_it_ran_lets_the_next_one_due_run() {
        let next = RepackTaskOutcome::iter()
            .filter(|outcome| outcome.next_step() == NextStep::NextRepack)
            .collect::<Vec<_>>();
        assert_eq!(
            next,
            [
                RepackTaskOutcome::NothingQueued,
                RepackTaskOutcome::Unregistered,
                RepackTaskOutcome::TenantUnavailable,
                RepackTaskOutcome::SchemaBehind,
                RepackTaskOutcome::Lapsed,
                RepackTaskOutcome::Deferred,
                RepackTaskOutcome::Withheld,
            ]
        );
    }

    #[test]
    fn a_repack_that_ran_or_an_execution_that_stopped_ends_the_execution() {
        for outcome in [
            RepackTaskOutcome::Repacked,
            RepackTaskOutcome::Abandoned,
            RepackTaskOutcome::Failed,
            RepackTaskOutcome::Cancelled,
            RepackTaskOutcome::Observing,
            RepackTaskOutcome::NotClaimed,
        ] {
            assert_eq!(outcome.next_step(), NextStep::Done, "{outcome}");
        }
    }

    #[test]
    fn each_abandonment_records_its_own_result_and_a_held_table_records_none() {
        assert_eq!(abandonment_result(RepackAbandonment::LockWait), Some(ActionResult::RepackAbandonedLockWait));
        assert_eq!(abandonment_result(RepackAbandonment::NotConverging), Some(ActionResult::RepackNotConverging));
        assert_eq!(abandonment_result(RepackAbandonment::Yielded), Some(ActionResult::RepackYielded));
        assert_eq!(abandonment_result(RepackAbandonment::InvalidIndex), Some(ActionResult::Failed));
        assert_eq!(abandonment_result(RepackAbandonment::TableLocked), None);
        assert_eq!(abandonment_result(RepackAbandonment::ConflictingTrigger), None);
    }
}
