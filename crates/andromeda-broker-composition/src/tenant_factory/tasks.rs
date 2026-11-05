//! Construction and supervision of one tenant's background tasks.

mod abandoned_drain;
mod columnstore_conversion;
mod csource_notification;
mod discovery_census;
mod entity_notification;
mod expiry;
mod maintenance;
mod notification_delivery;
mod notification_schedule;
mod notification_worker_failure;
mod periodic_evaluation;
mod pool_gauges;
mod retention;
mod snapshot;
mod subscription_cache_reconcile;
mod table_size_gauges;
mod temporal_retention;
mod temporal_sweep_failure;

use crate::{
    event::{entity_change_sink::QueuedEntityChangeSink, registration_change_sink::QueuedRegistrationChangeSink},
    tenant_factory::{
        TenantFactoryConfig,
        assembly_invariant::AssemblyInvariant,
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
                EntityNotificationStreams,
                IntervalDelivery,
                compose_notification_processor,
                compose_notification_query,
                register_interval_notification,
                register_notification_workers,
            },
            expiry::register_expiry_workers,
            maintenance::{register_entity_side_effects, register_spatial_workers},
            notification_delivery::{DeliveryDrainDrive, delivery_queue},
            periodic_evaluation::register_periodic_evaluations,
            pool_gauges::register_pool_gauges,
            snapshot::{PreparationRecovery, SnapshotStatusWake, register_snapshot_workers},
            table_size_gauges::register_table_size_gauges,
            temporal_retention::register_temporal_retention,
        },
    },
};
use andromeda_configuration::schema::temporal::storage::TemporalStorageConfig;
use andromeda_context_commit::{committer::ContextCommitter, subscription_key::SubscriptionKey};
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_notification::pending_change::{PendingChange, PendingCommits};
use andromeda_postgresql_context_store::{csource_subscription::repo::CsourceSubscriptionRepository, subscription::repo::SubscriptionRepository};
use andromeda_postgresql_temporal_store::{history_due::HistorySchedule, repo::TemporalRepository};
use andromeda_snapshot::status_delivery_signal::StatusDeliverySignal;
use andromeda_subscription::{
    change::{CsourceSubscriptionChange, SubscriptionChange},
    store::SubscriptionStore,
};
use andromeda_task_runtime::{
    keyed_slots::KeyedSlots,
    supervisor::TaskSupervisor,
    wake_trigger::{DrainWakeReceiver, WakeHandle},
};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use std::{num::NonZeroUsize, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, broadcast, mpsc, watch},
    task::JoinHandle,
};
use tokio_util::task::TaskTracker;

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
    pub(super) registration: QueuedRegistrationChangeSink,
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
        let registration = QueuedRegistrationChangeSink::new(registration);
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
    /// The tasks finishing the hand-off of committed changes to notification
    /// evaluation, which the runtime waits for after its tasks stop.
    pub(super) handoffs: Vec<TaskTracker>,
    /// The committed changes handed to live evaluation and not yet evaluated,
    /// which the runtime cache reads to spare a tenant with a backlog.
    pub(super) commits: Arc<PendingCommits>,
    pub(super) health_shutdown: Option<watch::Sender<bool>>,
    pub(super) task_supervisor: TaskSupervisor,
}

