//! Construction and supervision of one tenant's background workers.

use crate::{
    config::{outbox::OutboxConfig, runtime::RuntimeConfig},
    error::AndromedaError,
    event::registration_change_sink::BroadcastRegistrationChangeSink,
};
use andromeda_context_commit::{change_event::ChangeEvent, committer::ContextCommitter};
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_entity::{
    EntityService,
    change_sink::EntityChangeSink,
    error::EntityError,
    expiry::{EntityExpiryBatchSize, EntityExpiryOutcome, EntityExpirySweep},
    port::{
        discovery::EntityDiscoveryStore,
        expiry::EntityExpirySource,
        query::EntityQueryStore,
        reader::EntityReader,
        spatial_snapshot::EntitySpatialSnapshotSource,
    },
    query::EntityQueryService,
    side_effect_drain::{EntitySideEffectDrain, EntitySideEffectDrainConfig},
    side_effect_queue::SideEffectLeaseOwner,
    spatial_reconciliation::reconcile_location,
    spatial_synchronization::SpatialSynchronizer,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_notification::{
    csource_interval::CsourceIntervalNotificationOperation,
    csource_processor::CsourceNotificationProcessor,
    error::NotificationError,
    interval::IntervalNotificationOperation,
    outbox_drain::{NotificationOutboxDelivery, NotificationOutboxDrain},
    outbox_store::OutboxStore,
    processor::{NotificationProcessor, NotificationProcessorDeps},
};
use andromeda_postgresql_context_store::{
    attribute_current::materializer::AttributeCurrentMaterializer,
    csource_subscription::repo::CsourceSubscriptionRepository,
    outbox::repo::OutboxRepository,
    pending_side_effects::repo::PendingSideEffectsRepository,
    subscription::repo::SubscriptionRepository,
};
use andromeda_registration::{RegistrationChangeSink, error::RegistrationError, expiry::RegistrationExpiry, store::RegistrationStore};
use andromeda_snapshot::{
    error::SnapshotError,
    executor::{SnapshotPreparation, SnapshotPreparationOutcome},
    expiry::SnapshotExpiry,
    store::SnapshotStore,
};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change::{CsourceSubscriptionChange, SubscriptionChange},
    csource::CsrSubscriptionVariant,
    entity::EntitySubscriptionVariant,
    error::SubscriptionError,
    expiry::SubscriptionExpiry,
    lifecycle::{SubscriptionService, SubscriptionVariant},
    store::SubscriptionStore,
};
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
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{
        broadcast,
        broadcast::error::{RecvError, TryRecvError},
        watch,
    },
    task::JoinHandle,
};
use typed_builder::TypedBuilder;

/// Change channels shared by tenant services and their workers.
pub(super) struct TenantSignals {
    pub(super) entity: broadcast::Sender<ChangeEvent>,
    pub(super) subscription: broadcast::Sender<SubscriptionChange>,
    pub(super) registration: broadcast::Sender<CsrChangeEvent>,
    pub(super) csource_subscription: broadcast::Sender<CsourceSubscriptionChange>,
}

impl TenantSignals {
    /// Open bounded change channels for a tenant.
    pub(super) fn new(entity_capacity: usize) -> TenantSignals {
        let (entity, _) = broadcast::channel(entity_capacity.max(1));
        let (subscription, _) = broadcast::channel(1024);
        let (registration, _) = broadcast::channel(1024);
        let (csource_subscription, _) = broadcast::channel(1024);
        TenantSignals {
            entity,
            subscription,
            registration,
            csource_subscription,
        }
    }
}

/// Inputs owned or shared by the tenant's worker set.
#[derive(TypedBuilder)]
pub(super) struct WorkerSupervisionInputs<'a> {
    pub(super) config: &'a RuntimeConfig,
    pub(super) pool: &'a PgPool,
    pub(super) tenant: TenantRef,
    pub(super) signals: TenantSignals,
    pub(super) materializer: AttributeCurrentMaterializer,
    pub(super) entity_policy: Arc<EntityService>,
    pub(super) subscription_policy: Arc<SubscriptionService<EntitySubscriptionVariant>>,
    pub(super) csource_subscription_policy: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    pub(super) entity_reader: Arc<dyn EntityReader>,
    pub(super) entity_query_store: Arc<dyn EntityQueryStore>,
    pub(super) entity_discovery_store: Arc<dyn EntityDiscoveryStore>,
    pub(super) entity_expiry_source: Arc<dyn EntityExpirySource>,
    pub(super) entity_spatial_snapshots: Arc<dyn EntitySpatialSnapshotSource>,
    pub(super) registration_store: Arc<dyn RegistrationStore>,
    pub(super) snapshot_store: Arc<dyn SnapshotStore>,
    pub(super) snapshot_preparation: Arc<SnapshotPreparation>,
    pub(super) subscription_cache: Arc<SubscriptionCache>,
    pub(super) writes: Arc<dyn ContextCommitter>,
    pub(super) engine: JsonLdEngine,
    pub(super) spatial_index: Arc<SpatialEntityIndex>,
    pub(super) transport: Arc<TransportDispatcher>,
    pub(super) task_supervisor: TaskSupervisor,
}

