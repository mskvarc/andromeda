//! Construction and supervision of one tenant's background tasks.

mod csource_notification;
mod entity_notification;
mod expiry;
mod maintenance;
mod notification_schedule;
mod pool_gauges;
mod retention;
mod table_size_gauges;
mod temporal_retention;

use crate::tenant_factory::{
    TenantFactoryConfig,
    error::TenantAssemblyError,
    services::ConstructedServices,
    storage::TenantStorage,
    tasks::{
        csource_notification::{CsourceNotificationStreams, register_csource_notification_workers},
        entity_notification::{compose_notification_processor, register_interval_notification, register_notification_workers},
        expiry::{register_expiry_workers, register_snapshot_workers},
        maintenance::{register_entity_side_effects, register_spatial_workers},
        pool_gauges::register_pool_gauges,
        table_size_gauges::register_table_size_gauges,
        temporal_retention::register_temporal_retention,
    },
};
use andromeda_context_commit::{committer::ContextCommitter, context_change_event::ContextChangeEvent};
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_postgresql_context_store::{csource_subscription::repo::CsourceSubscriptionRepository, subscription::repo::SubscriptionRepository};
use andromeda_subscription::{
    change::{CsourceSubscriptionChange, SubscriptionChange},
    store::SubscriptionStore,
};
use andromeda_task_runtime::{supervisor::TaskSupervisor, wake_trigger::WakeHandle};
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
    pub(super) drain_wakes: Vec<WakeHandle>,
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
    signals: TenantSignals,
    writes: &Arc<dyn ContextCommitter>,
    transport: &Arc<TransportDispatcher>,
    task_supervisor: TaskSupervisor,
) -> Result<TaskSupervision, TenantAssemblyError> {
    let TenantSignals {
        entity,
        subscription,
        registration,
        csource_subscription,
    } = signals;
    let mut handles: Vec<JoinHandle<()>> = Vec::new();
    let mut drain_wakes = Vec::new();
    register_pool_gauges(&task_supervisor, storage.pool.clone(), &storage.tenant)?; // clone: PgPool is a shared handle sampled by the gauge task.
    register_table_size_gauges(&task_supervisor, storage.pool.clone(), &storage.tenant)?; // clone: PgPool is a shared handle sampled by the size-gauge task.
    drain_wakes.push(register_entity_side_effects(&task_supervisor, config, storage, writes, &entity)?);
    let health_shutdown = if storage.tenant.is_unnamed_default() {
        let (shutdown, receiver) = watch::channel(false);
        handles.push(transport.mqtt_pool().spawn_health_probe(receiver));
        Some(shutdown)
    } else {
        None
    };
    let entity_subscriptions = Arc::new(SubscriptionRepository::new(&storage.pool)) as Arc<dyn SubscriptionStore>;
    let csource_subscriptions = Arc::new(CsourceSubscriptionRepository::new(&storage.pool)) as Arc<dyn SubscriptionStore>;
    let notification_processor = compose_notification_processor(storage, &services.state.forwarding, transport, &entity_subscriptions);
    drain_wakes.extend(register_notification_workers(
        &task_supervisor,
        &storage.pool,
        &config.notifications.delivery,
        notification_processor,
        &entity,
        subscription,
    )?);
    register_expiry_workers(&task_supervisor, config, storage, &services.state, &registration)?;
    register_interval_notification(&task_supervisor, storage, transport, entity_subscriptions)?;
    register_csource_notification_workers(
        &task_supervisor,
        storage,
        transport,
        csource_subscriptions,
        CsourceNotificationStreams {
            registrations: registration,
            subscriptions: csource_subscription,
        },
    )?;
    register_temporal_retention(&task_supervisor, Arc::clone(&storage.temporal_repo), &config.temporal)?; // clone: the sweep and the read stores share one repository.
    register_snapshot_workers(
        &task_supervisor,
        &storage.pool,
        Arc::clone(&services.snapshot_preparation), // clone: the worker drives the same preparation the snapshot service schedules.
        transport,
        &config.notifications.delivery,
    )?;
    register_spatial_workers(&task_supervisor, config, storage, entity)?;
    Ok(TaskSupervision {
        handles,
        health_shutdown,
        task_supervisor,
        drain_wakes,
    })
}
