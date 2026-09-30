//! Construction and supervision of one tenant's background tasks.

mod csource_notification;
mod discovery_census;
mod entity_notification;
mod expiry;
mod maintenance;
mod notification_delivery;
mod notification_schedule;
mod notification_worker_failure;
mod pool_gauges;
mod retention;
mod snapshot;
mod table_size_gauges;
mod temporal_retention;

use crate::tenant_factory::{
    TenantFactoryConfig,
    background_pacer::BackgroundPacer,
    error::TenantAssemblyError,
    services::ConstructedServices,
    storage::TenantStorage,
    tasks::{
        csource_notification::{CsourceNotificationStreams, register_csource_notification_workers},
        discovery_census::register_discovery_census_fold,
        entity_notification::{compose_notification_processor, register_interval_notification, register_notification_workers},
        expiry::register_expiry_workers,
        maintenance::{register_entity_side_effects, register_spatial_workers},
        notification_delivery::DeliveryDrainDrive,
        pool_gauges::register_pool_gauges,
        snapshot::{PreparationRecovery, SnapshotStatusWake, register_snapshot_workers},
        table_size_gauges::register_table_size_gauges,
        temporal_retention::register_temporal_retention,
    },
};
use andromeda_context_commit::{committer::ContextCommitter, context_change_event::ContextChangeEvent};
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_postgresql_context_store::{csource_subscription::repo::CsourceSubscriptionRepository, subscription::repo::SubscriptionRepository};
use andromeda_snapshot::status_delivery_signal::StatusDeliverySignal;
use andromeda_subscription::{
    change::{CsourceSubscriptionChange, SubscriptionChange},
    store::SubscriptionStore,
};
use andromeda_task_runtime::{
    supervisor::TaskSupervisor,
    wake_trigger::{DrainWakeReceiver, WakeHandle},
};
use std::sync::Arc;
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
};

