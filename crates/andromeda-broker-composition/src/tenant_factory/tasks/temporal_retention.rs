//! Registration of the operator temporal-retention sweep.
//!
//! Retention ages temporal storage out on a cadence the operator chooses. It is
//! not an NGSI-LD operation: the specification defines deletion of a Temporal
//! Evolution (clause 5.6.16), while
//! a dropped retention chunk is storage the operator decided not to keep and
//! raises nothing. The task is therefore registered only once a horizon has been
//! configured, and no broker runs it by default.
//!
//! The first sweep runs when the tenant opens. A tenant the wake loop loaded
//! for history past the horizon stays loaded only briefly, and a tenant used
//! in short bursts may never stay a full interval, so a first sweep one
//! interval later would never run on either.

use crate::tenant_factory::{assembly_invariant::AssemblyInvariant, error::TenantAssemblyError, tasks::temporal_sweep_failure::classify_sweep_failure};
use andromeda_configuration::schema::temporal::storage::RetentionConfig;
use andromeda_postgresql_temporal_store::{
    repo::TemporalRepository,
    retention_sweep::{TemporalRetention, TemporalRetentionOutcome},
};
use andromeda_retry::schedule::BackoffSchedule;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
};
use chrono::Utc;
use std::sync::Arc;

/// Register the retention sweep when the operator has enabled retention.
///
/// Returns without registering anything while retention is disabled, which is
/// how a broker behaves unless retention is turned on.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the configured cadence
/// names no usable schedule, or when the supervisor refuses the task.
pub(super) fn register_temporal_retention(
    supervisor: &TaskSupervisor,
    repository: Arc<TemporalRepository>,
    retention: TemporalRetention,
    config: &RetentionConfig,
) -> Result<(), TenantAssemblyError> {
    if retention == TemporalRetention::Disabled {
        return Ok(());
    }
    let cadence = config.sweep_interval.duration();
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("temporal-retention").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, BackoffSchedule::constant(cadence)),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let repository = Arc::clone(&repository); // clone: each bounded pass owns the repository handle while running.
        async move {
            let outcome = repository.sweep_retention(retention, Utc::now()).await.map_err(classify_sweep_failure)?;
            match outcome {
                TemporalRetentionOutcome::Disabled => Ok(()),
                TemporalRetentionOutcome::Swept(sweep) => {
                    tracing::info!(
                        cutoff = %sweep.cutoff,
                        attribute_history_chunks = sweep.attribute_history_chunks,
                        scope_events = sweep.scope_events,
                        streams = sweep.streams,
                        narrowed_shape_masks = sweep.narrowed_shape_masks,
                        vacated_evolutions = sweep.vacated_evolutions,
                        "temporal retention swept storage older than the configured horizon"
                    );
                    Ok(())
                }
            }
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}
