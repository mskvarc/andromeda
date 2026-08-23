//! Entity, subscription, registration, and snapshot lifecycle task registration.

use crate::tenant_factory::{
    error::TenantAssemblyError,
    tasks::maintenance::{classify_entity_expiry_failure, classify_registration_expiry_failure, classify_subscription_expiry_failure},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    EntityService,
    expiry::{EntityExpiryBatchSize, EntityExpiryOutcome, EntityExpirySweep},
    port::expiry::EntityExpirySource,
};
use andromeda_entity_map::{error::EntityMapError, expiry::EntityMapExpiry, store::EntityMapStore};
use andromeda_registration::{RegistrationChangeSink, expiry::RegistrationExpiry, store::RegistrationStore};
use andromeda_snapshot::{
    error::SnapshotError,
    executor::{SnapshotPreparation, SnapshotPreparationOutcome},
    expiry::SnapshotExpiry,
    store::SnapshotStore,
};
use andromeda_subscription::{
    expiry::SubscriptionExpiry,
    lifecycle::{SubscriptionService, SubscriptionVariant},
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
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};

pub(super) fn register_entity_expiry(
    supervisor: &TaskSupervisor,
    target: ContextTarget,
    entity_policy: Arc<EntityService>,
    entity_expiry_source: Arc<dyn EntityExpirySource>,
    expiration_interval: u64,
    expiry_batch_size: usize,
) -> Result<(), TenantAssemblyError> {
    let cadence = Duration::from_secs(expiration_interval);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("entity-expiry").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry_batch_size = EntityExpiryBatchSize::try_from(expiry_batch_size).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let sweep = Arc::new(EntityExpirySweep::new(target, entity_policy, entity_expiry_source, expiry_batch_size));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let sweep = Arc::clone(&sweep); // clone: each scheduled future owns the shared policy operation for its duration.
        async move {
            let cutoff = DateTime::<Utc>::from(execution.cutoff());
            match sweep.execute(cutoff).await.map_err(classify_entity_expiry_failure)? {
                EntityExpiryOutcome::DeferredForBatchActivity => tracing::debug!("entity expiry deferred during recent batch activity"),
                EntityExpiryOutcome::Applied {
                    expired_entities,
                    purged_attributes,
                } => tracing::info!(expired_entities, purged_attributes, "entity expiry sweep completed"),
            }
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Register one subscription variant as an independent scheduled policy operation.
pub(super) fn register_subscription_expiry<V>(
    supervisor: &TaskSupervisor,
    task_name: &'static str,
    target: ContextTarget,
    policy: Arc<SubscriptionService<V>>,
    expiration_interval: u64,
) -> Result<(), TenantAssemblyError>
where
    V: SubscriptionVariant + 'static,
{
    let cadence = Duration::from_secs(expiration_interval);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from(task_name).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(SubscriptionExpiry::new(target, policy));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let expiry = Arc::clone(&expiry); // clone: each scheduled future owns the policy operation while executing.
        async move {
            let cutoff = DateTime::<Utc>::from(execution.cutoff());
            let outcome = expiry.execute(cutoff).await.map_err(classify_subscription_expiry_failure)?;
            tracing::info!(expired = outcome.expired.len(), task = task_name, "subscription expiry completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Register semantic registration deletion as its own scheduled operation.
///
/// Clause 5.9.2.4: a registration with a future `expiresAt` is deleted when
/// that instant is reached. The sweep runs immediately at start-up and then
/// every `expiration_interval` seconds (`ANDROMEDA_TEMPORAL_EXPIRATION_CHECK_INTERVAL_SECS`,
/// 30 s by default, 5 s in the conformance stacks), deleting every
/// registration whose `expiresAt` is at or before the sweep's cutoff and
/// publishing the clause 5.11 `NoLongerMatching` change for each. A missed
/// tick is skipped rather than replayed, so the sweep never runs twice in one
/// cadence. Until the sweep reaches an expired row, the registration store's
/// reads already treat it as deleted.
pub(super) fn register_registration_expiry(
    supervisor: &TaskSupervisor,
    store: Arc<dyn RegistrationStore>,
    change_sink: Arc<dyn RegistrationChangeSink>,
    expiration_interval: u64,
) -> Result<(), TenantAssemblyError> {
    let cadence = Duration::from_secs(expiration_interval);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("registration-expiry").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(RegistrationExpiry::new(store, Some(change_sink)));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let expiry = Arc::clone(&expiry); // clone: each scheduled future owns the policy operation while executing.
        async move {
            let cutoff = DateTime::<Utc>::from(execution.cutoff());
            let outcome = expiry.execute(cutoff).await.map_err(classify_registration_expiry_failure)?;
            tracing::info!(expired = outcome.expired.len(), "registration expiry completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

pub(super) fn register_snapshot_preparation(supervisor: &TaskSupervisor, preparation: Arc<SnapshotPreparation>) -> Result<(), TenantAssemblyError> {
    let cadence = Duration::from_secs(1);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("snapshot-preparation").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let preparation = Arc::clone(&preparation); // clone: each execution owns the bounded operation while running.
        async move {
            match preparation.execute().await.map_err(classify_snapshot_failure)? {
                SnapshotPreparationOutcome::Idle => {}
                SnapshotPreparationOutcome::Prepared { count } => tracing::info!(count, "snapshot preparation completed"),
            }
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

pub(super) fn register_snapshot_expiry(
    supervisor: &TaskSupervisor,
    store: Arc<dyn SnapshotStore>,
    expiration_interval: u64,
) -> Result<(), TenantAssemblyError> {
    let cadence = Duration::from_secs(expiration_interval);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("snapshot-expiry").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(SnapshotExpiry::new(store));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let expiry = Arc::clone(&expiry); // clone: each execution owns the bounded operation while running.
        async move {
            let cutoff = DateTime::<Utc>::from(execution.cutoff());
            let expired = expiry.execute(cutoff).await.map_err(classify_snapshot_failure)?;
            tracing::info!(expired, "snapshot expiry completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Register the sweep that removes Entity Maps past their `expiresAt`
/// (clause 5.2.39); the maps are durable, so a stale one would otherwise stay
/// referenceable across instances forever.
pub(super) fn register_entity_map_expiry(
    supervisor: &TaskSupervisor,
    store: Arc<dyn EntityMapStore>,
    expiration_interval: u64,
) -> Result<(), TenantAssemblyError> {
    let cadence = Duration::from_secs(expiration_interval);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("entity-map-expiry").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(EntityMapExpiry::new(store));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let expiry = Arc::clone(&expiry); // clone: each execution owns the bounded operation while running.
        async move {
            let cutoff = DateTime::<Utc>::from(execution.cutoff());
            let expired = expiry.execute(cutoff).await.map_err(classify_entity_map_failure)?;
            tracing::info!(expired, "entity map expiry completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

fn classify_entity_map_failure(error: EntityMapError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        EntityMapError::Persistence { .. } => TaskFailure::transient(summary),
        EntityMapError::Invariant { .. } => TaskFailure::invariant_violation(summary),
        EntityMapError::NotFound { .. } | EntityMapError::RequestChanged { .. } => TaskFailure::permanent(summary),
    }
}

fn classify_snapshot_failure(error: SnapshotError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        SnapshotError::Persistence { .. } => TaskFailure::transient(summary),
        SnapshotError::Internal { .. } => TaskFailure::invariant_violation(summary),
        SnapshotError::Validation { .. }
        | SnapshotError::NotFound { .. }
        | SnapshotError::AlreadyExists { .. }
        | SnapshotError::PreparationTimeout { .. }
        | SnapshotError::Entity(_)
        | SnapshotError::Temporal(_)
        | SnapshotError::Federation(_)
        | SnapshotError::Request(_)
        | SnapshotError::Representation(_) => TaskFailure::permanent(summary),
    }
}
