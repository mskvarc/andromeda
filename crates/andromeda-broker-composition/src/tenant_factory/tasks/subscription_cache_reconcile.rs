//! The pass that reads again the subscriptions whose change the matching cache
//! could not read.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
use andromeda_notification::processor::NotificationProcessor;
use andromeda_retry::{schedule::BackoffSchedule, tokio_retry_delay::TokioRetryDelay};
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
use std::{convert::Infallible, sync::Arc, time::Duration};

/// Register the task that, every `interval`, reads again each subscription the
/// cache marked dirty after a failed read.
///
/// A pass with nothing dirty reads nothing. The task is observed, not
/// tenant-degrading: a subscription it could not reach stays marked and the
/// next pass tries it again.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the interval or the name is rejected,
/// or when the supervisor refuses the registration.
pub(super) fn register_subscription_cache_reconcile(
    supervisor: &TaskSupervisor,
    processor: Arc<NotificationProcessor>,
    interval: Duration,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(interval, InitialExecution::AfterCadence, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("subscription-cache-reconcile").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(interval)),
        ShutdownPolicy::new(interval),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let processor = Arc::clone(&processor); // clone: each pass owns the shared processor while running.
        async move {
            processor.reconcile_subscriptions(&TokioRetryDelay).await;
            Ok::<(), TaskFailure<Infallible>>(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}
