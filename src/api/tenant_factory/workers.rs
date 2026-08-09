//! Construction and supervision of one tenant's background workers.

use crate::{
    config::runtime::RuntimeConfig,
    service::snapshot::store::SnapshotStore,
    worker::{
        csr_interval::CsrIntervalWorker,
        csr_notification::CsrNotificationWorker,
        data_expiration::DataExpirationWorker,
        interval::IntervalWorker,
        notification::NotificationWorker,
        outbox_delivery::OutboxDeliveryWorker,
        pending_side_effects::{PendingSideEffectsWorker, WorkerConfig},
        snapshot_expiration::SnapshotExpirationWorker,
        spatial_rebuild::periodic_rebuild,
        spatial_sync::SpatialSyncWorker,
        subscription_expiration::SubscriptionExpirationWorker,
    },
};
use andromeda_context_commit::{change_event::ChangeEvent, committer::ContextCommitter};
use andromeda_delivery::dispatch::TransportDispatcher;
use andromeda_entity::{
    EntityService,
    batch_activity::BatchActivityTracker,
    port::{
        discovery::EntityDiscoveryStore,
        expiry::EntityExpirySource,
        query::EntityQueryStore,
        reader::EntityReader,
        spatial_snapshot::EntitySpatialSnapshotSource,
    },
    query::EntityQueryService,
    side_effect_queue::SideEffectLeaseOwner,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_model::notification::CsrChangeEvent;
use andromeda_postgresql_context_store::{
    attribute_current::materializer::AttributeCurrentMaterializer,
    csource_subscription::repo::CsourceSubscriptionRepository,
    outbox::repo::OutboxRepository,
    pending_side_effects::repo::PendingSideEffectsRepository,
    subscription::repo::SubscriptionRepository,
};
use andromeda_registration::store::RegistrationStore;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_subscription::{
    cache::SubscriptionCache,
    change::{CsourceSubscriptionChange, SubscriptionChange},
};
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
pub(super) struct WorkerSupervisionInputs<'a> {
    pub(super) config: &'a RuntimeConfig,
    pub(super) pool: &'a PgPool,
    pub(super) tenant: TenantRef,
    pub(super) signals: TenantSignals,
    pub(super) materializer: AttributeCurrentMaterializer,
    pub(super) entity_policy: Arc<EntityService>,
    pub(super) entity_reader: Arc<dyn EntityReader>,
    pub(super) entity_query_store: Arc<dyn EntityQueryStore>,
    pub(super) entity_discovery_store: Arc<dyn EntityDiscoveryStore>,
    pub(super) entity_expiry_source: Arc<dyn EntityExpirySource>,
    pub(super) entity_spatial_snapshots: Arc<dyn EntitySpatialSnapshotSource>,
    pub(super) registration_store: Arc<dyn RegistrationStore>,
    pub(super) snapshot_store: Arc<dyn SnapshotStore>,
    pub(super) subscription_cache: Arc<SubscriptionCache>,
    pub(super) writes: Arc<dyn ContextCommitter>,
    pub(super) engine: JsonLdEngine,
    pub(super) spatial_index: Arc<SpatialEntityIndex>,
    pub(super) batch_activity: BatchActivityTracker,
    pub(super) transport: Arc<TransportDispatcher>,
}

/// Handles needed to stop the complete tenant worker set.
pub(super) struct WorkerSupervision {
    pub(super) handles: Vec<JoinHandle<()>>,
    pub(super) health_shutdown: Option<watch::Sender<bool>>,
}

