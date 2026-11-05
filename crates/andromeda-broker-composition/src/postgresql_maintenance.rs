//! Process-scoped PostgreSQL maintenance task registration: the controller's
//! passes, and beside them the task that runs the repacks the passes queue.

use andromeda_configuration::schema::database::maintenance::{MaintenanceConfig, MaintenanceMode};
use andromeda_postgresql_context_store::{error::ContextStoreError, migration_set::branch::current_branch_version};
use andromeda_postgresql_maintenance::{
    controller::MaintenanceController,
    error::MaintenanceError,
    expected_deployment::ExpectedDeployment,
    policy::PolicyProfile,
    repack_task::RepackTask,
};
use andromeda_postgresql_pool::database_url::DatabaseUrl;
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    failure::{FailureClass, TaskFailure},
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule, PeriodicScheduleError},
    registration::{TaskRegistration, TaskRegistrationError},
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::{TaskName, TaskNameError},
};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tracing::debug;

/// Failure while assembling the process-scoped maintenance schedule.
#[derive(Debug, Error)]
pub(crate) enum MaintenanceRegistrationError {
    /// The fixed cadence or jitter is invalid.
    #[error("invalid PostgreSQL maintenance schedule: {0}")]
    Schedule(#[from] PeriodicScheduleError),
    /// The fixed task name is invalid.
    #[error("invalid PostgreSQL maintenance task name: {0}")]
    Name(#[from] TaskNameError),
    /// A process task already owns the fixed name.
    #[error("PostgreSQL maintenance task registration failed: {0}")]
    Registration(#[from] TaskRegistrationError),
    /// The binary's branch schema version could not be established.
    #[error("PostgreSQL maintenance branch schema version: {0}")]
    BranchVersion(#[source] ContextStoreError),
}

/// Register the one process-scoped controller unless maintenance is disabled.
pub(crate) fn register_postgresql_maintenance(
    supervisor: &TaskSupervisor,
    platform_pool: PgPool,
    direct_url: DatabaseUrl,
    config: MaintenanceConfig,
    acquire_timeout: Duration,
    expected: ExpectedDeployment,
) -> Result<(), MaintenanceRegistrationError> {
    if config.mode == MaintenanceMode::Disabled {
        return Ok(());
    }
    let profile = PolicyProfile::from_profile(config.profile);
    let schedule = PeriodicSchedule::new(profile.cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)?.with_jitter(profile.maximum_jitter)?;
    let registration = TaskRegistration::new(
        TaskName::try_from("postgresql-maintenance")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(profile.cadence)),
        ShutdownPolicy::new(Duration::from_secs(30)),
    );
    let branch_version = current_branch_version().map_err(|error| MaintenanceRegistrationError::BranchVersion(ContextStoreError::BranchVersion(error)))?;
    let repack = Arc::new(RepackTask::new(
        platform_pool.clone(), // clone: the repack task holds its own platform connection beside the pass's
        direct_url.clone(),    // clone: the repack task derives tenant URLs over the direct base
        config,
        acquire_timeout,
        branch_version,
    ));
    let controller = Arc::new(MaintenanceController::new(
        platform_pool,
        direct_url,
        config,
        acquire_timeout,
        expected,
        branch_version,
    ));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let controller = Arc::clone(&controller); // clone: each bounded pass owns the process controller while executing
        async move {
            let outcome = controller.run(execution.cancellation()).await.map_err(classify_failure)?;
            debug!(outcome = %outcome, "PostgreSQL maintenance pass finished");
            Ok(())
        }
    });
    supervisor.register_scheduled(task).map(drop).map_err(MaintenanceRegistrationError::from)?;
    register_repack_task(supervisor, repack, profile)
}

/// Register the repack task on the pass cadence. It claims one queued repack
/// per execution under the repack fleet lock, so on every replica it runs
/// beside the passes, and a shutdown gives a running repack the time its
/// cleanup takes.
fn register_repack_task(supervisor: &TaskSupervisor, repack: Arc<RepackTask>, profile: PolicyProfile) -> Result<(), MaintenanceRegistrationError> {
    let schedule = PeriodicSchedule::new(profile.cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)?.with_jitter(profile.maximum_jitter)?;
    let registration = TaskRegistration::new(
        TaskName::try_from("postgresql-repack")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(profile.cadence)),
        ShutdownPolicy::new(Duration::from_secs(60)),
    );
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let repack = Arc::clone(&repack); // clone: each execution owns the process task while it runs
        async move {
            let outcome = repack.run(execution.cancellation()).await.map_err(classify_failure)?;
            debug!(outcome = %outcome, "PostgreSQL repack task finished");
            Ok(())
        }
    });
    supervisor.register_scheduled(task).map(drop).map_err(MaintenanceRegistrationError::from)
}

/// Classify platform failures; per-tenant capability and action failures are pass outcomes.
const fn classify_failure(error: MaintenanceError) -> TaskFailure<MaintenanceError> {
    let class = match &error {
        MaintenanceError::Database(_) | MaintenanceError::ExtensionVersions(ContextStoreError::Database(_)) => FailureClass::Transient,
        MaintenanceError::SchemaContract(_)
        | MaintenanceError::NegativeCatalogValue { .. }
        | MaintenanceError::CatalogValueOverflow { .. }
        | MaintenanceError::Statement(_)
        | MaintenanceError::TenantName(_)
        | MaintenanceError::UnknownTarget(_)
        | MaintenanceError::UnknownWalStatus(_)
        | MaintenanceError::ExtensionVersions(_)
        | MaintenanceError::UnknownHorizonHolder(_)
        | MaintenanceError::UnreadableCode(_)
        | MaintenanceError::ProbeTargetOfAnotherKind(_)
        | MaintenanceError::FleetLockNotHeld(_)
        | MaintenanceError::RepackVersion(_)
        | MaintenanceError::UnknownLockMode(_)
        | MaintenanceError::KeyLayoutTargetNotDensityMeasured(_)
        | MaintenanceError::InvalidKeyLayout { .. }
        | MaintenanceError::UnknownBranchSchema
        | MaintenanceError::InvalidSnapshotKey(_)
        | MaintenanceError::UnreadableSnapshotStatus(_)
        | MaintenanceError::BranchLockNotHeld { .. }
        | MaintenanceError::LiveOnlyTarget { .. } => FailureClass::InvariantViolation,
    };
    TaskFailure::new(class, error)
}
