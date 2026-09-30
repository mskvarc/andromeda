//! Periodic sampling of one tenant's connection pool into the metrics gauges.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError};
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
use andromeda_telemetry::pool::{PoolGauges, PoolSample};
use andromeda_tenancy::tenant_ref::TenantRef;
use sqlx::PgPool;
use std::{convert::Infallible, time::Duration};

/// How often the pool occupancy is sampled.
const SAMPLE_CADENCE: Duration = Duration::from_secs(5);

/// Register the task that publishes the tenant pool's occupancy every few seconds.
///
/// The gauges are cheap reads of pool counters, so the task is merely observed:
/// its failure never degrades the tenant.
pub(super) fn register_pool_gauges(supervisor: &TaskSupervisor, pool: PgPool, tenant: &TenantRef) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(SAMPLE_CADENCE, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("pool-gauges").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, SAMPLE_CADENCE, SAMPLE_CADENCE),
        ShutdownPolicy::new(SAMPLE_CADENCE),
    );
    let gauges = PoolGauges::for_tenant(tenant.to_string());
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let pool = pool.clone(); // clone: PgPool is a shared handle; each pass reads its counters.
        let gauges = gauges.clone(); // clone: each bounded pass owns the gauge labels while recording.
        async move {
            gauges.record(PoolSample {
                open: pool.size(),
                idle: pool.num_idle(),
            });
            Ok::<(), TaskFailure<Infallible>>(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}
