//! Durable entity side-effect and spatial-maintenance task registration.

use crate::{
    event::entity_change_sink::BroadcastEntityChangeSink,
    tenant_factory::{TenantFactoryConfig, error::TenantAssemblyError, storage::TenantStorage, tasks::retention::retention_duration},
};
use andromeda_context_commit::{committer::ContextCommitter, context_change_event::ContextChangeEvent, lease_holder::LeaseHolder};
use andromeda_entity::{
    change_sink::EntityChangeSink,
    error::EntityError,
    port::spatial_snapshot::EntitySpatialSnapshotSource,
    side_effect_drain::{EntitySideEffectDrain, EntitySideEffectDrainConfig},
    spatial_reconciliation::reconcile_location,
    spatial_synchronization::SpatialSynchronizer,
};
use andromeda_postgresql_context_store::pending_side_effects::repo::PendingSideEffectsRepository;
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
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver},
};
use andromeda_tenancy::tenant_ref::TenantRef;
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;

/// Compose the tenant's durable side-effect drain and register the workers
/// that service it.
///
/// The drain always runs: `Prefer: respond-async` enqueues a durable job
/// whenever the request opts in, with or without a configured batch-size
/// threshold, so an unconditional drain is what keeps every enqueued job from
/// waiting forever.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the configured dead-letter age is out
/// of range or when the supervisor refuses either registration.
pub(super) fn register_entity_side_effects(
    supervisor: &TaskSupervisor,
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    writes: &Arc<dyn ContextCommitter>,
    changes: &broadcast::Sender<ContextChangeEvent>,
    wake: DrainWakeReceiver,
) -> Result<(), TenantAssemblyError> {
    let entity_config = &config.entity;
    let change_sink = Arc::new(BroadcastEntityChangeSink::new(changes.clone())) as Arc<dyn EntityChangeSink>; // clone: services and the drain each own a sender handle.
    let drain = Arc::new(EntitySideEffectDrain::new(
        Arc::new(PendingSideEffectsRepository::new(&storage.pool)),
        Arc::clone(writes), // clone: request handling and the drain share one committer.
        Some(change_sink),
        EntitySideEffectDrainConfig {
            lease_seconds: i32::try_from(entity_config.pending_side_effects_lease.duration().as_secs()).unwrap_or(i32::MAX),
            batch_size: i32::try_from(entity_config.pending_side_effects_batch_size).unwrap_or(i32::MAX),
            max_attempts: entity_config.pending_side_effects_max_attempts,
            base_backoff: entity_config.pending_side_effects_base_backoff.duration(),
            lease_holder: LeaseHolder::mint(),
        },
    ));
    register_entity_side_effect_drain(
        supervisor,
        Arc::clone(&drain), // clone: the delivery and reap workers share one drain operation.
        entity_config.pending_side_effects_poll.duration(),
        entity_config.pending_side_effects_poll_max.duration(),
        entity_config.pending_side_effects_lease.duration(),
        wake,
    )?;
    register_entity_side_effect_reap(
        supervisor,
        drain,
        entity_config.pending_side_effects_cleanup_interval.duration(),
        retention_duration(
            entity_config.pending_side_effects_dead_letter_after.duration().as_secs(),
            "pending side-effect dead-letter age",
        )?,
    )?;
    Ok(())
}

/// Register the spatial index workers: the resident synchronizer that applies
/// entity changes and the periodic reconciliation that repairs any drift.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the configured rebuild cadence is
/// rejected or when the supervisor refuses either registration.
pub(super) fn register_spatial_workers(
    supervisor: &TaskSupervisor,
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    changes: broadcast::Sender<ContextChangeEvent>,
) -> Result<(), TenantAssemblyError> {
    let synchronizer = Arc::new(SpatialSynchronizer::new(
        Arc::clone(&storage.spatial_index), // clone: synchronization and reconciliation share one index.
        Arc::clone(&storage.entity_ports.reader),
        storage.tenant.clone(), // clone: the synchronizer owns its tenant partition.
    ));
    register_spatial_synchronization(supervisor, changes, synchronizer)?;
    register_spatial_reconciliation(
        supervisor,
        Arc::clone(&storage.spatial_index), // clone: synchronization and reconciliation share one index.
        Arc::clone(&storage.entity_ports.spatial_snapshots),
        storage.tenant.clone(), // clone: the reconciliation pass owns its tenant partition.
        config.spatial.rebuild_interval.duration(),
    )
}