/// Handles needed to stop the complete tenant worker set.
pub(super) struct WorkerSupervision {
    pub(super) handles: Vec<JoinHandle<()>>,
    pub(super) health_shutdown: Option<watch::Sender<bool>>,
    pub(super) task_supervisor: TaskSupervisor,
    pub(super) drain_wakes: Vec<WakeHandle>,
}

#[derive(Clone, Copy)]
enum EntityNotificationDelivery {
    Live,
    Durable,
}

/// Spawn every background worker owned by a tenant.
pub(super) fn supervise_workers(inputs: WorkerSupervisionInputs<'_>) -> Result<WorkerSupervision, AndromedaError> {
    let WorkerSupervisionInputs {
        config,
        pool,
        tenant,
        signals,
        materializer,
        entity_policy,
        subscription_policy,
        csource_subscription_policy,
        entity_reader,
        entity_query_store,
        entity_discovery_store,
        entity_expiry_source,
        entity_spatial_snapshots,
        registration_store,
        snapshot_store,
        snapshot_preparation,
        subscription_cache,
        writes,
        engine,
        spatial_index,
        transport,
        task_supervisor,
    } = inputs;
    let TenantSignals {
        entity,
        subscription,
        registration,
        csource_subscription,
    } = signals;
    let mut handles = vec![tokio::spawn(materializer.run())];
    let mut drain_wakes = Vec::new();

    if config.performance.batch_async_side_effect_threshold.is_some() {
        let lease_seconds = i32::try_from(config.performance.pending_side_effects_lease_secs.max(1)).unwrap_or(i32::MAX);
        let batch_size = i32::try_from(config.performance.pending_side_effects_batch_size.max(1)).unwrap_or(i32::MAX);
        let change_sink = Arc::new(crate::event::entity_change_sink::BroadcastEntityChangeSink::new(entity.clone())) as Arc<dyn EntityChangeSink>;
        let drain = EntitySideEffectDrain::new(
            Arc::new(PendingSideEffectsRepository::new(pool)),
            Arc::clone(&writes),
            Some(change_sink),
            EntitySideEffectDrainConfig {
                lease_seconds,
                batch_size,
                max_attempts: config.performance.pending_side_effects_max_attempts.max(1),
                base_backoff: Duration::from_secs(config.performance.pending_side_effects_base_backoff_secs.max(1)),
                lease_owner: SideEffectLeaseOwner::for_tenant(&config.info.source_id, uuid::Uuid::new_v4()),
            },
        );
        drain_wakes.push(register_entity_side_effect_drain(
            &task_supervisor,
            Arc::new(drain),
            Duration::from_secs(config.performance.pending_side_effects_poll_secs.max(1)),
            Duration::from_secs(config.performance.pending_side_effects_lease_secs.max(1)),
        )?);
    }

    let health_shutdown = if tenant.is_unnamed_default() {
        let (shutdown, receiver) = watch::channel(false);
        handles.push(transport.mqtt_pool().spawn_health_probe(receiver));
        Some(shutdown)
    } else {
        None
    };
    let notification_query = Arc::new(EntityQueryService::new(
        Arc::clone(&entity_reader),
        Arc::clone(&entity_query_store),
        engine.clone(),
        Arc::clone(&spatial_index),
        tenant.clone(), // clone: the query service owns its tenant partition.
    ));
    let entity_subscription_store = Arc::new(SubscriptionRepository::new(pool)) as Arc<dyn SubscriptionStore>;
    let csource_subscription_store = Arc::new(CsourceSubscriptionRepository::new(pool)) as Arc<dyn SubscriptionStore>;
    let notification_processor = Arc::new(NotificationProcessor::new(NotificationProcessorDeps {
        sub_repo: Arc::clone(&entity_subscription_store),
        entity_reader: Arc::clone(&entity_reader),
        entity_query: notification_query,
        engine: engine.clone(),
        transport: Arc::clone(&transport),
        tenant: tenant.clone(), // clone: notification policy owns its tenant partition.
        subscription_cache: Arc::clone(&subscription_cache),
    }));
    register_notification_processor(
        &task_supervisor,
        entity.clone(),
        subscription,
        Arc::clone(&notification_processor),
        if config.outbox.enabled {
            EntityNotificationDelivery::Durable
        } else {
            EntityNotificationDelivery::Live
        },
    )?;
    if config.outbox.enabled {
        let store = Arc::new(OutboxRepository::new(pool)) as Arc<dyn OutboxStore>;
        let delivery = notification_processor as Arc<dyn NotificationOutboxDelivery>;
        let policy_config = config.outbox.policy();
        let drain = NotificationOutboxDrain::new(store, delivery, &policy_config).map_err(|error| AndromedaError::internal(error.to_string()))?;
        drain_wakes.push(register_notification_outbox(&task_supervisor, Arc::new(drain), &config.outbox)?);
    }
    let expiration_interval = config.temporal.expiration_check_interval_secs;
    register_subscription_expiry(&task_supervisor, "subscription-expiry", subscription_policy, expiration_interval)?;
    register_subscription_expiry(
        &task_supervisor,
        "csource-subscription-expiry",
        csource_subscription_policy,
        expiration_interval,
    )?;
    register_registration_expiry(
        &task_supervisor,
        Arc::clone(&registration_store),
        Arc::new(BroadcastRegistrationChangeSink::new(registration.clone())),
        expiration_interval,
    )?;
    register_interval_notification(
        &task_supervisor,
        Arc::new(IntervalNotificationOperation::new(
            entity_subscription_store,
            Arc::clone(&entity_reader),
            Arc::clone(&entity_discovery_store),
            engine,
            Arc::clone(&transport),
            tenant.clone(), // clone: interval policy owns its tenant partition.
        )),
    )?;
    register_csource_notification_processor(
        &task_supervisor,
        registration,
        csource_subscription,
        csource_subscription_store.clone(),
        Arc::clone(&registration_store),
        Arc::clone(&transport),
        tenant.clone(),
    )?;
    register_entity_expiry(
        &task_supervisor,
        entity_policy,
        entity_expiry_source,
        expiration_interval,
        config.performance.entity_expiry_batch_size,
    )?;
    register_snapshot_preparation(&task_supervisor, snapshot_preparation)?;
    register_snapshot_expiry(&task_supervisor, snapshot_store, expiration_interval)?;
    register_spatial_synchronization(
        &task_supervisor,
        entity,
        Arc::new(SpatialSynchronizer::new(Arc::clone(&spatial_index), Arc::clone(&entity_reader), tenant.clone())),
    )?;
    if config.spatial.rebuild_interval_secs > 0 {
        register_spatial_reconciliation(
            &task_supervisor,
            spatial_index,
            entity_spatial_snapshots,
            tenant.clone(),
            Duration::from_secs(config.spatial.rebuild_interval_secs),
        )?;
    }
    register_csource_interval_notification(
        &task_supervisor,
        Arc::new(CsourceIntervalNotificationOperation::new(
            csource_subscription_store,
            registration_store,
            transport,
            tenant,
        )),
    )?;
    Ok(WorkerSupervision {
        handles,
        health_shutdown,
        task_supervisor,
        drain_wakes,
    })
}

