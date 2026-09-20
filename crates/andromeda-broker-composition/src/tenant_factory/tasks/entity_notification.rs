//! The workers turning entity changes and elapsed intervals into clause 5.8
//! notifications, and the drain that delivers what they queued.
//!
//! Clause 5.11's context source notifications are registered separately, in
//! `crate::tenant_factory::tasks::csource_notification`: they watch a
//! different change stream, carry registrations rather than entities, and
//! share nothing with these beyond the schedule helper.

use crate::{
    linked_entity_forwarding::ForwardedLinkedEntitySource,
    tenant_factory::{
        error::TenantAssemblyError,
        storage::TenantStorage,
        tasks::{
            EntityNotificationDelivery,
            notification_schedule::{classify_notification_failure, register_notification_schedule},
            retention::retention_duration,
        },
    },
};
use andromeda_context_commit::context_change_event::ContextChangeEvent;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_entity::query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource};
use andromeda_federation::FederationService;
use andromeda_jsonld::operation_hit_scope::run_in_operation_scope;
use andromeda_notification::{
    delivery_config::DeliveryConfig,
    delivery_drain::{NotificationDeliveryDrain, NotificationDeliveryTransport},
    delivery_store::DeliveryStore,
    interval::IntervalNotificationOperation,
    processor::{NotificationProcessor, NotificationProcessorDeps},
};
use andromeda_postgresql_context_store::notification_delivery::queue::NotificationDeliveryQueue;
use andromeda_subscription::{change::SubscriptionChange, store::SubscriptionStore};
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
    wake_trigger::{DrainFallbackPolicy, DrainWakeReceiver, wake_channel},
};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::{
    broadcast,
    broadcast::error::{RecvError, TryRecvError},
};

/// Compose the notification policy operation one tenant evaluates its
/// subscriptions with.
///
/// A notification's join resolves Linked Entities exactly as a request's does,
/// so the operation owns a query service composed like the request-facing one.
pub(super) fn compose_notification_processor(
    storage: &TenantStorage,
    forwarding: &Arc<FederationService>,
    transport: &Arc<TransportDispatcher>,
    subscriptions: &Arc<dyn SubscriptionStore>,
) -> Arc<NotificationProcessor> {
    let notification_query = Arc::new(EntityQueryService::new(
        Arc::clone(&storage.entity_ports.reader),
        Arc::clone(&storage.entity_ports.query),
        storage.engine.clone(), // clone: the notification query owns its engine handle.
        Arc::clone(&storage.spatial_index),
        storage.tenant.clone(), // clone: the query service owns its tenant partition.
        // A notification's join resolves Linked Entities exactly as a request's
        // does, including the ones clause 4.5.23.1 places outside this broker.
        Some(Arc::new(ForwardedLinkedEntitySource::new(
            Arc::clone(forwarding),      // clone: request handling and notification joins share one forwarding service.
            storage.live_target.clone(), // clone: linked resolution runs against the tenant's live branch.
        )) as Arc<dyn RemoteLinkedEntitySource>),
    ));
    Arc::new(NotificationProcessor::new(NotificationProcessorDeps {
        live_target: storage.live_target.clone(), // clone: worker startup reloads the durable live-branch cache.
        sub_repo: Arc::clone(subscriptions),      // clone: the processor and the interval worker share one store.
        entity_reader: Arc::clone(&storage.entity_ports.reader),
        entity_query: notification_query,
        engine: storage.engine.clone(),   // clone: notification policy owns its engine handle.
        transport: Arc::clone(transport), // clone: every worker delivers through one dispatcher.
        tenant: storage.tenant.clone(),   // clone: notification policy owns its tenant partition.
        subscription_cache: Arc::clone(&storage.subscription_cache),
    }))
}

/// Register the workers driving one tenant's notification policy: the resident
/// processor evaluating subscriptions and, when durable delivery is configured,
/// the drain that delivers what it queued.
///
/// The returned wake handle exists only in durable mode; live delivery has no
/// queue to wake.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the delivery configuration is rejected
/// or when the supervisor refuses a registration.
pub(super) fn register_notification_workers(
    supervisor: &TaskSupervisor,
    pool: &PgPool,
    config: &DeliveryConfig,
    processor: Arc<NotificationProcessor>,
    entity_changes: &broadcast::Sender<ContextChangeEvent>,
    subscription_changes: broadcast::Sender<SubscriptionChange>,
    wake: Option<DrainWakeReceiver>,
) -> Result<(), TenantAssemblyError> {
    let delivery = if config.is_durable() {
        EntityNotificationDelivery::Durable
    } else {
        EntityNotificationDelivery::Live
    };
    register_notification_processor(
        supervisor,
        entity_changes.clone(), // clone: each worker owns a sender handle it subscribes through.
        subscription_changes,
        Arc::clone(&processor), // clone: the resident processor is also the durable delivery transport.
        delivery,
    )?;
    if !config.is_durable() {
        return Ok(());
    }
    let wake = wake.ok_or_else(|| TenantAssemblyError::invariant("durable notification delivery requires a wake receiver"))?;
    let store = Arc::new(NotificationDeliveryQueue::new(pool)) as Arc<dyn DeliveryStore>;
    let drain = NotificationDeliveryDrain::new(store, processor as Arc<dyn NotificationDeliveryTransport>, config)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    register_notification_delivery(supervisor, Arc::new(drain), config, wake)
}

