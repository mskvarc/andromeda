//! Notification task registration and runtime failure classification.

use crate::tenant_factory::tasks::EntityNotificationDelivery;

use crate::tenant_factory::error::TenantAssemblyError;
use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_notification::{
    csource_interval::CsourceIntervalNotificationOperation,
    csource_processor::CsourceNotificationProcessor,
    delivery_config::DeliveryConfig,
    delivery_drain::NotificationDeliveryDrain,
    error::NotificationError,
    interval::IntervalNotificationOperation,
    processor::NotificationProcessor,
};
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
use std::{sync::Arc, time::Duration};
use tokio::sync::{
    broadcast,
    broadcast::error::{RecvError, TryRecvError},
};

pub(super) fn register_notification_processor(
    supervisor: &TaskSupervisor,
    entity_changes: broadcast::Sender<ChangeEvent>,
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
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

pub(super) fn register_csource_notification_processor(
    supervisor: &TaskSupervisor,
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
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))
}

pub(super) fn register_interval_notification(supervisor: &TaskSupervisor, operation: Arc<IntervalNotificationOperation>) -> Result<(), TenantAssemblyError> {
    register_notification_schedule(supervisor, "notification-interval", move || {
        let operation = Arc::clone(&operation); // clone: each bounded execution owns the shared notification operation.
        async move { operation.execute().await }
    })
}

pub(super) fn register_csource_interval_notification(
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
    Fut: std::future::Future<Output = Result<usize, NotificationError>> + Send + 'static,
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
            let due = operation().await.map_err(classify_notification_failure)?;
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
fn retention_duration(seconds: u64, name: &'static str) -> Result<chrono::Duration, TenantAssemblyError> {
    let seconds = i64::try_from(seconds).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
    chrono::Duration::try_seconds(seconds).ok_or_else(|| TenantAssemblyError::invariant(format!("{name} is out of range")))
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

pub(super) fn register_notification_delivery(
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
            let outcome = operation.execute(now).await.map_err(classify_notification_failure)?;
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
            let purged = drain.purge_delivered(now - cleanup_after).await.map_err(classify_notification_failure)?;
            // Dead letters are reaped before the orphan pass so an event a
            // dead-lettered delivery just released can be swept in the same
            // tick once it also clears the orphan-event horizon.
            let dead_lettered = drain.purge_exhausted(now - dead_letter_after).await.map_err(classify_notification_failure)?;
            let orphaned = drain
                .purge_orphan_events(now - orphan_event_after)
                .await
                .map_err(classify_notification_failure)?;
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
