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
    drain_task::{DrainPassOutcome, DrainTask},
    failure::TaskFailure,
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver},
};
use chrono::{DateTime, Utc};
use std::{future::Future, sync::Arc, time::Duration};

/// Register `operation` as a wake-driven periodic sweep named `task_name`.
///
/// The operation runs one sweep at `cutoff` and reports the earliest instant a
/// subscription is next due, or `None` when none is; the drain then sleeps until
/// then (clause 5.8.6, floor 1 s) rather than polling every second, waking early
/// on `wake`, and never sleeping past `ceiling`.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the floor/ceiling or the name is
/// rejected, or when the supervisor refuses the registration.
pub(super) fn register_notification_schedule<F, Fut>(
    supervisor: &TaskSupervisor,
    task_name: &'static str,
    ceiling: Duration,
    wake: DrainWakeReceiver,
    operation: F,
) -> Result<(), TenantAssemblyError>
where
    F: Fn(DateTime<Utc>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Option<DateTime<Utc>>, NotificationError>> + Send + 'static,
{
    // Clause 5.8.6 fixes the shortest period at one second; the ceiling only
    // bounds how long the drain waits when nothing is due.
    let floor = Duration::from_secs(1);
    let fallback = DrainFallbackPolicy::new(floor, ceiling.max(floor)).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from(task_name).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, floor, ceiling.max(floor)),
        ShutdownPolicy::new(floor),
    );
    let operation = Arc::new(operation);
    let task = DrainTask::new(registration, fallback, move |execution| {
        let operation = Arc::clone(&operation); // clone: each bounded pass owns its operation while executing.
        async move {
            let cutoff = DateTime::<Utc>::from(execution.cutoff());
            match operation(cutoff).await.map_err(|error| classify_notification_failure(&error))? {
                Some(next_due) => Ok(DrainPassOutcome::IdleUntil(next_due.into())),
                None => Ok(DrainPassOutcome::Idle),
            }
        }
    });
    supervisor
        .register_drain(task, wake)
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