fn register_entity_side_effect_drain(
    supervisor: &TaskSupervisor,
    drain: Arc<EntitySideEffectDrain>,
    fallback_floor: Duration,
    fallback_ceiling: Duration,
    lease_duration: Duration,
    wake: DrainWakeReceiver,
) -> Result<(), TenantAssemblyError> {
    let fallback = DrainFallbackPolicy::new(fallback_floor, fallback_ceiling).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("entity-side-effect-delivery").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback_floor, fallback_ceiling),
        ShutdownPolicy::new(lease_duration),
    );
    let task = DrainTask::new(registration, fallback, move |execution| {
        let drain = Arc::clone(&drain); // clone: each bounded future owns the policy operation while executing.
        async move {
            let now = DateTime::<Utc>::from(execution.cutoff());
            let outcome = drain.execute(now).await.map_err(|error| classify_entity_expiry_failure(&error))?;
            if outcome.work_may_remain {
                Ok(DrainPassOutcome::WorkMayRemain)
            } else {
                Ok(DrainPassOutcome::Idle)
            }
        }
    });
    supervisor
        .register_drain(task, wake)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Register the periodic reap of dead-lettered side-effect jobs.
///
/// A dead job is retained as evidence for `dead_letter_after`; this schedule
/// removes older ones every `cadence`. It is `Observed` rather than
/// tenant-degrading: a missed reap costs disk, not correctness.
fn register_entity_side_effect_reap(
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
            let purged = drain
                .purge_dead(now - dead_letter_after)
                .await
                .map_err(|error| classify_entity_expiry_failure(&error))?;
            tracing::debug!(purged, "pending side-effect dead-letter reap completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

fn register_spatial_synchronization(
    supervisor: &TaskSupervisor,
    changes: broadcast::Sender<ContextChangeEvent>,
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
                    delivery = receiver.recv() => {
                        match delivery {
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

fn register_spatial_reconciliation(
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
                .map_err(|error| classify_entity_expiry_failure(&error))?;
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

pub(super) fn classify_subscription_expiry_failure(error: &SubscriptionError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        SubscriptionError::Persistence(_) | SubscriptionError::StoreSaturated => TaskFailure::transient(summary),
        SubscriptionError::Internal(_) => TaskFailure::invariant_violation(summary),
        SubscriptionError::Validation(_)
        | SubscriptionError::NotFound { .. }
        | SubscriptionError::AlreadyExists { .. }
        | SubscriptionError::JsonLd(_)
        | SubscriptionError::Query(_)
        | SubscriptionError::Representation(_)
        | SubscriptionError::Identifier(_) => TaskFailure::permanent(summary),
    }
}

pub(super) fn classify_registration_expiry_failure(error: &RegistrationError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        RegistrationError::Persistence(_) | RegistrationError::StoreSaturated => TaskFailure::transient(summary),
        RegistrationError::Internal(_) => TaskFailure::invariant_violation(summary),
        RegistrationError::Validation(_)
        | RegistrationError::NotFound { .. }
        | RegistrationError::AlreadyExists { .. }
        | RegistrationError::Conflict(_)
        | RegistrationError::JsonLd(_)
        | RegistrationError::Query(_)
        | RegistrationError::Representation(_)
        | RegistrationError::Identifier(_) => TaskFailure::permanent(summary),
    }
}

/// Classify exhaustive entity-policy failures at the binary composition boundary.
pub(super) fn classify_entity_expiry_failure(error: &EntityError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        EntityError::Commit(_) | EntityError::Persistence(_) | EntityError::StoreSaturated | EntityError::Contention { identifier: _, attempts: _ } => {
            TaskFailure::transient(summary)
        }
        EntityError::Internal(_) => TaskFailure::invariant_violation(summary),
        EntityError::Validation(_)
        | EntityError::NotFound { identifier: _ }
        | EntityError::TypeNotFound { name: _ }
        | EntityError::AttributeNotFound { name: _ }
        | EntityError::MemberNotFound(_)
        | EntityError::AlreadyExists { identifier: _ }
        | EntityError::OperationNotSupported(_)
        | EntityError::JsonLd(_)
        | EntityError::Query(_)
        | EntityError::Request(_)
        | EntityError::Representation(_)
        | EntityError::Identifier(_)
        | EntityError::EntityTypeListIdentifier(_)
        | EntityError::AttributeListIdentifier(_) => TaskFailure::permanent(summary),
    }
}
