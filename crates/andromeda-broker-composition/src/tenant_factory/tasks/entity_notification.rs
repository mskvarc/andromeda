//! The workers turning entity changes and elapsed intervals into clause 5.8
//! notifications.
//!
//! The drain delivering what the tenant database queued is registered in
//! `crate::tenant_factory::tasks::notification_delivery`; the resident
//! processor registered here is its transport.
//!
//! Clause 5.11's context source notifications are registered separately, in
//! `crate::tenant_factory::tasks::csource_notification`: they watch a
//! different change stream, carry registrations rather than entities, and
//! share nothing with these beyond the schedule helper.

use crate::{
    linked_entity_forwarding::ForwardedLinkedEntitySource,
    tenant_factory::{
        assembly_invariant::AssemblyInvariant,
        background_pacer::BackgroundPacer,
        error::TenantAssemblyError,
        storage::TenantStorage,
        tasks::{
            EntityNotificationDelivery,
            notification_delivery::{DeliveryDrainDrive, register_notification_delivery},
            notification_schedule::{register_notification_schedule, register_periodic_wake},
            notification_worker_failure::{ChangeChannel, channel_closed, worker_notification_failure},
        },
    },
};
use andromeda_context_commit::context_change_event::ContextChangeEvent;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_entity::{
    config::EntityConfig,
    query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource},
};
use andromeda_federation::FederationService;
use andromeda_jsonld::operation_hit_scope::run_in_operation_scope;
use andromeda_notification::{
    delivery_config::DeliveryConfig,
    interval::{IntervalNotificationDeps, IntervalNotificationOperation},
    processor::{NotificationProcessor, NotificationProcessorDeps},
};
use andromeda_subscription::{change::SubscriptionChange, store::SubscriptionStore};
use andromeda_task_runtime::{
    criticality::TaskCriticality,
    registration::TaskRegistration,
    resident_task::ResidentTask,
    restart_policy::RestartPolicy,
    shutdown_policy::ShutdownPolicy,
    supervisor::TaskSupervisor,
    task_name::TaskName,
    wake_trigger::wake_channel,
};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::sync::{
    broadcast,
    broadcast::error::{RecvError, TryRecvError},
};

/// Compose the query service one tenant's notifications resolve the Linked
/// Entities of their `join` with, change-driven and periodic alike.
///
/// A notification's join resolves Linked Entities exactly as a request's does,
/// so the service is composed like the request-facing one.
pub(super) fn compose_notification_query(
    storage: &TenantStorage,
    forwarding: &Arc<FederationService>,
    entity_config: &EntityConfig,
) -> Arc<EntityQueryService> {
    Arc::new(EntityQueryService::new(
        Arc::clone(&storage.entity_ports.reader),
        Arc::clone(&storage.entity_ports.query),
        storage.engine.clone(),  // clone: the notification query owns its engine handle.
        storage.spatial.clone(), // clone: the notification query narrows through the tenant's one index.
        storage.tenant.clone(),  // clone: the query service owns its tenant partition.
        // A notification's join resolves Linked Entities exactly as a request's
        // does, including the ones clause 4.5.23.1 places outside this broker.
        Some(Arc::new(ForwardedLinkedEntitySource::new(
            Arc::clone(forwarding),      // clone: request handling and notification joins share one forwarding service.
            storage.live_target.clone(), // clone: linked resolution runs against the tenant's live branch.
        )) as Arc<dyn RemoteLinkedEntitySource>),
        // A notification query only resolves Linked Entities by identifier and
        // compiles no reference geometry: a subscription's `geoQ` is matched in
        // memory from its stored form, admitted once when it was written, so a
        // lower limit configured since never refuses a stored subscription.
        entity_config.geometry_census_limits(),
    ))
}

/// Compose the notification policy operation one tenant evaluates its
/// subscriptions with.
pub(super) fn compose_notification_processor(
    storage: &TenantStorage,
    notification_query: Arc<EntityQueryService>,
    transport: &Arc<TransportDispatcher>,
    subscriptions: &Arc<dyn SubscriptionStore>,
) -> Arc<NotificationProcessor> {
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

/// Register the workers driving one tenant's clause 5.8 notification policy:
/// the resident processor, and the drain delivering the notifications queued
/// in the tenant's database.
///
/// The processor evaluates the tenant's committed entity changes only under
/// live delivery, sending each notification at once. Under durable delivery
/// the committer records every notification in the commit that raised it (the
/// entity services' notification write configuration) and the processor
/// evaluates no change of its own. Under either mode the processor is the
/// drain's transport, re-evaluating and sending each queued delivery the drain
/// claims; the drain itself runs under either mode, for the reasons
/// `crate::tenant_factory::tasks::notification_delivery` gives.
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
    drive: DeliveryDrainDrive,
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
        Arc::clone(&processor), // clone: the resident processor is also the delivery drain's transport.
        delivery,
    )?;
    register_notification_delivery(supervisor, pool, config, processor, drive)
}