impl TaskSupervision {
    /// Gather what stops the tenant's tasks with the sinks they read from.
    fn new(
        handles: Vec<JoinHandle<()>>,
        health_shutdown: Option<watch::Sender<bool>>,
        task_supervisor: TaskSupervisor,
        entity: &QueuedEntityChangeSink,
        registration: &QueuedRegistrationChangeSink,
    ) -> TaskSupervision {
        TaskSupervision {
            handles,
            // clone: the runtime waits on the trackers the sinks spawn on.
            handoffs: vec![entity.handoffs().clone(), registration.handoffs().clone()],
            // clone: the runtime cache reads the count the sink and processor share.
            commits: Arc::clone(entity.commits()),
            health_shutdown,
            task_supervisor,
        }
    }
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

/// What one periodic sweep runs on: its pacing, and the tenant's evaluation
/// slots it hands each due Subscription's evaluation to.
#[derive(Clone)]
pub(super) struct PeriodicSweepDrive {
    /// How the sweep is paced and where it publishes its next due instant.
    pub(super) pacing: PeriodicPacing,
    /// The slots shared by both sweeps of the tenant.
    pub(super) evaluations: KeyedSlots<SubscriptionKey>,
}

/// Build the delivery signal the Snapshot service raises after enqueuing status
/// notifications outside the scheduled preparation sweep, wrapping a clone of the
/// status-delivery drain's wake handle.
pub(super) fn snapshot_delivery_signal(wake: WakeHandle) -> Arc<dyn StatusDeliverySignal> {
    Arc::new(snapshot::StatusDeliveryWaker::new(wake))
}

/// Start the MQTT pool's health probe in the unnamed default tenant, which
/// exists once per process, and return its handle with the sender that stops
/// it; every other tenant starts none.
fn spawn_mqtt_health_probe(storage: &TenantStorage, transport: &TransportDispatcher) -> (Vec<JoinHandle<()>>, Option<watch::Sender<bool>>) {
    if storage.tenant.is_unnamed_default() {
        let (shutdown, receiver) = watch::channel(false);
        (vec![transport.mqtt_pool().spawn_health_probe(receiver)], Some(shutdown))
    } else {
        (Vec::new(), None)
    }
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
    register_pool_gauges(&task_supervisor, storage.pool.clone(), &storage.tenant)?; // clone: PgPool is a shared handle sampled by the gauge task.
    // clone: PgPool is a shared handle sampled by the size-gauge task, which consults the tenant's one pacer.
    register_table_size_gauges(&task_supervisor, Arc::clone(&pacer), storage.pool.clone(), &storage.tenant)?;
    // clone: PgPool is a shared handle the census fold acquires from, and the fold consults the tenant's one pacer.
    register_discovery_census_fold(&task_supervisor, Arc::clone(&pacer), storage.pool.clone())?;
    // clone: every background pass of the tenant consults its one pacer.
    register_entity_side_effects(&task_supervisor, config, storage, writes, &entity, side_effect_wake, Arc::clone(&pacer))?;
    let (handles, health_shutdown) = spawn_mqtt_health_probe(storage, transport);
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
        EntityNotificationStreams {
            entity_changes: entity_receiver,
            handoffs: entity.handoffs().clone(),   // clone: the processor waits for the hand-offs the sink starts.
            commits: Arc::clone(entity.commits()), // clone: the processor reports the changes it gives up on.
            subscription_changes: subscription,
        },
        DeliveryDrainDrive {
            wake: notification_wake,
            pacer: Arc::clone(&pacer), // clone: every background pass of the tenant consults its one pacer.
        },
    )?;
    register_expiry_workers(&task_supervisor, config, storage, &services.state, &registration, &pacer)?;
    let periodic = PeriodicSweepDrive {
        pacing: periodic,
        evaluations: register_periodic_evaluations(&task_supervisor)?,
    };
    register_interval_notification(
        &task_supervisor,
        storage,
        IntervalDelivery {
            subscriptions: entity_subscriptions,
            notification_query,
            transport: Arc::clone(transport), // clone: every worker delivers through one dispatcher.
        },
        interval_subscription_changes,
        periodic.clone(), // clone: both periodic sweeps share the tenant's pacer, its due cell and its evaluation slots.
    )?;
    register_csource_notification_workers(
        &task_supervisor,
        storage,
        transport,
        csource_subscriptions,
        CsourceNotificationStreams {
            registrations: registration_receiver,
            handoffs: registration.handoffs().clone(), // clone: the worker waits for the hand-offs the sink starts.
            subscriptions: csource_subscription,
            shutdown_drain: config.notifications.evaluation.shutdown_drain(),
        },
        periodic,
    )?;
    register_history_sweeps(&task_supervisor, &storage.temporal_repo, &config.temporal.storage)?;
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
    Ok(TaskSupervision::new(handles, health_shutdown, task_supervisor, &entity, &registration))
}

/// Register the history sweeps, conversion and retention, under the horizons
/// `storage` names.
///
/// # Errors
///
/// Returns [`TenantAssemblyError::Invariant`] when `storage` names no usable
/// horizon or cadence, or when the supervisor refuses a task.
fn register_history_sweeps(
    supervisor: &TaskSupervisor,
    repository: &Arc<TemporalRepository>,
    storage: &TemporalStorageConfig,
) -> Result<(), TenantAssemblyError> {
    let history = HistorySchedule::try_from(&TemporalStoragePolicy::try_from(storage).map_err(AssemblyInvariant::from)?).map_err(AssemblyInvariant::from)?;
    register_temporal_retention(
        supervisor,
        Arc::clone(repository), // clone: the sweep and the read stores share one repository.
        history.retention,
        &storage.retention,
    )?;
    register_columnstore_conversion(
        supervisor,
        Arc::clone(repository), // clone: the sweep and the read stores share one repository.
        history.columnstore,
        &storage.columnstore,
    )
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