/// Change channels shared by tenant services and their workers.
pub(super) struct TenantSignals {
    pub(super) entity: broadcast::Sender<ContextChangeEvent>,
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

/// Handles needed to stop the complete tenant worker set.
pub(super) struct TaskSupervision {
    pub(super) handles: Vec<JoinHandle<()>>,
    pub(super) health_shutdown: Option<watch::Sender<bool>>,
    pub(super) task_supervisor: TaskSupervisor,
}

/// The change signals and consequence-drain wake receivers a tenant's tasks
/// consume, bundled so supervision takes one input rather than several.
pub(super) struct TenantTaskChannels {
    /// The entity, subscription and registration change streams.
    pub(super) signals: TenantSignals,
    /// The notification-delivery drain's wake receiver.
    pub(super) notification_wake: DrainWakeReceiver,
    /// The entity side-effect drain's wake receiver.
    pub(super) side_effect_wake: DrainWakeReceiver,
    /// The Snapshot status-delivery drain's wake handle, shared with the
    /// scheduled preparation sweep and the create-time preparation path.
    pub(super) snapshot_status_wake: WakeHandle,
    /// The Snapshot status-delivery drain's wake receiver.
    pub(super) snapshot_status_receiver: DrainWakeReceiver,
    /// Decides when the tenant's background passes run; every task consults
    /// the one pacer, so one obligation read serves them all.
    pub(super) pacer: Arc<BackgroundPacer>,
}

/// Build the delivery signal the Snapshot service raises after enqueuing status
/// notifications outside the scheduled preparation sweep, wrapping a clone of the
/// status-delivery drain's wake handle.
pub(super) fn snapshot_delivery_signal(wake: WakeHandle) -> Arc<dyn StatusDeliverySignal> {
    Arc::new(snapshot::StatusDeliveryWaker::new(wake))
}

/// Whether entity notifications are delivered live or through the durable queue.
#[derive(Clone, Copy)]
enum EntityNotificationDelivery {
    Live,
    Durable,
}

/// Spawn every background worker owned by a tenant.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when a worker's configuration is out of
/// range or the supervisor refuses one of the registrations.
pub(super) fn supervise_tasks(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    services: &ConstructedServices,
    writes: &Arc<dyn ContextCommitter>,
    transport: &Arc<TransportDispatcher>,
    task_supervisor: TaskSupervisor,
    channels: TenantTaskChannels,
) -> Result<TaskSupervision, TenantAssemblyError> {
    let TenantTaskChannels {
        signals,
        notification_wake,
        side_effect_wake,
        snapshot_status_wake,
        snapshot_status_receiver,
        pacer,
    } = channels;
    let TenantSignals {
        entity,
        subscription,
        registration,
        csource_subscription,
    } = signals;
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    register_pool_gauges(&task_supervisor, storage.pool.clone(), &storage.tenant)?; // clone: PgPool is a shared handle sampled by the gauge task.
    // clone: PgPool is a shared handle sampled by the size-gauge task, which consults the tenant's one pacer.
    register_table_size_gauges(&task_supervisor, Arc::clone(&pacer), storage.pool.clone(), &storage.tenant)?;
    // clone: PgPool is a shared handle the census fold acquires from, and the fold consults the tenant's one pacer.
    register_discovery_census_fold(&task_supervisor, Arc::clone(&pacer), storage.pool.clone())?;
    // clone: every background pass of the tenant consults its one pacer.
    register_entity_side_effects(&task_supervisor, config, storage, writes, &entity, side_effect_wake, Arc::clone(&pacer))?;
    let health_shutdown = if storage.tenant.is_unnamed_default() {
        let (shutdown, receiver) = watch::channel(false);
        handles.push(transport.mqtt_pool().spawn_health_probe(receiver));
        Some(shutdown)
    } else {
        None
    };
    // clone: both subscription stores intern attribute lists through the tenant's one dictionary.
    let entity_subscriptions = Arc::new(SubscriptionRepository::new(&storage.pool, Arc::clone(&storage.attribute_names))) as Arc<dyn SubscriptionStore>;
    let csource_subscriptions = Arc::new(CsourceSubscriptionRepository::new(&storage.pool, Arc::clone(&storage.attribute_names))) as Arc<dyn SubscriptionStore>;
    let notification_processor = compose_notification_processor(storage, &services.state.forwarding, transport, &entity_subscriptions);
    // clone: the periodic sweep subscribes to the same change stream so a new or
    // amended timeInterval subscription wakes it, while the event-driven workers
    // own the sender.
    let interval_subscription_changes = subscription.clone();
    register_notification_workers(
        &task_supervisor,
        &storage.pool,
        &config.notifications.delivery,
        notification_processor,
        &entity,
        subscription,
        DeliveryDrainDrive {
            wake: notification_wake,
            pacer: Arc::clone(&pacer), // clone: every background pass of the tenant consults its one pacer.
        },
    )?;
    register_expiry_workers(&task_supervisor, config, storage, &services.state, &registration, &pacer)?;
    register_interval_notification(
        &task_supervisor,
        storage,
        transport,
        entity_subscriptions,
        interval_subscription_changes,
        config.notifications.interval_max_sleep.duration(),
        Arc::clone(&pacer), // clone: every background pass of the tenant consults its one pacer.
    )?;
    register_csource_notification_workers(
        &task_supervisor,
        storage,
        transport,
        csource_subscriptions,
        CsourceNotificationStreams {
            registrations: registration,
            subscriptions: csource_subscription,
        },
        config.notifications.interval_max_sleep.duration(),
        Arc::clone(&pacer), // clone: every background pass of the tenant consults its one pacer.
    )?;
    register_temporal_retention(&task_supervisor, Arc::clone(&storage.temporal_repo), &config.temporal)?; // clone: the sweep and the read stores share one repository.
    register_snapshot_workers(
        &task_supervisor,
        &storage.pool,
        PreparationRecovery {
            preparation: Arc::clone(&services.snapshot_preparation), // clone: the worker drives the same preparation the snapshot service schedules.
            cadence: config.snapshot.preparation_recovery.duration(),
            pacer,
        },
        transport,
        &config.notifications.delivery,
        SnapshotStatusWake {
            sweep_wake: snapshot_status_wake,
            receiver: snapshot_status_receiver,
        },
    )?;
    register_spatial_workers(&task_supervisor, config, storage)?;
    Ok(TaskSupervision {
        handles,
        health_shutdown,
        task_supervisor,
    })
}

#[cfg(test)]
mod tests {
    use andromeda_context_commit::context_change_event::ContextChangeEvent;

    /// `notifications.live_change_capacity` prices a slot of the live change
    /// channel at under half a kilobyte, allocated up front for every slot of
    /// every cached tenant runtime; an event that outgrows the price makes the
    /// documented memory cost a lie.
    #[test]
    fn a_live_change_slot_stays_within_its_documented_price() {
        assert!(
            size_of::<ContextChangeEvent>() <= 448,
            "a change event is {} bytes",
            size_of::<ContextChangeEvent>()
        );
    }
}