fn register_notification_processor(
    supervisor: &TaskSupervisor,
    entity_changes: broadcast::Sender<ChangeEvent>,
    subscription_changes: broadcast::Sender<SubscriptionChange>,
    processor: Arc<NotificationProcessor>,
    delivery: EntityNotificationDelivery,
) -> Result<(), AndromedaError> {
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("notification-events").map_err(|error| AndromedaError::internal(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, restart_delay, restart_delay),
        ShutdownPolicy::new(restart_delay),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let mut entity_receiver = entity_changes.subscribe();
        let mut subscription_receiver = subscription_changes.subscribe();
        let processor = Arc::clone(&processor); // clone: every resident attempt owns the shared policy operation.
        async move {
            processor.reload_cache().await.map_err(classify_notification_failure)?;
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = subscription_receiver.recv() => match received {
                        Ok(change) => processor.apply_subscription_change(change).await,
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "notification subscription stream lagged; reloading policy cache");
                            processor.reload_cache().await.map_err(classify_notification_failure)?;
                        }
                        Err(RecvError::Closed) => return Err(TaskFailure::transient("notification subscription channel closed")),
                    },
                    received = entity_receiver.recv(), if matches!(delivery, EntityNotificationDelivery::Live) => match received {
                        Ok(event) => {
                            loop {
                                match subscription_receiver.try_recv() {
                                    Ok(change) => processor.apply_subscription_change(change).await,
                                    Err(TryRecvError::Lagged(skipped)) => {
                                        tracing::debug!(skipped, "notification subscription stream lagged before entity evaluation; reloading policy cache");
                                        processor.reload_cache().await.map_err(classify_notification_failure)?;
                                    }
                                    Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                                }
                            }
                            processor.apply_entity_change(event).await;
                        }
                        Err(RecvError::Lagged(skipped)) => tracing::debug!(skipped, "notification entity stream lagged"),
                        Err(RecvError::Closed) => return Err(TaskFailure::transient("notification entity channel closed")),
                    },
                }
            }
        }
    });
    supervisor
        .register_resident(task)
        .map(drop)
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

