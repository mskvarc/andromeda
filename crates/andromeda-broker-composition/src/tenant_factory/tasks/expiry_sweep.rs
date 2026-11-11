//! The registration every sweep shares that deletes one kind of resource past
//! its expiry and counts what it deleted.
//!
//! The sweep runs at once and then every cadence, skips a missed tick rather
//! than replaying it, and runs only when the tenant's pacer finds something
//! due. A transient failure is retried after one cadence.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, background_pacer::BackgroundPacer, error::TenantAssemblyError};
use andromeda_retry::schedule::BackoffSchedule;
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
use chrono::{DateTime, Utc};
use std::{error::Error, future::Future, sync::Arc, time::Duration};

/// Register the sweep `name` that every `cadence` runs `sweep` with the
/// tick's cutoff, which deletes what is due by it and counts what it deleted.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the cadence or the name is rejected,
/// or when the supervisor refuses the registration.
pub(super) fn register_expiry_sweep<F, Fut, E>(
    supervisor: &TaskSupervisor,
    pacer: Arc<BackgroundPacer>,
    name: &'static str,
    cadence: Duration,
    sweep: F,
) -> Result<(), TenantAssemblyError>
where
    F: Fn(DateTime<Utc>) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<u64, TaskFailure<E>>> + Send + 'static,
    E: Error,
{
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let task_name = TaskName::try_from(name).map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        task_name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(cadence)),
        ShutdownPolicy::new(cadence),
    );
    let sweep = Arc::new(sweep);
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let sweep = Arc::clone(&sweep);
        let pacer = Arc::clone(&pacer);
        async move {
            pacer
                .run(async move {
                    let expired = sweep(DateTime::<Utc>::from(execution.cutoff())).await?;
                    tracing::info!(expired, task = name, "expiry sweep completed");
                    Ok(())
                })
                .await
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}
