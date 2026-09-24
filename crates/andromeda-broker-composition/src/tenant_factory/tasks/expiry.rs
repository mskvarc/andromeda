//! Entity, subscription, registration, Snapshot and Entity Map `expiresAt`
//! sweeps, and the clause 5.13.1 cached-context invalidation pass.

use crate::{
    cached_context_invalidation::{CACHED_CONTEXT_INVALIDATION_INTERVAL, invalidate_once},
    event::registration_change_sink::BroadcastRegistrationChangeSink,
    tenant_factory::{
        TenantFactoryConfig,
        error::TenantAssemblyError,
        storage::TenantStorage,
        tasks::{
            maintenance::{classify_entity_expiry_failure, classify_registration_expiry_failure, classify_subscription_expiry_failure},
            snapshot::register_snapshot_expiry,
        },
    },
    tenant_state::AndromedaState,
};
use andromeda_context_registry::cached_context_invalidation::CachedContextInvalidation;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    EntityService,
    expiry::{EntityExpiryBatchSize, EntityExpiryOutcome, EntityExpirySweep},
    port::expiry::EntityExpirySource,
};
use andromeda_entity_map::{error::EntityMapError, expiry::EntityMapExpiry, store::EntityMapStore};
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_registration::{RegistrationChangeSink, expiry::RegistrationExpiry, store::RegistrationStore};
use andromeda_subscription::{expiry::SubscriptionExpiry, lifecycle::SubscriptionService, variant::SubscriptionVariant};
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
use tokio::sync::broadcast;

/// Register every sweep that deletes a resource past its `expiresAt`.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the configured expiry batch size or
/// check interval is rejected, or when the supervisor refuses a registration.
pub(super) fn register_expiry_workers(
    supervisor: &TaskSupervisor,
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    state: &AndromedaState,
    registration_changes: &broadcast::Sender<CsrChangeEvent>,
) -> Result<(), TenantAssemblyError> {
    let interval = config.temporal.expiration_check_interval.duration();
    register_subscription_expiry(
        supervisor,
        "subscription-expiry",
        state.live_target.clone(),        // clone: the sweep owns the live branch it operates on.
        Arc::clone(&state.subscriptions), // clone: request handling and the sweep share one policy.
        interval,
    )?;
    register_subscription_expiry(
        supervisor,
        "csource-subscription-expiry",
        state.live_target.clone(),            // clone: the sweep owns the live branch it operates on.
        Arc::clone(&state.csr_subscriptions), // clone: request handling and the sweep share one policy.
        interval,
    )?;
    register_registration_expiry(
        supervisor,
        Arc::clone(&storage.registration_store), // clone: every registration worker shares one store.
        Arc::new(BroadcastRegistrationChangeSink::new(registration_changes.clone())), // clone: each worker owns a sender handle.
        interval,
    )?;
    register_entity_expiry(
        supervisor,
        state.live_target.clone(),   // clone: the sweep owns the live branch it operates on.
        Arc::clone(&state.entities), // clone: request handling and the sweep share one policy.
        Arc::clone(&storage.entity_ports.expiry),
        interval,
        config.entity.expiry_batch_size,
    )?;
    register_snapshot_expiry(supervisor, Arc::clone(&storage.snapshot_store), interval)?; // clone: every snapshot worker shares one store.
    register_entity_map_expiry(supervisor, Arc::clone(&storage.entity_map_store), interval)?; // clone: request handling and the sweep share one store.
    register_cached_context_invalidation(supervisor, Arc::clone(&storage.cached_context_invalidation)) // clone: the sweep and every other Context capability share one repository.
}

/// Register the clause 5.13.1 pass that reclaims lapsed cached `@contexts`.
///
/// It runs immediately at start-up and then every
/// [`CACHED_CONTEXT_INVALIDATION_INTERVAL`], on its own clock, so a broker that
/// never writes another cache row still invalidates the rows it holds. The pass
/// logs and swallows a failure rather than escalating: the rows it would reclaim
/// are already refused by every read.
fn register_cached_context_invalidation(supervisor: &TaskSupervisor, store: Arc<dyn CachedContextInvalidation>) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(CACHED_CONTEXT_INVALIDATION_INTERVAL, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("cached-context-invalidation").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, CACHED_CONTEXT_INVALIDATION_INTERVAL, CACHED_CONTEXT_INVALIDATION_INTERVAL),
        ShutdownPolicy::new(CACHED_CONTEXT_INVALIDATION_INTERVAL),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let store = Arc::clone(&store); // clone: each scheduled pass owns the shared invalidation port while running.
        async move {
            invalidate_once(store.as_ref()).await;
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

fn register_entity_expiry(
    supervisor: &TaskSupervisor,
    target: ContextTarget,
    entity_policy: Arc<EntityService>,
    entity_expiry_source: Arc<dyn EntityExpirySource>,
    cadence: Duration,
    expiry_batch_size: usize,
) -> Result<(), TenantAssemblyError> {
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
            match sweep.execute(cutoff).await.map_err(|error| classify_entity_expiry_failure(&error))? {
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
fn register_subscription_expiry<V>(
    supervisor: &TaskSupervisor,
    task_name: &'static str,
    target: ContextTarget,
    policy: Arc<SubscriptionService<V>>,
    cadence: Duration,
) -> Result<(), TenantAssemblyError>
where
    V: SubscriptionVariant + 'static,
{
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
            let outcome = expiry.execute(cutoff).await.map_err(|error| classify_subscription_expiry_failure(&error))?;
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
/// every `cadence` (`ANDROMEDA__TEMPORAL__EXPIRATION_CHECK_INTERVAL`,
/// 30 s by default, 5 s in the conformance stacks), deleting every
/// registration whose `expiresAt` is at or before the sweep's cutoff and
/// publishing the clause 5.11 `NoLongerMatching` change for each. A missed
/// tick is skipped rather than replayed, so the sweep never runs twice in one
/// cadence. Until the sweep reaches an expired row, the registration store's
/// reads already treat it as deleted.
fn register_registration_expiry(
    supervisor: &TaskSupervisor,
    store: Arc<dyn RegistrationStore>,
    change_sink: Arc<dyn RegistrationChangeSink>,
    cadence: Duration,
) -> Result<(), TenantAssemblyError> {
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
            let outcome = expiry.execute(cutoff).await.map_err(|error| classify_registration_expiry_failure(&error))?;
            tracing::info!(expired = outcome.expired.len(), "registration expiry completed");
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
fn register_entity_map_expiry(supervisor: &TaskSupervisor, store: Arc<dyn EntityMapStore>, cadence: Duration) -> Result<(), TenantAssemblyError> {
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
            let expired = expiry.execute(cutoff).await.map_err(|error| classify_entity_map_failure(&error))?;
            tracing::info!(expired, "entity map expiry completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

fn classify_entity_map_failure(error: &EntityMapError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        EntityMapError::Persistence { .. } | EntityMapError::StoreSaturated => TaskFailure::transient(summary),
        EntityMapError::Invariant { .. } => TaskFailure::invariant_violation(summary),
        EntityMapError::NotFound { .. } | EntityMapError::RequestChanged { .. } => TaskFailure::permanent(summary),
    }
}
