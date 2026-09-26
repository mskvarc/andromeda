//! Durable entity side-effect and spatial-maintenance task registration.

use crate::{
    event::entity_change_sink::BroadcastEntityChangeSink,
    tenant_factory::{
        TenantFactoryConfig,
        error::TenantAssemblyError,
        spatial::location_placement,
        storage::TenantStorage,
        tasks::retention::retention_duration,
    },
};
use andromeda_context_commit::{committer::ContextCommitter, context_change_event::ContextChangeEvent, lease_holder::LeaseHolder};
use andromeda_entity::{
    change_sink::EntityChangeSink,
    error::EntityError,
    side_effect_drain::{EntitySideEffectDrain, EntitySideEffectDrainConfig},
    spatial_acceleration::SpatialAcceleration,
    spatial_reconciliation::{RebuildOutcome, rebuild_location_index},
};
use andromeda_postgresql_context_store::pending_side_effects::repo::PendingSideEffectsRepository;
use andromeda_registration::error::RegistrationError;
use andromeda_subscription::error::SubscriptionError;
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    drain_task::{DrainPassOutcome, DrainTask},
    failure::TaskFailure,
    periodic_trigger::{InitialExecution, MissedTickPolicy, PeriodicSchedule},
    registration::TaskRegistration,
    restart_policy::RestartPolicy,
    scheduled_task::ScheduledTask,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver},
};
use chrono::{DateTime, Utc};
use std::{future::Future, sync::Arc, time::Duration};
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

/// Register the periodic rebuild of the tenant's spatial index from the store,
/// under the consistency mode the index was composed for.
///
/// No worker applies entity changes to the index: under `commit-synchronous`
/// the commit path already has, and under `watermark` the query path catches
/// the index up from the store itself. The rebuild is what drops the entries
/// of entities deleted since, and under `commit-synchronous` what brings in the
/// writes of another broker process sharing the store.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the configured rebuild cadence is
/// rejected or when the supervisor refuses the registration.
pub(super) fn register_spatial_workers(supervisor: &TaskSupervisor, config: &TenantFactoryConfig, storage: &TenantStorage) -> Result<(), TenantAssemblyError> {
    let cadence = config.spatial.rebuild_interval.duration();
    match &storage.spatial {
        SpatialAcceleration::StoreVerified => Ok(()),
        SpatialAcceleration::CommitSynchronous(index) => {
            let index = Arc::clone(index); // clone: the rebuild and the commit path share one index.
            let source = Arc::clone(&storage.entity_ports.spatial_snapshots); // clone: the rebuild reads the store the other ports share.
            let placement = Arc::new(location_placement(&storage.tenant)?);
            register_spatial_rebuild(supervisor, cadence, move || {
                // clone: each bounded pass owns the index, source and placement handles while running.
                let (index, source, placement) = (Arc::clone(&index), Arc::clone(&source), Arc::clone(&placement));
                async move { rebuild_location_index(&index, source.as_ref(), &placement).await }
            })
        }
        SpatialAcceleration::Watermark(watermark) => {
            let watermark = Arc::clone(watermark); // clone: the rebuild and the query path share one watermark.
            register_spatial_rebuild(supervisor, cadence, move || {
                let watermark = Arc::clone(&watermark); // clone: each bounded pass owns the watermark while running.
                async move { watermark.rebuild().await }
            })
        }
    }
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

/// Register one periodic spatial rebuild pass at `cadence`.
fn register_spatial_rebuild<Rebuild, Pass>(supervisor: &TaskSupervisor, cadence: Duration, rebuild: Rebuild) -> Result<(), TenantAssemblyError>
where
    Rebuild: Fn() -> Pass + Send + Sync + 'static,
    Pass: Future<Output = Result<RebuildOutcome, EntityError>> + Send + 'static,
{
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("spatial-rebuild").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let pass = rebuild();
        async move {
            let outcome = pass.await.map_err(|error| classify_entity_expiry_failure(&error))?;
            tracing::info!(
                partitions = outcome.partition_count,
                entries = outcome.total_entries,
                "spatial rebuild completed"
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
        EntityError::Commit(_)
        | EntityError::Persistence(_)
        | EntityError::StoreSaturated
        | EntityError::TransactionContended
        | EntityError::Contention { identifier: _, attempts: _ } => TaskFailure::transient(summary),
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
