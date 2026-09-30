//! Periodic folding of one tenant's discovery census ledgers.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, background_pacer::BackgroundPacer, error::TenantAssemblyError};
use andromeda_postgresql_context_store::entity::census::fold::fold_every_census;
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
use sqlx::PgPool;
use std::{
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

/// How often the census ledgers are folded.
///
/// Every discovery read sums the rows of the keys it answers, and every write
/// that changes a type or an attribute instance adds rows; half a minute keeps
/// the rows a read sums near one per key under any sustained write rate, while
/// a tenant at rest folds nothing.
const FOLD_CADENCE: Duration = Duration::from_secs(30);

/// Register the task that folds the census of the live branch and of every
/// published Snapshot branch of the tenant.
///
/// A fold never changes an answer, only how many rows answering it sums, so
/// the task is merely observed: a failed pass leaves the census exactly as
/// correct and a little larger. A tick folds only when the tenant has been
/// written or a background pass has run since the last fold.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the cadence names no usable
/// schedule or the supervisor refuses the registration.
pub(super) fn register_discovery_census_fold(supervisor: &TaskSupervisor, pacer: Arc<BackgroundPacer>, pool: PgPool) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(FOLD_CADENCE, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("discovery-census-fold").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, FOLD_CADENCE, FOLD_CADENCE),
        ShutdownPolicy::new(FOLD_CADENCE),
    );
    // The write count the last fold ran under, shared by every tick.
    let observed = Arc::new(AtomicU64::new(0));
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let pool = pool.clone(); // clone: PgPool is a shared handle; each pass acquires its own connection.
        let pacer = Arc::clone(&pacer); // clone: each pass consults the tenant's one pacer.
        let observed = Arc::clone(&observed); // clone: every tick compares against the one last-fold count.
        async move {
            pacer
                .run_when_written(&observed, async move {
                    let outcome = fold_every_census(&pool).await.map_err(TaskFailure::transient)?;
                    tracing::debug!(summed_rows = outcome.summed_rows, "discovery census folded");
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