/// Spawn every background worker owned by a tenant.
pub(super) fn supervise_workers(inputs: WorkerSupervisionInputs<'_>) -> WorkerSupervision {
    let WorkerSupervisionInputs {
        config,
        pool,
        tenant,
        signals,
        materializer,
        entity_policy,
        entity_reader,
        entity_query_store,
        entity_discovery_store,
        entity_expiry_source,
        entity_spatial_snapshots,
        registration_store,
        snapshot_store,
        subscription_cache,
        writes,
        engine,
        spatial_index,
        batch_activity,
        transport,
    } = inputs;
    let TenantSignals {
        entity,
        subscription,
        registration,
        csource_subscription,
    } = signals;
    let mut handles = vec![tokio::spawn(materializer.run())];

    if config.performance.batch_async_side_effect_threshold.is_some() {
        let lease_seconds = i32::try_from(config.performance.pending_side_effects_lease_secs.max(1)).unwrap_or(i32::MAX);
        let batch_size = i32::try_from(config.performance.pending_side_effects_batch_size.max(1)).unwrap_or(i32::MAX);
        let worker = PendingSideEffectsWorker::new(
            Arc::new(PendingSideEffectsRepository::new(pool)),
            Arc::clone(&writes),
            Some(entity.clone()),
            WorkerConfig {
                poll_interval: Duration::from_secs(config.performance.pending_side_effects_poll_secs.max(1)),
                lease_seconds,
                batch_size,
                max_attempts: config.performance.pending_side_effects_max_attempts.max(1),
                base_backoff: Duration::from_secs(config.performance.pending_side_effects_base_backoff_secs.max(1)),
                worker_id: SideEffectLeaseOwner::for_tenant(&config.info.source_id, uuid::Uuid::new_v4()),
            },
        );
        handles.push(tokio::spawn(worker.run()));
    }

    let health_shutdown = if tenant.is_unnamed_default() {
        let (shutdown, receiver) = watch::channel(false);
        handles.push(transport.mqtt_pool().spawn_health_probe(receiver));
        Some(shutdown)
    } else {
        None
    };
    let spatial_change = entity.subscribe();
    let notification_query = EntityQueryService::new(
        Arc::clone(&entity_reader),
        Arc::clone(&entity_query_store),
        engine.clone(),
        Arc::clone(&spatial_index),
        tenant.clone(), // clone: the query service owns its tenant partition.
    );
    let notification = NotificationWorker::new(
        entity.subscribe(),
        subscription.subscribe(),
        crate::worker::notification::NotificationWorkerDeps {
            sub_repo: SubscriptionRepository::new(pool),
            entity_reader: Arc::clone(&entity_reader),
            entity_query: notification_query,
            engine: engine.clone(),
            transport: Arc::clone(&transport),
            tenant: tenant.clone(),
            subscription_cache,
        },
    );
    handles.push(tokio::spawn(notification.run()));
    if config.outbox.enabled {
        handles.push(tokio::spawn(
            OutboxDeliveryWorker::new(Arc::new(OutboxRepository::new(pool)), &config.outbox).run(),
        ));
    }
    let expiration_interval = config.temporal.expiration_check_interval_secs;
    handles.push(tokio::spawn(
        SubscriptionExpirationWorker::new(
            SubscriptionRepository::new(pool),
            subscription,
            CsourceSubscriptionRepository::new(pool),
            csource_subscription.clone(),
            expiration_interval,
        )
        .run(),
    ));
    handles.push(tokio::spawn(
        IntervalWorker::new(
            SubscriptionRepository::new(pool),
            Arc::clone(&entity_reader),
            Arc::clone(&entity_discovery_store),
            engine,
            Arc::clone(&transport),
            tenant.clone(),
        )
        .run(),
    ));
    handles.push(tokio::spawn(
        CsrNotificationWorker::new(
            registration.subscribe(),
            csource_subscription.subscribe(),
            CsourceSubscriptionRepository::new(pool),
            Arc::clone(&registration_store),
            Arc::clone(&transport),
            tenant.clone(),
        )
        .run(),
    ));
    handles.push(tokio::spawn(
        DataExpirationWorker::new(entity_policy, entity_expiry_source, expiration_interval, batch_activity).run(),
    ));
    handles.push(tokio::spawn(SnapshotExpirationWorker::new(snapshot_store, expiration_interval).run()));
    handles.push(tokio::spawn(
        SpatialSyncWorker::new(
            spatial_change,
            Arc::clone(&spatial_index),
            entity_reader,
            tenant.clone(),
            Duration::from_secs(config.spatial.lag_log_interval_secs.max(1)),
        )
        .run(),
    ));
    if config.spatial.rebuild_interval_secs > 0 {
        handles.push(tokio::spawn(periodic_rebuild(
            spatial_index,
            entity_spatial_snapshots,
            tenant.clone(),
            Duration::from_secs(config.spatial.rebuild_interval_secs),
        )));
    }
    handles.push(tokio::spawn(
        CsrIntervalWorker::new(CsourceSubscriptionRepository::new(pool), registration_store, transport, tenant).run(),
    ));
    WorkerSupervision { handles, health_shutdown }
}
