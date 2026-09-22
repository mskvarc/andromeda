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
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver, WakeHandle},
};
use chrono::{DateTime, Utc};
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::{broadcast, broadcast::error::RecvError};

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

/// Register a resident task that pulses `wake` whenever a lifecycle change
/// arrives on `changes`, so the periodic sweep it drives notices a new or
/// amended subscription within its fallback floor rather than its ceiling.
///
/// A `timeInterval` subscription is only ever discovered when the sweep runs,
/// and a wake-triggered drain that nothing wakes runs only on the geometric
/// backoff bounded by `interval_max_sleep_secs` (clause 5.8.6 fixes the period,
/// not the discovery latency). Without this bridge a subscription created just
/// after an idle pass waits out the whole ceiling before its first notification.
/// Forwarding every change to the sweep's wake is what makes the ceiling the
/// fallback the operator-facing setting documents, not the common case.
///
/// The wake coalesces, so a burst of changes costs the sweep one prompt pass; a
/// lagged stream still means the subscription set changed, so it wakes too.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the task name is rejected or the
/// supervisor refuses the registration.
pub(super) fn register_periodic_wake<T>(
    supervisor: &TaskSupervisor,
    task_name: &'static str,
    wake: WakeHandle,
    changes: broadcast::Sender<T>,
) -> Result<(), TenantAssemblyError>
where
    T: Clone + Send + 'static,
{
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from(task_name).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, restart_delay, restart_delay),
        ShutdownPolicy::new(restart_delay),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let wake = wake.clone(); // clone: each resident attempt owns a wake handle it pulses.
        let mut changes = changes.subscribe();
        async move {
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = changes.recv() => match received {
                        Ok(_) | Err(RecvError::Lagged(_)) => {
                            let _ = wake.wake();
                        }
                        Err(RecvError::Closed) => return Err(TaskFailure::transient("subscription change channel closed")),
                    },
                }
            }
        }
    });
    supervisor
        .register_resident(task)
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