/// Register the sweep delivering one tenant's `timeInterval` entity
/// subscriptions.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the supervisor refuses the registration.
pub(super) fn register_interval_notification(
    supervisor: &TaskSupervisor,
    storage: &TenantStorage,
    transport: &Arc<TransportDispatcher>,
    subscriptions: Arc<dyn SubscriptionStore>,
    interval_max_sleep_secs: u64,
) -> Result<(), TenantAssemblyError> {
    let operation = Arc::new(IntervalNotificationOperation::new(
        storage.live_target.clone(), // clone: interval delivery and expiry both operate on the tenant's live branch.
        subscriptions,
        Arc::clone(&storage.entity_ports.reader),
        Arc::clone(&storage.entity_ports.discovery),
        storage.engine.clone(), // clone: interval policy owns its engine handle.
        Arc::clone(transport),  // clone: every worker delivers through one dispatcher.
        storage.tenant.clone(), // clone: interval policy owns its tenant partition.
    ));
    let (_interval_wake, receiver) = wake_channel();
    register_notification_schedule(
        supervisor,
        "notification-interval",
        Duration::from_secs(interval_max_sleep_secs),
        receiver,
        move |cutoff| {
            let operation = Arc::clone(&operation); // clone: each bounded pass owns the shared notification operation.
            async move {
                operation.execute(cutoff).await?;
                operation.next_due(cutoff).await
            }
        },
    )
}

fn register_notification_processor(
    supervisor: &TaskSupervisor,
    entity_changes: broadcast::Sender<ContextChangeEvent>,
    subscription_changes: broadcast::Sender<SubscriptionChange>,
    processor: Arc<NotificationProcessor>,
    delivery: EntityNotificationDelivery,
) -> Result<(), TenantAssemblyError> {
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("notification-events").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
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
            processor.reload_cache().await.map_err(|error| classify_notification_failure(&error))?;
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = subscription_receiver.recv() => match received {
                        Ok(change) => processor.apply_subscription_change(change).await,
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "notification subscription stream lagged; reloading policy cache");
                            processor.reload_cache().await.map_err(|error| classify_notification_failure(&error))?;
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
                                        processor.reload_cache().await.map_err(|error| classify_notification_failure(&error))?;
                                    }
                                    Err(TryRecvError::Empty | TryRecvError::Closed) => break,
                                }
                            }
                            // Clause 5.13.3.5: counting one hit per context per
                            // operation means the notification's own context
                            // resolutions count once for this change, distinct
                            // from the write that produced it.
                            run_in_operation_scope(processor.apply_entity_change(event)).await;
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
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

fn register_notification_delivery(
    supervisor: &TaskSupervisor,
    drain: Arc<NotificationDeliveryDrain>,
    config: &DeliveryConfig,
    wake: DrainWakeReceiver,
) -> Result<(), TenantAssemblyError> {
    let fallback_floor = Duration::from_secs(config.poll_interval_secs.max(1));
    let fallback_ceiling = Duration::from_secs(config.poll_interval_max_secs.max(config.poll_interval_secs).max(1));
    let fallback = DrainFallbackPolicy::new(fallback_floor, fallback_ceiling).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("notification-delivery").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback_floor, fallback_ceiling),
        ShutdownPolicy::new(fallback_floor),
    );
    let operation = Arc::clone(&drain); // clone: delivery and cleanup executions share one stateless delivery operation.
    let task = DrainTask::new(registration, fallback, move |execution| {
        let operation = Arc::clone(&operation); // clone: each delivery pass owns the policy operation while executing.
        async move {
            let now = DateTime::<Utc>::from(execution.cutoff());
            let outcome = operation.execute(now).await.map_err(|error| classify_notification_failure(&error))?;
            if outcome.work_may_remain {
                Ok(DrainPassOutcome::WorkMayRemain)
            } else {
                Ok(DrainPassOutcome::Idle)
            }
        }
    });
    supervisor
        .register_drain(task, wake)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;

    let cleanup_cadence = Duration::from_secs(config.cleanup_interval_secs.max(1));
    let cleanup_after = retention_duration(config.cleanup_after_secs, "notification delivery cleanup age")?;
    let dead_letter_after = retention_duration(config.dead_letter_after_secs, "notification delivery dead-letter age")?;
    let orphan_event_after = retention_duration(config.orphan_event_after_secs, "notification delivery orphan-event age")?;
    let schedule = PeriodicSchedule::new(cleanup_cadence, InitialExecution::AfterCadence, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("notification-delivery-cleanup").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::Observed,
        RestartPolicy::transient(u32::MAX, cleanup_cadence, cleanup_cadence),
        ShutdownPolicy::new(cleanup_cadence),
    );
    let task = ScheduledTask::new(registration, schedule, move |execution| {
        let drain = Arc::clone(&drain); // clone: each cleanup execution owns the policy operation while executing.
        async move {
            let now = DateTime::<Utc>::from(execution.cutoff());
            let purged = drain
                .purge_delivered(now - cleanup_after)
                .await
                .map_err(|error| classify_notification_failure(&error))?;
            // Dead letters are reaped before the orphan pass so an event a
            // dead-lettered delivery just released can be swept in the same
            // tick once it also clears the orphan-event horizon.
            let dead_lettered = drain
                .purge_exhausted(now - dead_letter_after)
                .await
                .map_err(|error| classify_notification_failure(&error))?;
            let orphaned = drain
                .purge_orphan_events(now - orphan_event_after)
                .await
                .map_err(|error| classify_notification_failure(&error))?;
            tracing::debug!(purged, dead_lettered, orphaned, "notification delivery cleanup completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}