fn register_csource_notification_processor(
    supervisor: &TaskSupervisor,
    registration_changes: broadcast::Sender<CsrChangeEvent>,
    subscription_changes: broadcast::Sender<CsourceSubscriptionChange>,
    subscription_store: Arc<dyn SubscriptionStore>,
    registration_store: Arc<dyn RegistrationStore>,
    transport: Arc<TransportDispatcher>,
    tenant: TenantRef,
) -> Result<(), AndromedaError> {
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("csource-notification-events").map_err(|error| AndromedaError::internal(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, restart_delay, restart_delay),
        ShutdownPolicy::new(restart_delay),
    );
    let task = ResidentTask::new(registration, move |execution| {
        let mut registration_receiver = registration_changes.subscribe();
        let mut subscription_receiver = subscription_changes.subscribe();
        let processor = CsourceNotificationProcessor::new(
            Arc::clone(&subscription_store),
            Arc::clone(&registration_store),
            Arc::clone(&transport),
            tenant.clone(),
        );
        async move {
            processor.reload_cache().await.map_err(classify_notification_failure)?;
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = subscription_receiver.recv() => match received {
                        Ok(change) => processor.apply_subscription_change(change).await,
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "context-source subscription stream lagged; reloading policy cache");
                            processor.reload_cache().await.map_err(classify_notification_failure)?;
                        }
                        Err(RecvError::Closed) => return Err(TaskFailure::transient("context-source subscription channel closed")),
                    },
                    received = registration_receiver.recv() => match received {
                        Ok(event) => {
                            loop {
                                match subscription_receiver.try_recv() {
                                    Ok(change) => processor.apply_subscription_change(change).await,
                                    Err(TryRecvError::Lagged(skipped)) => {
                                        tracing::debug!(skipped, "context-source subscription stream lagged before registration evaluation; reloading policy cache");
                                        processor.reload_cache().await.map_err(classify_notification_failure)?;
                                    }
                                    Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                                }
                            }
                            processor.apply_registration_change(event).await;
                        }
                        Err(RecvError::Lagged(skipped)) => tracing::debug!(skipped, "registration stream lagged"),
                        Err(RecvError::Closed) => return Err(TaskFailure::transient("registration channel closed")),
                    },
                }
            }
        }
    });
    supervisor
        .register_resident(task)
        .map(drop)
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

fn register_interval_notification(supervisor: &TaskSupervisor, operation: Arc<IntervalNotificationOperation>) -> Result<(), AndromedaError> {
    register_notification_schedule(supervisor, "notification-interval", move || {
        let operation = Arc::clone(&operation); // clone: each bounded execution owns the shared notification operation.
        async move { operation.execute().await }
    })
}

fn register_csource_interval_notification(supervisor: &TaskSupervisor, operation: Arc<CsourceIntervalNotificationOperation>) -> Result<(), AndromedaError> {
    register_notification_schedule(supervisor, "csource-notification-interval", move || {
        let operation = Arc::clone(&operation); // clone: each bounded execution owns the shared notification operation.
        async move { operation.execute().await }
    })
}

