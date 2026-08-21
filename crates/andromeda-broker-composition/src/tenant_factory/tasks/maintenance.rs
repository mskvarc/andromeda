//! Durable entity side-effect and spatial-maintenance task registration.

use crate::tenant_factory::error::TenantAssemblyError;
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_entity::{
    error::EntityError,
    port::spatial_snapshot::EntitySpatialSnapshotSource,
    side_effect_drain::EntitySideEffectDrain,
    spatial_reconciliation::reconcile_location,
    spatial_synchronization::SpatialSynchronizer,
};
use andromeda_registration::error::RegistrationError;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_subscription::error::SubscriptionError;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::TaskFailure,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackCadence, WakeHandle},
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;

pub(super) fn register_entity_side_effect_drain(
    supervisor: &TaskSupervisor,
    drain: Arc<EntitySideEffectDrain>,
    fallback_duration: Duration,
    lease_duration: Duration,
) -> Result<WakeHandle, TenantAssemblyError> {
    let fallback = DrainFallbackCadence::new(fallback_duration).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("entity-side-effect-delivery").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback_duration, fallback_duration),
        ShutdownPolicy::new(lease_duration),
    );
    let task = DrainTask::new(registration, fallback, move |execution| {
        let drain = Arc::clone(&drain); // clone: each bounded future owns the policy operation while executing.
        async move {
            let now = DateTime::<Utc>::from(execution.cutoff());
            let outcome = drain.execute(now).await.map_err(classify_entity_expiry_failure)?;
            if outcome.work_may_remain {
                Ok(DrainPassOutcome::WorkMayRemain)
            } else {
                Ok(DrainPassOutcome::Idle)
            }
        }
    });
    supervisor
        .register_drain(task)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Register the periodic reap of dead-lettered side-effect jobs.
///
/// A dead job is retained as evidence for `dead_letter_after`; this schedule
/// removes older ones every `cadence`. It is `Observed` rather than
/// tenant-degrading: a missed reap costs disk, not correctness.
pub(super) fn register_entity_side_effect_reap(
    supervisor: &TaskSupervisor,
    drain: Arc<EntitySideEffectDrain>,
    cadence: Duration,
    dead_letter_after: chrono::Duration,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("entity-side-effect-dead-letter-reap").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let drain = Arc::clone(&drain); // clone: each reap execution owns the policy operation while executing.
        async move {
            let now = DateTime::<Utc>::from(execution.cutoff());
            let purged = drain.purge_dead(now - dead_letter_after).await.map_err(classify_entity_expiry_failure)?;
            tracing::debug!(purged, "pending side-effect dead-letter reap completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

pub(super) fn register_spatial_synchronization(
    supervisor: &TaskSupervisor,
    changes: broadcast::Sender<ChangeEvent>,
    synchronizer: Arc<SpatialSynchronizer>,
) -> Result<(), TenantAssemblyError> {
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("spatial-synchronization").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, restart_delay, restart_delay),
        ShutdownPolicy::new(restart_delay),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let mut receiver = changes.subscribe();
        let synchronizer = Arc::clone(&synchronizer); // clone: one resident attempt owns the operation while running.
        async move {
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = receiver.recv() => {
                        match received {
                            Ok(event) => synchronizer.apply(event).await,
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(skipped, "spatial synchronization lagged; reconciliation will repair drift");
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                return Err(TaskFailure::invariant_violation("entity change channel closed while spatial synchronization was active"));
                            }
                        }
                    }
                }
            }
        }
    });
    supervisor
        .register_resident(task)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

pub(super) fn register_spatial_reconciliation(
    supervisor: &TaskSupervisor,
    index: Arc<SpatialEntityIndex>,
    source: Arc<dyn EntitySpatialSnapshotSource>,
    tenant: TenantRef,
    cadence: Duration,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("spatial-reconciliation").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let index = Arc::clone(&index); // clone: each bounded pass owns index and source handles while running.
        let source = Arc::clone(&source);
        let tenant = tenant.clone();
        async move {
            let outcome = reconcile_location(&index, source.as_ref(), &tenant)
                .await
                .map_err(classify_entity_expiry_failure)?;
            tracing::info!(
                partitions = outcome.partition_count,
                entries = outcome.total_entries,
                "spatial reconciliation completed"
            );
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

pub(super) fn classify_subscription_expiry_failure(error: SubscriptionError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        SubscriptionError::Persistence { .. } => TaskFailure::transient(summary),
        SubscriptionError::Internal { .. } => TaskFailure::invariant_violation(summary),
        SubscriptionError::Validation { .. }
        | SubscriptionError::NotFound { .. }
        | SubscriptionError::AlreadyExists { .. }
        | SubscriptionError::JsonLd(_)
        | SubscriptionError::Query(_)
        | SubscriptionError::Representation(_)
        | SubscriptionError::Identifier(_) => TaskFailure::permanent(summary),
    }
}

pub(super) fn classify_registration_expiry_failure(error: RegistrationError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        RegistrationError::Persistence { .. } => TaskFailure::transient(summary),
        RegistrationError::Internal { .. } => TaskFailure::invariant_violation(summary),
        RegistrationError::Validation { .. }
        | RegistrationError::NotFound { .. }
        | RegistrationError::AlreadyExists { .. }
        | RegistrationError::Conflict { .. }
        | RegistrationError::JsonLd(_)
        | RegistrationError::Query(_)
        | RegistrationError::Representation(_)
        | RegistrationError::Identifier(_) => TaskFailure::permanent(summary),
    }
}

/// Classify exhaustive entity-policy failures at the binary composition boundary.
pub(super) fn classify_entity_expiry_failure(error: EntityError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        EntityError::Commit(_) | EntityError::Persistence { detail: _ } | EntityError::Contention { identifier: _, attempts: _ } => {
            TaskFailure::transient(summary)
        }
        EntityError::Internal { detail: _ } => TaskFailure::invariant_violation(summary),
        EntityError::Validation { detail: _ }
        | EntityError::NotFound { identifier: _ }
        | EntityError::TypeNotFound { name: _ }
        | EntityError::AttributeNotFound { name: _ }
        | EntityError::MemberNotFound { detail: _ }
        | EntityError::AlreadyExists { identifier: _ }
        | EntityError::OperationNotSupported { detail: _ }
        | EntityError::JsonLd(_)
        | EntityError::Query(_)
        | EntityError::Request(_)
        | EntityError::Representation(_)
        | EntityError::Identifier(_)
        | EntityError::EntityTypeListIdentifier(_)
        | EntityError::AttributeListIdentifier(_) => TaskFailure::permanent(summary),
    }
}
