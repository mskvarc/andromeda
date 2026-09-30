//! Entity, subscription, registration, Snapshot and Entity Map `expiresAt`
//! sweeps, and the clause 5.13.1 cached-context invalidation pass.

use crate::{
    cached_context_invalidation::{CACHED_CONTEXT_INVALIDATION_INTERVAL, invalidate_once},
    event::registration_change_sink::BroadcastRegistrationChangeSink,
    tenant_factory::{
        TenantFactoryConfig,
        assembly_invariant::AssemblyInvariant,
        background_pacer::BackgroundPacer,
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
use andromeda_registration::{RegistrationChangeSink, expiry::RegistrationExpiry, removal_hook::RegistrationRemovalHook, store::RegistrationStore};
use andromeda_subscription::{expiry::SubscriptionExpiry, lifecycle::SubscriptionService, variant::SubscriptionVariant};
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
use chrono::{DateTime, Utc};
use std::{convert::Infallible, sync::Arc, time::Duration};
use tokio::sync::broadcast;

/// Register every sweep that deletes a resource past its `expiresAt`, each
/// paced by `pacer` so a tenant nobody is using sweeps only when something is
/// due.
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
    pacer: &Arc<BackgroundPacer>,
) -> Result<(), TenantAssemblyError> {
    let interval = config.temporal.expiration_check_interval.duration();
    // clone: every sweep of the tenant consults the tenant's one pacer.
    let shared_pacer = || Arc::clone(pacer);
    register_subscription_expiry(
        supervisor,
        shared_pacer(),
        "subscription-expiry",
        state.live_target.clone(),        // clone: the sweep owns the live branch it operates on.
        Arc::clone(&state.subscriptions), // clone: request handling and the sweep share one policy.
        interval,
    )?;
    register_subscription_expiry(
        supervisor,
        shared_pacer(),
        "csource-subscription-expiry",
        state.live_target.clone(),            // clone: the sweep owns the live branch it operates on.
        Arc::clone(&state.csr_subscriptions), // clone: request handling and the sweep share one policy.
        interval,
    )?;
    register_registration_expiry(
        supervisor,
        shared_pacer(),
        Arc::clone(&storage.registration_store), // clone: every registration worker shares one store.
        Arc::new(BroadcastRegistrationChangeSink::new(registration_changes.clone())), // clone: each worker owns a sender handle.
        // clone: request handling and the sweep tear down removed registrations through one forwarding service.
        state
            .subscription_forwarding
            .as_ref()
            .map(|forwarding| Arc::clone(forwarding) as Arc<dyn RegistrationRemovalHook>),
        interval,
    )?;
    register_entity_expiry(
        supervisor,
        shared_pacer(),
        state.live_target.clone(),   // clone: the sweep owns the live branch it operates on.
        Arc::clone(&state.entities), // clone: request handling and the sweep share one policy.
        Arc::clone(&storage.entity_ports.expiry),
        interval,
        config.entity.expiry_batch_size,
    )?;
    register_snapshot_expiry(supervisor, shared_pacer(), Arc::clone(&storage.snapshot_store), interval)?; // clone: every snapshot worker shares one store.
    register_entity_map_expiry(supervisor, shared_pacer(), Arc::clone(&storage.entity_map_store), interval)?; // clone: request handling and the sweep share one store.
    register_cached_context_invalidation(supervisor, shared_pacer(), Arc::clone(&storage.cached_context_invalidation)) // clone: the sweep and every other Context capability share one repository.
}

/// Register the clause 5.13.1 pass that reclaims lapsed cached `@contexts`.
///
/// It ticks every [`CACHED_CONTEXT_INVALIDATION_INTERVAL`] on its own clock,
/// so a broker that never writes another cache row still invalidates the rows
/// it holds, and a tick runs when the tenant's pacer finds something due: the
/// earliest expiration of a `"Cached"` entry is one of the obligations it
/// reads, so the pass runs on the first tick after an entry lapses. A lapsed
/// row it has not reclaimed yet is never served in the meantime, because every
/// read passes over it; the pass logs and swallows a failure for the same
/// reason.
fn register_cached_context_invalidation(
    supervisor: &TaskSupervisor,
    pacer: Arc<BackgroundPacer>,
    store: Arc<dyn CachedContextInvalidation>,
) -> Result<(), TenantAssemblyError> {
    let schedule =
        PeriodicSchedule::new(CACHED_CONTEXT_INVALIDATION_INTERVAL, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("cached-context-invalidation").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, CACHED_CONTEXT_INVALIDATION_INTERVAL, CACHED_CONTEXT_INVALIDATION_INTERVAL),
        ShutdownPolicy::new(CACHED_CONTEXT_INVALIDATION_INTERVAL),
    );
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let store = Arc::clone(&store); // clone: each scheduled pass owns the shared invalidation port while running.
        let pacer = Arc::clone(&pacer); // clone: each pass consults the tenant's one pacer.
        async move {
            pacer
                .run(async move {
                    invalidate_once(store.as_ref()).await;
                    Ok::<(), TaskFailure<Infallible>>(())
                })
                .await
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}

fn register_entity_expiry(
    supervisor: &TaskSupervisor,
    pacer: Arc<BackgroundPacer>,
    target: ContextTarget,
    entity_policy: Arc<EntityService>,
    entity_expiry_source: Arc<dyn EntityExpirySource>,
    cadence: Duration,
    expiry_batch_size: usize,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("entity-expiry").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry_batch_size = EntityExpiryBatchSize::try_from(expiry_batch_size).map_err(AssemblyInvariant::from)?;
    let sweep = Arc::new(EntityExpirySweep::new(target, entity_policy, entity_expiry_source, expiry_batch_size));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let sweep = Arc::clone(&sweep); // clone: each scheduled future owns the shared policy operation for its duration.
        let pacer = Arc::clone(&pacer); // clone: each scheduled future consults the tenant's one pacer.
        async move {
            pacer
                .run(async move {
                    let cutoff = DateTime::<Utc>::from(execution.cutoff());
                    let EntityExpiryOutcome {
                        expired_entities,
                        purged_attributes,
                    } = sweep.execute(cutoff).await.map_err(classify_entity_expiry_failure)?;
                    tracing::info!(expired_entities, purged_attributes, "entity expiry sweep completed");
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

/// Register one subscription variant as an independent scheduled policy operation.
fn register_subscription_expiry<V>(
    supervisor: &TaskSupervisor,
    pacer: Arc<BackgroundPacer>,
    task_name: &'static str,
    target: ContextTarget,
    policy: Arc<SubscriptionService<V>>,
    cadence: Duration,
) -> Result<(), TenantAssemblyError>
where
    V: SubscriptionVariant + 'static,
{
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from(task_name).map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(SubscriptionExpiry::new(target, policy));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let expiry = Arc::clone(&expiry); // clone: each scheduled future owns the policy operation while executing.
        let pacer = Arc::clone(&pacer); // clone: each scheduled future consults the tenant's one pacer.
        async move {
            pacer
                .run(async move {
                    let cutoff = DateTime::<Utc>::from(execution.cutoff());
                    let outcome = expiry.execute(cutoff).await.map_err(classify_subscription_expiry_failure)?;
                    tracing::info!(expired = outcome.expired.len(), task = task_name, "subscription expiry completed");
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
    pacer: Arc<BackgroundPacer>,
    store: Arc<dyn RegistrationStore>,
    change_sink: Arc<dyn RegistrationChangeSink>,
    removal_hook: Option<Arc<dyn RegistrationRemovalHook>>,
    cadence: Duration,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("registration-expiry").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(RegistrationExpiry::new(store, Some(change_sink), removal_hook));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let expiry = Arc::clone(&expiry); // clone: each scheduled future owns the policy operation while executing.
        let pacer = Arc::clone(&pacer); // clone: each scheduled future consults the tenant's one pacer.
        async move {
            pacer
                .run(async move {
                    let cutoff = DateTime::<Utc>::from(execution.cutoff());
                    let outcome = expiry.execute(cutoff).await.map_err(classify_registration_expiry_failure)?;
                    tracing::info!(expired = outcome.expired.len(), "registration expiry completed");
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

/// Register the sweep that removes Entity Maps past their `expiresAt`
/// (clause 5.2.39); the maps are durable, so a stale one would otherwise stay
/// referenceable across instances forever.
fn register_entity_map_expiry(
    supervisor: &TaskSupervisor,
    pacer: Arc<BackgroundPacer>,
    store: Arc<dyn EntityMapStore>,
    cadence: Duration,
) -> Result<(), TenantAssemblyError> {
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(AssemblyInvariant::from)?;
    let name = TaskName::try_from("entity-map-expiry").map_err(AssemblyInvariant::from)?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(EntityMapExpiry::new(store));
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let expiry = Arc::clone(&expiry); // clone: each execution owns the bounded operation while running.
        let pacer = Arc::clone(&pacer); // clone: each execution consults the tenant's one pacer.
        async move {
            pacer
                .run(async move {
                    let cutoff = DateTime::<Utc>::from(execution.cutoff());
                    let expired = expiry.execute(cutoff).await.map_err(classify_entity_map_failure)?;
                    tracing::info!(expired, "entity map expiry completed");
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

const fn classify_entity_map_failure(error: EntityMapError) -> TaskFailure<EntityMapError> {
    let class = match &error {
        EntityMapError::Persistence(_) | EntityMapError::StoreSaturated => FailureClass::Transient,
        EntityMapError::Invariant(_) => FailureClass::InvariantViolation,
        EntityMapError::NotFound { .. } | EntityMapError::RequestChanged { .. } => FailureClass::Permanent,
    };
    TaskFailure::new(class, error)
}