fn register_notification_schedule<F, Fut>(supervisor: &TaskSupervisor, task_name: &'static str, operation: F) -> Result<(), AndromedaError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<usize, NotificationError>> + Send + 'static,
{
    let cadence = Duration::from_secs(1);
    let schedule =
        PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from(task_name).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let operation = Arc::new(operation);
    let task = ScheduledTask::new(registration, schedule, move |_execution| {
        let operation = Arc::clone(&operation); // clone: scheduled execution owns its operation factory.
        async move {
            let due = operation().await.map_err(classify_notification_failure)?;
            tracing::debug!(task = task_name, due, "interval notification evaluation completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

fn classify_notification_failure(error: NotificationError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        NotificationError::Entity(_) | NotificationError::Subscription(_) | NotificationError::Registration(_) | NotificationError::JsonLd(_) => {
            TaskFailure::transient(summary)
        }
        NotificationError::Persistence { .. } => TaskFailure::transient(summary),
        NotificationError::Representation { .. } => TaskFailure::permanent(summary),
        NotificationError::Invariant { .. } => TaskFailure::invariant_violation(summary),
    }
}

fn register_notification_outbox(supervisor: &TaskSupervisor, drain: Arc<NotificationOutboxDrain>, config: &OutboxConfig) -> Result<WakeHandle, AndromedaError> {
    let fallback_duration = Duration::from_secs(config.poll_interval_secs.max(1));
    let fallback = DrainFallbackCadence::new(fallback_duration).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from("notification-outbox-delivery").map_err(|error| AndromedaError::internal(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback_duration, fallback_duration),
        ShutdownPolicy::new(fallback_duration),
    );
    let operation = Arc::clone(&drain); // clone: delivery and cleanup executions share one stateless outbox operation.
    let task = DrainTask::new(registration, fallback, move |execution| {
        let operation = Arc::clone(&operation); // clone: each delivery pass owns the policy operation while executing.
        async move {
            let now = DateTime::<Utc>::from(execution.cutoff());
            let outcome = operation.execute(now).await.map_err(classify_notification_failure)?;
            if outcome.work_may_remain {
                Ok(DrainPassOutcome::WorkMayRemain)
            } else {
                Ok(DrainPassOutcome::Idle)
            }
        }
    });
    let wake = supervisor.register_drain(task).map_err(|error| AndromedaError::internal(error.to_string()))?;

    let cleanup_cadence = Duration::from_secs(config.cleanup_interval_secs.max(1));
    let cleanup_after = chrono::Duration::try_seconds(i64::try_from(config.cleanup_after_secs).map_err(|error| AndromedaError::internal(error.to_string()))?)
        .ok_or_else(|| AndromedaError::internal("notification outbox cleanup age is out of range"))?;
    let schedule = PeriodicSchedule::new(cleanup_cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)
        .map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from("notification-outbox-cleanup").map_err(|error| AndromedaError::internal(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cleanup_cadence, cleanup_cadence),
        ShutdownPolicy::new(cleanup_cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let drain = Arc::clone(&drain); // clone: each cleanup execution owns the policy operation while executing.
        async move {
            let cutoff = DateTime::<Utc>::from(execution.cutoff()) - cleanup_after;
            let purged = drain.purge_delivered(cutoff).await.map_err(classify_notification_failure)?;
            tracing::debug!(purged, "notification outbox cleanup completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| AndromedaError::internal(error.to_string()))?;
    Ok(wake)
}

/// Register entity expiry as the first runtime-managed scheduled policy operation.
fn register_entity_expiry(
    supervisor: &TaskSupervisor,
    entity_policy: Arc<EntityService>,
    entity_expiry_source: Arc<dyn EntityExpirySource>,
    expiration_interval: u64,
    expiry_batch_size: usize,
) -> Result<(), AndromedaError> {
    let cadence = Duration::from_secs(expiration_interval);
    let schedule =
        PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from("entity-expiry").map_err(|error| AndromedaError::internal(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry_batch_size = EntityExpiryBatchSize::try_from(expiry_batch_size).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let sweep = Arc::new(EntityExpirySweep::new(entity_policy, entity_expiry_source, expiry_batch_size));
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
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

/// Register one subscription variant as an independent scheduled policy operation.
fn register_subscription_expiry<V>(
    supervisor: &TaskSupervisor,
    task_name: &'static str,
    policy: Arc<SubscriptionService<V>>,
    expiration_interval: u64,
) -> Result<(), AndromedaError>
where
    V: SubscriptionVariant + 'static,
{
    let cadence = Duration::from_secs(expiration_interval);
    let schedule =
        PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from(task_name).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cadence, cadence),
        ShutdownPolicy::new(cadence),
    );
    let expiry = Arc::new(SubscriptionExpiry::new(policy));
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
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

/// Register semantic registration deletion as its own scheduled operation.
fn register_registration_expiry(
    supervisor: &TaskSupervisor,
    store: Arc<dyn RegistrationStore>,
    change_sink: Arc<dyn RegistrationChangeSink>,
    expiration_interval: u64,
) -> Result<(), AndromedaError> {
    let cadence = Duration::from_secs(expiration_interval);
    let schedule =
        PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from("registration-expiry").map_err(|error| AndromedaError::internal(error.to_string()))?;
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
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

fn register_snapshot_preparation(supervisor: &TaskSupervisor, preparation: Arc<SnapshotPreparation>) -> Result<(), AndromedaError> {
    let cadence = Duration::from_secs(1);
    let schedule =
        PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from("snapshot-preparation").map_err(|error| AndromedaError::internal(error.to_string()))?;
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
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

fn register_snapshot_expiry(supervisor: &TaskSupervisor, store: Arc<dyn SnapshotStore>, expiration_interval: u64) -> Result<(), AndromedaError> {
    let cadence = Duration::from_secs(expiration_interval);
    let schedule =
        PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from("snapshot-expiry").map_err(|error| AndromedaError::internal(error.to_string()))?;
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
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

fn classify_snapshot_failure(error: SnapshotError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        SnapshotError::Persistence { .. } => TaskFailure::transient(summary),
        SnapshotError::Internal { .. } => TaskFailure::invariant_violation(summary),
        SnapshotError::Validation { .. }
        | SnapshotError::NotFound { .. }
        | SnapshotError::AlreadyExists { .. }
        | SnapshotError::Entity(_)
        | SnapshotError::Temporal(_)
        | SnapshotError::Federation(_)
        | SnapshotError::Request(_)
        | SnapshotError::Representation(_) => TaskFailure::permanent(summary),
    }
}

fn register_entity_side_effect_drain(
    supervisor: &TaskSupervisor,
    drain: Arc<EntitySideEffectDrain>,
    fallback_duration: Duration,
    lease_duration: Duration,
) -> Result<WakeHandle, AndromedaError> {
    let fallback = DrainFallbackCadence::new(fallback_duration).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from("entity-side-effect-delivery").map_err(|error| AndromedaError::internal(error.to_string()))?;
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
    supervisor.register_drain(task).map_err(|error| AndromedaError::internal(error.to_string()))
}

fn register_spatial_synchronization(
    supervisor: &TaskSupervisor,
    changes: broadcast::Sender<ChangeEvent>,
    synchronizer: Arc<SpatialSynchronizer>,
) -> Result<(), AndromedaError> {
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("spatial-synchronization").map_err(|error| AndromedaError::internal(error.to_string()))?;
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
    supervisor.register_resident(task).map_err(|error| AndromedaError::internal(error.to_string()))
}

fn register_spatial_reconciliation(
    supervisor: &TaskSupervisor,
    index: Arc<SpatialEntityIndex>,
    source: Arc<dyn EntitySpatialSnapshotSource>,
    tenant: TenantRef,
    cadence: Duration,
) -> Result<(), AndromedaError> {
    let schedule =
        PeriodicSchedule::new(cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip).map_err(|error| AndromedaError::internal(error.to_string()))?;
    let name = TaskName::try_from("spatial-reconciliation").map_err(|error| AndromedaError::internal(error.to_string()))?;
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
        .map_err(|error| AndromedaError::internal(error.to_string()))
}

fn classify_subscription_expiry_failure(error: SubscriptionError) -> TaskFailure {
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

fn classify_registration_expiry_failure(error: RegistrationError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        RegistrationError::Persistence { .. } => TaskFailure::transient(summary),
        RegistrationError::Internal { .. } => TaskFailure::invariant_violation(summary),
        RegistrationError::Validation { .. }
        | RegistrationError::NotFound { .. }
        | RegistrationError::AlreadyExists { .. }
        | RegistrationError::JsonLd(_)
        | RegistrationError::Query(_)
        | RegistrationError::Representation(_)
        | RegistrationError::Identifier(_) => TaskFailure::permanent(summary),
    }
}

/// Classify exhaustive entity-policy failures at the binary composition boundary.
fn classify_entity_expiry_failure(error: EntityError) -> TaskFailure {
    let summary = error.to_string();
    match error {
        EntityError::Commit(_) | EntityError::Persistence { detail: _ } => TaskFailure::transient(summary),
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
