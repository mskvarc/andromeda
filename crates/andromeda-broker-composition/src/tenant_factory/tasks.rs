//! Construction and supervision of one tenant's background tasks.

mod expiry;
mod maintenance;
mod notification;

use crate::{
    event::registration_change_sink::BroadcastRegistrationChangeSink,
    tenant_factory::{
        error::TenantAssemblyError,
        tasks::{
            expiry::{
                register_entity_expiry,
                register_registration_expiry,
                register_snapshot_expiry,
                register_snapshot_preparation,
                register_subscription_expiry,
            },
            maintenance::{register_entity_side_effect_drain, register_spatial_reconciliation, register_spatial_synchronization},
            notification::{
                register_csource_interval_notification,
                register_csource_notification_processor,
                register_interval_notification,
                register_notification_outbox,
                register_notification_processor,
            },
        },
    },
};
use andromeda_context_commit::{change_event::ChangeEvent, committer::ContextCommitter};
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_entity::{
    EntityService,
    change_sink::EntityChangeSink,
    config::EntityConfig,
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
    spatial_config::SpatialConfig,
    spatial_synchronization::SpatialSynchronizer,
};
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_notification::{
    csource_interval::CsourceIntervalNotificationOperation,
    interval::IntervalNotificationOperation,
    outbox_config::OutboxConfig,
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
use andromeda_registration::store::RegistrationStore;
use andromeda_snapshot::{executor::SnapshotPreparation, store::SnapshotStore};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change::{CsourceSubscriptionChange, SubscriptionChange},
    csource::CsrSubscriptionVariant,
    entity::EntitySubscriptionVariant,
    lifecycle::SubscriptionService,
    store::SubscriptionStore,
};
use andromeda_task_runtime::{supervisor::TaskSupervisor, wake_trigger::WakeHandle};
use andromeda_temporal::config::TemporalConfig;
use andromeda_tenancy::tenant_ref::TenantRef;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, watch},
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
pub(super) struct TaskSupervisionInputs<'a> {
    pub(super) entity_config: &'a EntityConfig,
    pub(super) info_config: &'a InfoConfig,
    pub(super) outbox_config: &'a OutboxConfig,
    pub(super) spatial_config: &'a SpatialConfig,
    pub(super) temporal_config: &'a TemporalConfig,
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
pub(super) struct TaskSupervision {
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
pub(super) fn supervise_tasks(inputs: TaskSupervisionInputs<'_>) -> Result<TaskSupervision, TenantAssemblyError> {
    let TaskSupervisionInputs {
        entity_config,
        info_config,
        outbox_config,
        spatial_config,
        temporal_config,
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

    if entity_config.batch_async_side_effect_threshold.is_some() {
        let lease_seconds = i32::try_from(entity_config.pending_side_effects_lease_secs).unwrap_or(i32::MAX);
        let batch_size = i32::try_from(entity_config.pending_side_effects_batch_size).unwrap_or(i32::MAX);
        let change_sink = Arc::new(crate::event::entity_change_sink::BroadcastEntityChangeSink::new(entity.clone())) as Arc<dyn EntityChangeSink>;
        let drain = EntitySideEffectDrain::new(
            Arc::new(PendingSideEffectsRepository::new(pool)),
            Arc::clone(&writes),
            Some(change_sink),
            EntitySideEffectDrainConfig {
                lease_seconds,
                batch_size,
                max_attempts: entity_config.pending_side_effects_max_attempts,
                base_backoff: Duration::from_secs(entity_config.pending_side_effects_base_backoff_secs),
                lease_owner: SideEffectLeaseOwner::for_tenant(&info_config.source_id, uuid::Uuid::new_v4()),
            },
        );
        drain_wakes.push(register_entity_side_effect_drain(
            &task_supervisor,
            Arc::new(drain),
            Duration::from_secs(entity_config.pending_side_effects_poll_secs),
            Duration::from_secs(entity_config.pending_side_effects_lease_secs),
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
        if outbox_config.enabled {
            EntityNotificationDelivery::Durable
        } else {
            EntityNotificationDelivery::Live
        },
    )?;
    if outbox_config.enabled {
        let store = Arc::new(OutboxRepository::new(pool)) as Arc<dyn OutboxStore>;
        let delivery = notification_processor as Arc<dyn NotificationOutboxDelivery>;
        let drain = NotificationOutboxDrain::new(store, delivery, outbox_config).map_err(|error| TenantAssemblyError::invariant(error.to_string()))?;
        drain_wakes.push(register_notification_outbox(&task_supervisor, Arc::new(drain), outbox_config)?);
    }
    let expiration_interval = temporal_config.expiration_check_interval_secs;
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
        entity_config.expiry_batch_size,
    )?;
    register_snapshot_preparation(&task_supervisor, snapshot_preparation)?;
    register_snapshot_expiry(&task_supervisor, snapshot_store, expiration_interval)?;
    register_spatial_synchronization(
        &task_supervisor,
        entity,
        Arc::new(SpatialSynchronizer::new(Arc::clone(&spatial_index), Arc::clone(&entity_reader), tenant.clone())),
    )?;
    register_spatial_reconciliation(
        &task_supervisor,
        spatial_index,
        entity_spatial_snapshots,
        tenant.clone(),
        Duration::from_secs(spatial_config.rebuild_interval_secs),
    )?;
    register_csource_interval_notification(
        &task_supervisor,
        Arc::new(CsourceIntervalNotificationOperation::new(
            csource_subscription_store,
            registration_store,
            transport,
            tenant,
        )),
    )?;
    Ok(TaskSupervision {
        handles,
        health_shutdown,
        task_supervisor,
        drain_wakes,
    })
}
