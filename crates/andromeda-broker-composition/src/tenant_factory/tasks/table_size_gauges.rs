//! Periodic sampling of one tenant's base-table sizes, heap-only update shares
//! and the history chunk each temporal hypertable converts next into the
//! metrics gauges.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, background_pacer::BackgroundPacer, error::TenantAssemblyError};
use andromeda_postgresql_maintenance::{error::MaintenanceError, history_chunk_size::rowstore_chunk_sizes, table_size::table_sizes};
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
use andromeda_telemetry::storage::{StorageGauges, TableSizeSample};
use andromeda_tenancy::tenant_ref::TenantRef;
use sqlx::PgPool;
use std::{
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

/// How often base-table sizes are sampled.
///
/// Coarse on purpose: the sizes move slowly and each pass runs a catalog query,
/// so a minute keeps the load negligible.
const SAMPLE_CADENCE: Duration = Duration::from_secs(60);

/// Register the task that publishes the tenant's base-table sizes and the size
/// of each history hypertable's largest unconverted chunk each minute.
///
/// The sizes are bounded catalog reads over fixed table allowlists, so the
/// task is merely observed: its failure never degrades the tenant. Sizes move
/// only when something writes, so a tick samples only when the tenant has been
/// used or a background pass has run since the last sample.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the cadence names no usable
/// schedule or the supervisor refuses the registration.
pub(super) fn register_table_size_gauges(
    supervisor: &TaskSupervisor,
    pacer: Arc<BackgroundPacer>,
    pool: PgPool,
    tenant: &TenantRef,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(SAMPLE_CADENCE, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("table-size-gauges").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, SAMPLE_CADENCE, SAMPLE_CADENCE),
        ShutdownPolicy::new(SAMPLE_CADENCE),
    );
    let gauges = StorageGauges::for_tenant(tenant.to_string());
    // The write count the last sample ran under, shared by every tick.
    let observed = Arc::new(AtomicU64::new(0));
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let pool = pool.clone(); // clone: PgPool is a shared handle; each pass acquires one connection.
        let gauges = gauges.clone(); // clone: each bounded pass owns the gauge labels while recording.
        let pacer = Arc::clone(&pacer); // clone: each pass consults the tenant's one pacer.
        let observed = Arc::clone(&observed); // clone: every tick compares against the one last-sample count.
        async move {
            pacer
                .run_when_written(&observed, async move {
                    let mut connection = pool
                        .acquire()
                        .await
                        .map_err(|error| TaskFailure::transient(MaintenanceError::Database(error)))?;
                    let samples = table_sizes(&mut connection).await.map_err(TaskFailure::transient)?;
                    for sample in samples {
                        gauges.record(
                            sample.table.relation_name(),
                            TableSizeSample {
                                total_bytes: sample.total_bytes,
                                index_bytes: sample.index_bytes,
                                updated_tuples: sample.updated_tuples,
                                hot_updated_tuples: sample.hot_updated_tuples,
                            },
                        );
                    }
                    for sample in rowstore_chunk_sizes(&mut connection).await.map_err(TaskFailure::transient)? {
                        gauges.record_largest_rowstore_chunk(sample.table.relation_name(), sample.largest_bytes);
                    }
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
