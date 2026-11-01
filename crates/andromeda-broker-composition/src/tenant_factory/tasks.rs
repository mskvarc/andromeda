//! Construction and supervision of one tenant's background tasks.

mod columnstore_conversion;
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
mod temporal_sweep_failure;

use crate::{
    event::entity_change_sink::QueuedEntityChangeSink,
    tenant_factory::{
        TenantFactoryConfig,
        background_pacer::BackgroundPacer,
        error::TenantAssemblyError,
        periodic_due::PeriodicDue,
        services::ConstructedServices,
        storage::TenantStorage,
        tasks::{
            columnstore_conversion::register_columnstore_conversion,
            csource_notification::{CsourceNotificationStreams, register_csource_notification_workers},
            discovery_census::register_discovery_census_fold,
            entity_notification::{
                IntervalDelivery,
                compose_notification_processor,
                compose_notification_query,
                register_interval_notification,
                register_notification_workers,
            },
            expiry::register_expiry_workers,
            maintenance::{register_entity_side_effects, register_spatial_workers},
            notification_delivery::{DeliveryDrainDrive, delivery_queue},
            pool_gauges::register_pool_gauges,
            snapshot::{PreparationRecovery, SnapshotStatusWake, register_snapshot_workers},
            table_size_gauges::register_table_size_gauges,
            temporal_retention::register_temporal_retention,
        },
    },
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_notification::pending_change::{PendingChange, PendingCommits};
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
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, broadcast, mpsc, watch},
    task::JoinHandle,
};

/// Committed registration changes queued for context source notification
/// evaluation before a further write waits for room.
const REGISTRATION_QUEUE_CAPACITY: NonZeroUsize = NonZeroUsize::MIN.saturating_add(1023);

/// Change channels shared by tenant services and their workers.
pub(super) struct TenantSignals {
    /// Where committed entity changes queue for live notification evaluation.
    pub(super) entity: QueuedEntityChangeSink,
    /// The queue's one reader, held across restarts of the notification worker
    /// so a change queued while it restarts is still evaluated.
    pub(super) entity_receiver: Arc<Mutex<mpsc::Receiver<PendingChange>>>,
    pub(super) subscription: broadcast::Sender<SubscriptionChange>,
    /// Where committed registration changes queue for context source
    /// notification evaluation.
    pub(super) registration: mpsc::Sender<CsrChangeEvent>,
    /// The registration queue's one reader, held across restarts of the
    /// context source notification worker so a change queued while it
    /// restarts is still evaluated.
    pub(super) registration_receiver: Arc<Mutex<mpsc::Receiver<CsrChangeEvent>>>,
    pub(super) csource_subscription: broadcast::Sender<CsourceSubscriptionChange>,
}

impl TenantSignals {
    /// Open bounded change channels for a tenant, queueing up to
    /// `entity_capacity` committed entity changes for live evaluation.
    pub(super) fn new(entity_capacity: NonZeroUsize) -> TenantSignals {
        let (entity, entity_receiver) = mpsc::channel(entity_capacity.get());
        let entity = QueuedEntityChangeSink::new(entity, Arc::new(PendingCommits::default()));
        let (subscription, _) = broadcast::channel(1024);
        let (registration, registration_receiver) = mpsc::channel(REGISTRATION_QUEUE_CAPACITY.get());
        let (csource_subscription, _) = broadcast::channel(1024);
        TenantSignals {
            entity,
            entity_receiver: Arc::new(Mutex::new(entity_receiver)),
            subscription,
            registration,
            registration_receiver: Arc::new(Mutex::new(registration_receiver)),
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
    /// How the periodic notification sweeps are paced, and where they
    /// publish when they are next due.
    pub(super) periodic: PeriodicPacing,
}

/// How one tenant's periodic sweeps are paced, and where each publishes the
/// instant it is next due.
#[derive(Clone)]
pub(super) struct PeriodicPacing {
    /// The longest a sweep sleeps with nothing waking it.
    pub(super) ceiling: Duration,
    /// The tenant's one pacer, which every background pass consults.
    pub(super) pacer: Arc<BackgroundPacer>,
    /// Where each sweep publishes its next due instant for the runtime cache.
    pub(super) due: Arc<PeriodicDue>,
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
        periodic,
    } = channels;
    let TenantSignals {
        entity,
        entity_receiver,
        subscription,
        registration,
        registration_receiver,
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
    let notification_query = compose_notification_query(storage, &services.state.forwarding, config);
    let notification_processor = compose_notification_processor(
        storage,
        Arc::clone(&notification_query), // clone: change-driven and periodic notifications resolve their joins through one service.
        transport,
        &entity_subscriptions,
        Arc::clone(entity.commits()), // clone: the sink holds what the processor releases.
    );
    // clone: the periodic sweep subscribes to the same change stream so a new or
    // amended timeInterval subscription wakes it, while the event-driven workers
    // own the sender.
    let interval_subscription_changes = subscription.clone();
    register_notification_workers(
        &task_supervisor,
        delivery_queue(storage),
        &config.notifications,
        notification_processor,
        entity_receiver,
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
        IntervalDelivery {
            subscriptions: entity_subscriptions,
            notification_query,
            transport: Arc::clone(transport), // clone: every worker delivers through one dispatcher.
        },
        interval_subscription_changes,
        periodic.clone(), // clone: both periodic sweeps consult the tenant's one pacer and publish into its one cell.
    )?;
    register_csource_notification_workers(
        &task_supervisor,
        storage,
        transport,
        csource_subscriptions,
        CsourceNotificationStreams {
            registrations: registration_receiver,
            subscriptions: csource_subscription,
        },
        periodic,
    )?;
    register_temporal_retention(&task_supervisor, Arc::clone(&storage.temporal_repo), &config.temporal)?; // clone: the sweep and the read stores share one repository.
    register_columnstore_conversion(&task_supervisor, Arc::clone(&storage.temporal_repo), &config.temporal)?; // clone: the sweep and the read stores share one repository.
    register_snapshot_workers(
        &task_supervisor,
        &storage.pool,
        PreparationRecovery {
            preparation: Arc::clone(&services.snapshot_preparation), // clone: the worker drives the same preparation the snapshot service schedules.
            cadence: config.snapshot.preparation.recovery_interval.duration(),
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
    use andromeda_notification::pending_change::PendingChange;

    /// `notifications.live_change_capacity` prices a slot of the live change
    /// channel at under half a kilobyte, allocated up front for every slot of
    /// every cached tenant runtime; an event that outgrows the price makes the
    /// documented memory cost a lie.
    #[test]
    fn a_live_change_slot_stays_within_its_documented_price() {
        assert!(size_of::<PendingChange>() <= 448, "a held change event is {} bytes", size_of::<PendingChange>());
    }
}
