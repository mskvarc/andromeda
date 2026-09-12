//! Registering a periodic notification evaluation, and classifying what one
//! reports when it fails.
//!
//! Entity notifications and context source notifications are different
//! policies with different operations, but both are swept on a fixed cadence
//! by a task the supervisor owns, and both report the same
//! [`NotificationError`]. Registering the schedule and reading that error are
//! the two things they genuinely share.

use crate::tenant_factory::error::TenantAssemblyError;
use andromeda_notification::error::NotificationError;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    failure::TaskFailure,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
};
use std::{future::Future, sync::Arc, time::Duration};

/// Register `operation` as a scheduled sweep named `task_name`.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the cadence or the name is rejected,
/// or when the supervisor refuses the registration.
pub(super) fn register_notification_schedule<F, Fut>(supervisor: &TaskSupervisor, task_name: &'static str, operation: F) -> Result<(), TenantAssemblyError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<usize, NotificationError>> + Send + 'static,
{
    let cadence = Duration::from_secs(1);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from(task_name).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let operation = Arc::new(operation);
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let operation = Arc::clone(&operation); // clone: scheduled execution owns its operation factory.
        async move {
            let due = operation().await.map_err(|error| classify_notification_failure(&error))?;
            tracing::debug!(task = task_name, due, "interval notification evaluation completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Whether a notification failure ends the worker, degrades the tenant, or is
/// worth another attempt.
pub(super) fn classify_notification_failure(error: &NotificationError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        NotificationError::Entity(_) | NotificationError::Subscription(_) | NotificationError::Registration(_) | NotificationError::JsonLd(_) => {
            TaskFailure::transient(summary)
        }
        NotificationError::Persistence { .. } => TaskFailure::transient(summary),
        NotificationError::Representation { .. } => TaskFailure::permanent(summary),
        NotificationError::Invariant { .. } => TaskFailure::invariant_violation(summary),
    }
}
