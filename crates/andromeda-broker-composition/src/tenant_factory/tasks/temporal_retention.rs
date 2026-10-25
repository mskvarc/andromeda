//! Registration of the operator temporal-retention sweep.
//!
//! Retention ages temporal storage out on a cadence the operator chooses. It is
//! not an NGSI-LD operation: the specification defines deletion of a Temporal
//! Evolution (clause 5.6.16), while
//! a dropped retention chunk is storage the operator decided not to keep and
//! raises nothing. The task is therefore registered only once a horizon has been
//! configured, and no broker runs it by default.

use crate::tenant_factory::{
    assembly_invariant::{AssemblyInvariant, RetentionSetting},
    error::TenantAssemblyError,
    tasks::temporal_sweep_failure::classify_sweep_failure,
};
use andromeda_postgresql_temporal_store::{
    repo::TemporalRepository,
    retention_sweep::{TemporalRetention, TemporalRetentionHorizon, TemporalRetentionOutcome},
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
use andromeda_temporal::config::TemporalConfig;
use chrono::{TimeDelta, Utc};
use std::sync::Arc;

/// Register the retention sweep when the operator has configured a horizon.
///
/// Returns without registering anything when no horizon is configured, which is
/// how a broker behaves unless retention is turned on.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when the configured horizon or
/// cadence names no usable schedule, or when the supervisor refuses the task.
pub(super) fn register_temporal_retention(
    supervisor: &TaskSupervisor,
    repository: Arc<TemporalRepository>,
    config: &TemporalConfig,
) -> Result<(), TenantAssemblyError> {
    let Some(horizon) = config.retention_horizon else {
        return Ok(());
    };
    // A validated retention horizon is a whole number of days (see
    // `TemporalConfig::validate`), so integer division recovers it exactly.
    let horizon_days =
        i64::try_from(horizon.duration().as_secs() / 86_400).map_err(|_| AssemblyInvariant::RetentionOutOfRange(RetentionSetting::TemporalHorizon))?;
    let span = TimeDelta::try_days(horizon_days).ok_or(AssemblyInvariant::RetentionOutOfRange(RetentionSetting::TemporalHorizon))?;
    let retention = TemporalRetention::Enabled {
        horizon: TemporalRetentionHorizon::new(span).map_err(AssemblyInvariant::from)?,
    };
    let cadence = config.retention_sweep_interval.duration();
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
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