/// What the periodic sweep reads and delivers through beside the tenant's
/// storage.
pub(super) struct IntervalDelivery {
    /// The tenant's entity subscriptions.
    pub(super) subscriptions: Arc<dyn SubscriptionStore>,
    /// The query service the sweep resolves `join` Linked Entities with.
    pub(super) notification_query: Arc<EntityQueryService>,
    /// The dispatcher every worker delivers through.
    pub(super) transport: Arc<TransportDispatcher>,
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
    delivery: IntervalDelivery,
    subscription_changes: broadcast::Sender<SubscriptionChange>,
    interval_max_sleep: Duration,
    pacer: Arc<BackgroundPacer>,
) -> Result<(), TenantAssemblyError> {
    let operation = Arc::new(IntervalNotificationOperation::new(IntervalNotificationDeps {
        target: storage.live_target.clone(), // clone: interval delivery and expiry both operate on the tenant's live branch.
        sub_repo: delivery.subscriptions,
        entity_reader: Arc::clone(&storage.entity_ports.reader),
        entity_pages: Arc::clone(&storage.entity_ports.payload_pages),
        entity_query: delivery.notification_query,
        engine: storage.engine.clone(), // clone: interval policy owns its engine handle.
        transport: delivery.transport,
        tenant: storage.tenant.clone(), // clone: interval policy owns its tenant partition.
    }));
    let (interval_wake, receiver) = wake_channel();
    register_periodic_wake(
        supervisor,
        "notification-interval-wake",
        interval_wake,
        ChangeChannel::Subscription,
        subscription_changes,
    )?;
    register_notification_schedule(supervisor, "notification-interval", interval_max_sleep, receiver, pacer, move |cutoff| {
        let operation = Arc::clone(&operation); // clone: each bounded pass owns the shared notification operation.
        async move {
            operation.execute(cutoff).await?;
            operation.next_due(cutoff).await
        }
    })
}

fn register_notification_processor(
    supervisor: &TaskSupervisor,
    entity_changes: broadcast::Sender<ContextChangeEvent>,
    subscription_changes: broadcast::Sender<SubscriptionChange>,
    processor: Arc<NotificationProcessor>,
    delivery: EntityNotificationDelivery,
) -> Result<(), TenantAssemblyError> {
    let restart_delay = Duration::from_secs(1);
    let name = TaskName::try_from("notification-events").map_err(AssemblyInvariant::from)?;
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
            processor.reload_cache().await.map_err(worker_notification_failure)?;
            execution.mark_running();
            loop {
                tokio::select! {
                    () = execution.cancellation().cancelled() => return Ok(()),
                    received = subscription_receiver.recv() => match received {
                        Ok(change) => processor.apply_subscription_change(change).await,
                        Err(RecvError::Lagged(skipped)) => {
                            tracing::debug!(skipped, "notification subscription stream lagged; reloading policy cache");
                            processor.reload_cache().await.map_err(worker_notification_failure)?;
                        }
                        Err(RecvError::Closed) => return Err(channel_closed(ChangeChannel::Subscription)),
                    },
                    received = entity_receiver.recv(), if matches!(delivery, EntityNotificationDelivery::Live) => match received {
                        Ok(event) => {
                            loop {
                                match subscription_receiver.try_recv() {
                                    Ok(change) => processor.apply_subscription_change(change).await,
                                    Err(TryRecvError::Lagged(skipped)) => {
                                        tracing::debug!(skipped, "notification subscription stream lagged before entity evaluation; reloading policy cache");
                                        processor.reload_cache().await.map_err(worker_notification_failure)?;
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
                        Err(RecvError::Closed) => return Err(channel_closed(ChangeChannel::Entity)),
                    },
                }
            }
        }
    });
    supervisor
        .register_resident(task)
        .map(drop)
        .map_err(|error| AssemblyInvariant::from(error).into())
}
