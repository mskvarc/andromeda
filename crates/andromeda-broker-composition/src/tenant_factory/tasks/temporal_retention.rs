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
};
use andromeda_postgresql_temporal_store::{
    error::TemporalStoreError,
    repo::TemporalRepository,
    retention_sweep::{TemporalRetention, TemporalRetentionHorizon, TemporalRetentionOutcome},
};
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    failure::{FailureClass, TaskFailure},
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
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let repository = Arc::clone(&repository); // clone: each bounded pass owns the repository handle while running.
        async move {
            let outcome = repository.sweep_retention(retention, Utc::now()).await.map_err(classify_retention_failure)?;
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
        .map_err(|error| AssemblyInvariant::from(error).into())
}

/// Classify exhaustive temporal-store failures at the composition boundary.
const fn classify_retention_failure(error: TemporalStoreError) -> TaskFailure<TemporalStoreError> {
    let class = match &error {
        // A collation object is created on demand by a read that orders under
        // it (clause 4.23.1); the statement fails for the same reasons any
        // other one does, so a retry is worth taking.
        TemporalStoreError::Database(_) | TemporalStoreError::NameLookup(_) | TemporalStoreError::Collation(_) => FailureClass::Transient,
        TemporalStoreError::WriteOrdinal(_)
        | TemporalStoreError::StatementBuild(_)
        | TemporalStoreError::RetentionHorizon(_)
        | TemporalStoreError::StoredTime(_)
        | TemporalStoreError::StoredJson(_)
        | TemporalStoreError::ScopeHasNoAttributeRow
        | TemporalStoreError::StoredHistory(_)
        | TemporalStoreError::StoredCatalog(_)
        | TemporalStoreError::StoredAggregation(_)
        | TemporalStoreError::Render(_)
        | TemporalStoreError::Encoding(_)
        | TemporalStoreError::HistoryMembers(_)
        | TemporalStoreError::Invariant(_)
        | TemporalStoreError::MixedAggregationGroup(_)
        | TemporalStoreError::WriteRecordMismatch { .. }
        | TemporalStoreError::ColumnstoreSettingsDrift { .. }
        | TemporalStoreError::UnitCode(_)
        // Retention only deletes aged rows and records no intent, so a
        // failure naming the entity whose intent it recorded cannot reach it.
        | TemporalStoreError::EntityIntent { .. } => FailureClass::InvariantViolation,
        TemporalStoreError::Refused(_)
        | TemporalStoreError::NotFound(_)
        | TemporalStoreError::TimestampPrecision(_)
        // Retention only deletes aged rows and never claims an instanceId, so
        // this variant cannot actually reach this classifier; it is grouped
        // with the other client-input refusals because that is what it would
        // mean if it somehow did.
        | TemporalStoreError::DuplicateInstanceClaim(_) => FailureClass::Permanent,
    };
    TaskFailure::new(class, error)
}
