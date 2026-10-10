//! Process-scoped PostgreSQL maintenance task registration.

use andromeda_postgresql_context_store::{
    maintenance_config::{MaintenanceConfig, MaintenanceMode},
    tenant_database::DatabaseUrl,
};
use andromeda_postgresql_maintenance::{controller::MaintenanceController, error::MaintenanceError, policy::PolicyProfile};
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
}

/// Register the one process-scoped controller unless maintenance is disabled.
pub(crate) fn register_postgresql_maintenance(
    supervisor: &TaskSupervisor,
    platform_pool: PgPool,
    direct_url: DatabaseUrl,
    config: MaintenanceConfig,
    acquire_timeout: Duration,
) -> Result<(), MaintenanceRegistrationError> {
    if config.mode == MaintenanceMode::Disabled {
        return Ok(());
    }
    let profile = PolicyProfile::from_profile(config.profile);
    let schedule = PeriodicSchedule::new(profile.cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)?.with_jitter(profile.maximum_jitter)?;
    let registration = TaskRegistration::new(
        TaskName::try_from("postgresql-maintenance")?,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, profile.cadence, profile.cadence),
        ShutdownPolicy::new(Duration::from_secs(30)),
    );
    let controller = Arc::new(MaintenanceController::new(platform_pool, direct_url, config, acquire_timeout));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let controller = Arc::clone(&controller); // clone: each bounded pass owns the process controller while executing
        async move {
            let outcome = controller.run(execution.cancellation()).await.map_err(classify_failure)?;
            debug!(outcome = %outcome, "PostgreSQL maintenance pass finished");
            Ok(())
        }
    });
    supervisor.register_scheduled(task).map(drop).map_err(MaintenanceRegistrationError::from)
}

/// Classify platform failures; per-tenant capability and action failures are pass outcomes.
const fn classify_failure(error: MaintenanceError) -> TaskFailure<MaintenanceError> {
    let class = match &error {
        MaintenanceError::Database(_) => FailureClass::Transient,
        MaintenanceError::SchemaContract(_)
        | MaintenanceError::NegativeCatalogValue { .. }
        | MaintenanceError::CatalogValueOverflow { .. }
        | MaintenanceError::Statement(_)
        | MaintenanceError::TenantName(_)
        | MaintenanceError::UnknownTarget(_)
        | MaintenanceError::UnknownHorizonHolder(_)
        | MaintenanceError::UnreadableCode(_)
        | MaintenanceError::ProbeTargetNotAnIndex(_)
        | MaintenanceError::InvalidDensityBaseline { .. }
        | MaintenanceError::UnknownBranchSchema
        | MaintenanceError::LiveOnlyTarget { .. } => FailureClass::InvariantViolation,
    };
    TaskFailure::new(class, error)
}
