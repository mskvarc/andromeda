//! Registration of the operator temporal-retention sweep.
//!
//! Retention ages temporal storage out on a cadence the operator chooses. It is
//! not an NGSI-LD operation: the specification defines deletion of a Temporal
//! Evolution (clause 5.6.16), while
//! a dropped retention chunk is storage the operator decided not to keep and
//! raises nothing. The task is therefore registered only once a horizon has been
//! configured, and no broker runs it by default.

use crate::tenant_factory::error::TenantAssemblyError;
use andromeda_postgresql_temporal_store::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    retention_sweep::{TemporalRetention, TemporalRetentionHorizon, TemporalRetentionOutcome},
};
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
    let horizon_days = i64::try_from(horizon.duration().as_secs() / 86_400).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let span = TimeDelta::try_days(horizon_days).ok_or_else(|| TenantAssemblyError::invariant("temporal retention horizon is out of range".to_owned()))?;
    let retention = TemporalRetention::Enabled {
        horizon: TemporalRetentionHorizon::new(span).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?,
    };
    let cadence = config.retention_sweep_interval.duration();
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("temporal-retention").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let repository = Arc::clone(&repository); // clone: each bounded pass owns the repository handle while running.
        async move {
            let outcome = repository
                .sweep_retention(retention, Utc::now())
                .await
                .map_err(|error| classify_retention_failure(&error))?;
            match outcome {
                TemporalRetentionOutcome::Disabled => Ok(()),
                TemporalRetentionOutcome::Swept(sweep) => {
                    tracing::info!(
                        cutoff = %sweep.cutoff,
                        attribute_history_chunks = sweep.attribute_history_chunks,
                        scope_history_chunks = sweep.scope_history_chunks,
                        streams = sweep.streams,
                        temporal_entities = sweep.temporal_entities,
                        released_write_records = sweep.released_write_records,
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
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Classify exhaustive temporal-store failures at the composition boundary.
fn classify_retention_failure(error: &TemporalStoreError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        // A collation object is created on demand by a read that orders under
        // it (clause 4.23.1); the statement fails for the same reasons any
        // other one does, so a retry is worth taking.
        TemporalStoreError::Database(_) | TemporalStoreError::NameLookup(_) | TemporalStoreError::Collation(_) => TaskFailure::transient(summary),
        TemporalStoreError::Decode(_)
        | TemporalStoreError::Encoding(_)
        | TemporalStoreError::CopyField(_)
        | TemporalStoreError::MixedAggregationGroup(_)
        | TemporalStoreError::UnitCode(_) => TaskFailure::invariant_violation(summary),
        TemporalStoreError::Validation(_)
        | TemporalStoreError::Query(_)
        | TemporalStoreError::NotFound(_)
        | TemporalStoreError::TimestampPrecision(_)
        // Retention only deletes aged rows and never claims an instanceId, so
        // this variant cannot actually reach this classifier; it is grouped
        // with the other client-input refusals because that is what it would
        // mean if it somehow did.
        | TemporalStoreError::DuplicateInstanceClaim(_) => TaskFailure::permanent(summary),
    }
}
