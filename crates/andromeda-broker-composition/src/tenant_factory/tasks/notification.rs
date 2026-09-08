//! Notification task registration and runtime failure classification.

use crate::{
    linked_entity_forwarding::ForwardedLinkedEntitySource,
    tenant_factory::{
        TenantFactoryConfig,
        error::TenantAssemblyError,
        storage::TenantStorage,
        tasks::{EntityNotificationDelivery, retention::retention_duration},
    },
};
use andromeda_context_commit::context_change_event::ContextChangeEvent;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_entity::query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource};
use andromeda_federation::FederationService;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_notification::{
    csource_interval::CsourceIntervalNotificationOperation,
    csource_processor::CsourceNotificationProcessor,
    delivery_config::DeliveryConfig,
    delivery_drain::{NotificationDeliveryDrain, NotificationDeliveryTransport},
    delivery_store::DeliveryStore,
    error::NotificationError,
    interval::IntervalNotificationOperation,
    processor::{NotificationProcessor, NotificationProcessorDeps},
};
use andromeda_postgresql_context_store::notification_delivery::queue::NotificationDeliveryQueue;
use andromeda_registration::store::RegistrationStore;
use andromeda_subscription::{
    change::{CsourceSubscriptionChange, SubscriptionChange},
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
use std::{future::Future, sync::Arc, time::Duration};
use tokio::sync::{
    broadcast,
    broadcast::error::{RecvError, TryRecvError},
};

/// Compose the tenant's notification policy and register the workers that
/// drive it: the resident processor evaluating subscriptions and, when durable
/// delivery is configured, the drain that delivers what it queued.
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
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    forwarding: &Arc<FederationService>,
    tenant: &TenantRef,
    transport: &Arc<TransportDispatcher>,
    subscriptions: &Arc<dyn SubscriptionStore>,
    entity_changes: &broadcast::Sender<ContextChangeEvent>,
    subscription_changes: broadcast::Sender<SubscriptionChange>,
) -> Result<Option<WakeHandle>, TenantAssemblyError> {
    let notification_query = Arc::new(EntityQueryService::new(
        Arc::clone(&storage.entity_ports.reader),
        Arc::clone(&storage.entity_ports.query),
        storage.engine.clone(), // clone: the notification query owns its engine handle.
        Arc::clone(&storage.spatial_index),
        tenant.clone(), // clone: the query service owns its tenant partition.
        // A notification's join resolves Linked Entities exactly as a request's
        // does, including the ones clause 4.5.23.1 places outside this broker.
        Some(Arc::new(ForwardedLinkedEntitySource::new(
            Arc::clone(forwarding),      // clone: request handling and notification joins share one forwarding service.
            storage.live_target.clone(), // clone: linked resolution runs against the tenant's live branch.
        )) as Arc<dyn RemoteLinkedEntitySource>),
    ));
    let processor = Arc::new(NotificationProcessor::new(NotificationProcessorDeps {
        live_target: storage.live_target.clone(), // clone: worker startup reloads the durable live-branch cache.
        sub_repo: Arc::clone(subscriptions),      // clone: the processor and the interval worker share one store.
        entity_reader: Arc::clone(&storage.entity_ports.reader),
        entity_query: notification_query,
        engine: storage.engine.clone(),   // clone: notification policy owns its engine handle.
        transport: Arc::clone(transport), // clone: every worker delivers through one dispatcher.
        tenant: tenant.clone(),           // clone: notification policy owns its tenant partition.
        subscription_cache: Arc::clone(&storage.subscription_cache),
    }));
    let delivery = if config.notification_delivery.enabled {
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
    if !config.notification_delivery.enabled {
        return Ok(None);
    }
    let store = Arc::new(NotificationDeliveryQueue::new(&storage.pool)) as Arc<dyn DeliveryStore>;
    let drain = NotificationDeliveryDrain::new(store, processor as Arc<dyn NotificationDeliveryTransport>, &config.notification_delivery)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    register_notification_delivery(supervisor, Arc::new(drain), &config.notification_delivery).map(Some)
}

/// Register the time-interval notification workers and the resident processor
/// that turns registration changes into clause 5.11 notifications.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the supervisor refuses a registration.
pub(super) fn register_interval_workers(
    supervisor: &TaskSupervisor,
    storage: &TenantStorage,
    tenant: &TenantRef,
    transport: &Arc<TransportDispatcher>,
    subscriptions: Arc<dyn SubscriptionStore>,
    csource_subscriptions: Arc<dyn SubscriptionStore>,
    registration_changes: broadcast::Sender<CsrChangeEvent>,
    csource_subscription_changes: broadcast::Sender<CsourceSubscriptionChange>,
) -> Result<(), TenantAssemblyError> {
    register_interval_notification(
        supervisor,
        Arc::new(IntervalNotificationOperation::new(
            storage.live_target.clone(), // clone: interval delivery and expiry both operate on the tenant's live branch.
            subscriptions,
            Arc::clone(&storage.entity_ports.reader),
            Arc::clone(&storage.entity_ports.discovery),
            storage.engine.clone(), // clone: interval policy owns its engine handle.
            Arc::clone(transport),  // clone: every worker delivers through one dispatcher.
            tenant.clone(),         // clone: interval policy owns its tenant partition.
        )),
    )?;
    register_csource_notification_processor(
        supervisor,
        storage.live_target.clone(), // clone: the resident processor owns the live branch it evaluates.
        registration_changes,
        csource_subscription_changes,
        Arc::clone(&csource_subscriptions),      // clone: the processor and the interval worker share one store.
        Arc::clone(&storage.registration_store), // clone: every registration worker shares one store.
        Arc::clone(transport),                   // clone: every worker delivers through one dispatcher.
        tenant.clone(),                          // clone: notification policy owns its tenant partition.
    )?;
    register_csource_interval_notification(
        supervisor,
        Arc::new(CsourceIntervalNotificationOperation::new(
            storage.live_target.clone(), // clone: interval delivery operates on the tenant's live branch.
            csource_subscriptions,
            Arc::clone(&storage.registration_store), // clone: every registration worker shares one store.
            Arc::clone(transport),                   // clone: every worker delivers through one dispatcher.
            tenant.clone(),                          // clone: interval policy owns its tenant partition.
        )),
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
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

fn register_csource_notification_processor(
    supervisor: &TaskSupervisor,
    target: ContextTarget,
    registration_changes: broadcast::Sender<CsrChangeEvent>,
    subscription_changes: broadcast::Sender<CsourceSubscriptionChange>,
    subscription_store: Arc<dyn SubscriptionStore>,
    registration_store: Arc<dyn RegistrationStore>,
    transport: Arc<TransportDispatcher>,
    tenant: TenantRef,
) -> Result<(), TenantAssemblyError> {
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("csource-notification-events").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
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
            target.clone(), // clone: each resident restart owns the same immutable live branch target.
            Arc::clone(&subscription_store),
            Arc::clone(&registration_store),
            Arc::clone(&transport),
            tenant.clone(),
        );
        async move {
            processor.reload_cache().await.map_err(|error| classify_notification_failure(&error))?;
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = subscription_receiver.recv() => match received {
                        Ok(change) => processor.apply_subscription_change(change).await,
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "context-source subscription stream lagged; reloading policy cache");
                            processor.reload_cache().await.map_err(|error| classify_notification_failure(&error))?;
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
                                        processor.reload_cache().await.map_err(|error| classify_notification_failure(&error))?;
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
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

fn register_interval_notification(supervisor: &TaskSupervisor, operation: Arc<IntervalNotificationOperation>) -> Result<(), TenantAssemblyError> {
    register_notification_schedule(supervisor, "notification-interval", move || {
        let operation = Arc::clone(&operation); // clone: each bounded execution owns the shared notification operation.
        async move { operation.execute().await }
    })
}

fn register_csource_interval_notification(
    supervisor: &TaskSupervisor,
    operation: Arc<CsourceIntervalNotificationOperation>,
) -> Result<(), TenantAssemblyError> {
    register_notification_schedule(supervisor, "csource-notification-interval", move || {
        let operation = Arc::clone(&operation); // clone: each bounded execution owns the shared notification operation.
        async move { operation.execute().await }
    })
}

fn register_notification_schedule<F, Fut>(supervisor: &TaskSupervisor, task_name: &'static str, operation: F) -> Result<(), TenantAssemblyError>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<usize, NotificationError>> + Send + 'static,
{
    let cadence = Duration::from_secs(1);
    let schedule = PeriodicSchedule::new(cadence, InitialExecution::Immediate, MissedTickPolicy::Skip)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from(task_name).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
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
            let due = operation().await.map_err(|error| classify_notification_failure(&error))?;
            tracing::debug!(task = task_name, due, "interval notification evaluation completed");
            Ok(())
        }
    });
    supervisor
        .register_scheduled(task)
        .map(drop)
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

/// Convert a configured retention in seconds into a [`chrono::Duration`],
/// naming the offending setting in the error rather than the raw number.
fn classify_notification_failure(error: &NotificationError) -> TaskFailure {
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

fn register_notification_delivery(
    supervisor: &TaskSupervisor,
    drain: Arc<NotificationDeliveryDrain>,
    config: &DeliveryConfig,
) -> Result<WakeHandle, TenantAssemblyError> {
    let fallback_duration = Duration::from_secs(config.poll_interval_secs.max(1));
    let fallback = DrainFallbackCadence::new(fallback_duration).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let name = TaskName::try_from("notification-delivery").map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    let registration = TaskRegistration::new(
        name,
        TaskCriticality::TenantDegrading,
        RestartPolicy::transient(u32::MAX, fallback_duration, fallback_duration),
        ShutdownPolicy::new(fallback_duration),
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
    let wake = supervisor
        .register_drain(task)
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
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    Ok(wake)
}
