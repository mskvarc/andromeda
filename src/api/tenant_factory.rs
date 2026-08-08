//! Building one tenant's runtime, as the tenancy crate's factory port.
//!
//! The composition root implements this and nothing else can: building a
//! tenant's runtime means naming every service, repository and worker the
//! broker has, which is exactly what `andromeda-tenancy` must not see.

use crate::{
    api::state::AndromedaState,
    core::{batch_activity::BatchActivityTracker, config::RuntimeConfig, error::AndromedaError},
    event::{
        change_event::ChangeEvent,
        csr_change_event::CsrChangeEvent,
        subscription_change::{CsourceSubscriptionChange, SubscriptionChange},
    },
    repository::atomic_write::anchor::PostgresAnchorStore,
    service::{
        csource_registration::{CsourceRegistrationService, store::RegistrationStore},
        csource_subscription::CsourceSubscriptionService,
        entity::{
            EntityService,
            OutboxWriteConfig,
            batch::execution::BatchExecutionConfig,
            discovery::EntityDiscoveryService,
            query::EntityQueryService,
            store::EntityStore,
            write_coordinator::{AnchoredWriteCoordinator, WriteCoordinator},
        },
        entity_map::EntityMapService,
        jsonld_context::JsonLdContextService,
        snapshot::store::SnapshotStore,
        subscription::SubscriptionService,
        subscription_cache::SubscriptionCache,
        temporal::TemporalService,
    },
    worker::{
        csr_interval::CsrIntervalWorker,
        csr_notification::CsrNotificationWorker,
        data_expiration::DataExpirationWorker,
        interval::IntervalWorker,
        notification::NotificationWorker,
        outbox_delivery::OutboxDeliveryWorker,
        pending_side_effects::PendingSideEffectsWorker,
        snapshot_expiration::SnapshotExpirationWorker,
        spatial_rebuild::periodic_rebuild,
        spatial_sync::SpatialSyncWorker,
        subscription_expiration::SubscriptionExpirationWorker,
    },
};
use andromeda_delivery::{
    dispatch::TransportDispatcher,
    http::config::HttpTransportConfig,
    mqtt::{config::MqttPoolConfig, pool::MqttTransportPool},
    stream::{registry::ConnectionRegistry, sse::SseNotificationEvent, ws::WsNotificationMessage},
};
use andromeda_postgresql_context_store::{
    attribute_current::materializer::AttributeCurrentMaterializer,
    attribute_names::AttributeNameCache,
    csource_registration::repo::CsourceRegistrationRepository,
    csource_subscription::repo::CsourceSubscriptionRepository,
    entity::repo::EntityRepository,
    entity_map::InMemoryEntityMapStore,
    jsonld_context::repo::JsonLdContextRepository,
    outbox::repo::OutboxRepository,
    pending_side_effects::repo::PendingSideEffectsRepository,
    snapshot::repo::SnapshotRepository,
    spatial_warmup::warmup_location,
    subscription::repo::SubscriptionRepository,
    tenant_database::{DatabaseUrl, configure_pool_options, migrate_tenant_db},
};
use andromeda_postgresql_query::{attribute_name_lookup::AttributeNameLookup, projection_barrier::ProjectionBarrier};
use andromeda_postgresql_temporal_store::{recorder::TemporalRecorder, repo::TemporalRepository, timescale_features::TimescaleFeatures};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_tenancy::{
    database_name::TenantDatabaseName,
    error::TenancyError,
    state_factory::{TenantRuntime, TenantStateFactory},
    tenant_ref::TenantRef,
};
use async_trait::async_trait;
use sqlx::PgPool;
use std::sync::{Arc, OnceLock};
use tokio::task::JoinHandle;
use tracing::debug;

/// Everything one tenant's runtime holds open.
///
/// Field order is the shutdown order the `Drop` glue would otherwise pick at
/// random; [`TenantRuntime::shutdown`] releases them explicitly instead,
/// because closing a pool that workers are still writing through is a
/// different thing from closing one they have stopped using.
pub struct BrokerTenantRuntime {
    /// The background workers running on this tenant's behalf.
    worker_handles: Vec<JoinHandle<()>>,
    /// The notification transports this tenant delivers through.
    transport: Arc<TransportDispatcher>,
    /// The tenant's database pool.
    pool: PgPool,
}

#[async_trait]
impl TenantRuntime for BrokerTenantRuntime {
    async fn shutdown(self) {
        self.transport.shutdown().await;
        for handle in self.worker_handles {
            handle.abort();
        }
        // Closed rather than dropped: PostgreSQL reclaims the connection slots
        // immediately instead of waiting out `idle_timeout`, which is what
        // makes evicting a tenant worth doing at all.
        self.pool.close().await;
    }
}

/// Builds a tenant's runtime from the broker's configuration.
pub struct BrokerTenantFactory {
    /// Every tunable a tenant's services read.
    config: Arc<RuntimeConfig>,
    /// The URL each tenant's database name is substituted into.
    database_url: DatabaseUrl,
    /// The MQTT pool the unnamed default tenant opened.
    ///
    /// Recorded here because `/health` reports on MQTT delivery and the pool
    /// belongs to a runtime the health router has no handle on. Written once:
    /// the unnamed default is the one tenant that is never evicted.
    default_mqtt_pool: OnceLock<Arc<MqttTransportPool>>,
}

impl BrokerTenantFactory {
    /// A factory over the broker's configuration.
    pub const fn new(config: Arc<RuntimeConfig>, database_url: DatabaseUrl) -> BrokerTenantFactory {
        BrokerTenantFactory {
            config,
            database_url,
            default_mqtt_pool: OnceLock::new(),
        }
    }

    /// The MQTT pool of the default tenant's runtime, once it has been built.
    pub fn default_mqtt_pool(&self) -> Option<Arc<MqttTransportPool>> {
        self.default_mqtt_pool.get().map(Arc::clone)
    }
}

#[async_trait]
impl TenantStateFactory for BrokerTenantFactory {
    type State = AndromedaState;
    type Runtime = BrokerTenantRuntime;

    async fn build(&self, tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<AndromedaState>, BrokerTenantRuntime), TenancyError> {
        let db_url = self.database_url.with_db_name(database.as_str());
        let (state, worker_handles, transport, pool) = build_tenant_state(&self.config, &db_url, tenant.clone()) // clone: the state owns its tenant
            .await
            .map_err(|error| TenancyError::RuntimeUnavailable {
                tenant: tenant.clone(), // clone: the error outlives this borrow
                detail: error.to_string(),
            })?;

        if tenant.is_unnamed_default() && self.default_mqtt_pool.set(Arc::clone(transport.mqtt_pool())).is_err() {
            debug!("the default tenant's MQTT pool was already recorded; /health keeps reporting on the first");
        }

        Ok((
            Arc::new(state),
            BrokerTenantRuntime {
                worker_handles,
                transport,
                pool,
            },
        ))
    }
}

/// Build every service, repository and worker one tenant needs.
///
/// Private to the factory: it is the factory's whole implementation, and the
/// route table has no business calling it.
async fn build_tenant_state(
    config: &RuntimeConfig,
    db_url: &str,
    tenant: TenantRef,
) -> Result<(AndromedaState, Vec<JoinHandle<()>>, Arc<TransportDispatcher>, sqlx::PgPool), AndromedaError> {
    let max_connections = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections,
        TenantRef::Named(_) => config.tenancy.tenant_max_connections,
    };
    let pool = configure_pool_options(max_connections, config.database.synchronous_commit.as_postgres_setting())
        .connect(db_url)
        .await?;

    migrate_tenant_db(&pool).await?;

    let timescale_features = TimescaleFeatures::detect(&pool)
        .await
        .map_err(|e| AndromedaError::internal(format!("Failed to probe TimescaleDB features: {}", e)))?;
    tracing::info!(
        has_continuous_aggregates = timescale_features.has_continuous_aggregates,
        "Detected TimescaleDB capabilities"
    );

    let (change_tx, _) = tokio::sync::broadcast::channel::<ChangeEvent>(config.spatial.change_channel_capacity.max(1));
    // Subscription / CSR / CSR-subscription change channels carry burst
    // traffic during mass updates: an operator uploading N subscriptions in
    // quick succession, or a federation refresh touching many CSRs. At 256
    // the buffer would overflow and force subscription caches to fall back
    // to a full reload; 1024 holds the burst without forcing reloads while
    // still bounding memory use.
    let (sub_change_tx, _) = tokio::sync::broadcast::channel::<SubscriptionChange>(1024);
    let (csr_change_tx, _) = tokio::sync::broadcast::channel::<CsrChangeEvent>(1024);
    let (csr_sub_change_tx, _) = tokio::sync::broadcast::channel::<CsourceSubscriptionChange>(1024);
    let mut worker_handles = Vec::new();

    let context_repo = Arc::new(JsonLdContextRepository::new(&pool));
    let engine = config.jsonld.build_engine(Arc::clone(&context_repo))?;

    if let Some(hit_counter) = engine.hit_counter() {
        let flush_repo = Arc::clone(&context_repo);
        let interval = std::time::Duration::from_secs(config.performance.context_hit_flush_secs.max(1));
        worker_handles.push(tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            // First tick fires immediately; skip it to avoid a flush before
            // any hits accumulate.
            ticker.tick().await;
            loop {
                ticker.tick().await;
                let pending = hit_counter.drain();
                if pending.is_empty() {
                    continue;
                }
                if let Err(error) = flush_repo.flush_hits(&pending).await {
                    tracing::warn!(error = %error, count = pending.len(), event = "context_hit_flush_failed");
                }
            }
        }));
    }

    let batch_activity = BatchActivityTracker::new();

    let pool_size = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections as usize,
        TenantRef::Named(_) => config.tenancy.tenant_max_connections as usize,
    };
    let batch_concurrency = config.performance.batch_concurrency.unwrap_or_else(|| usize::min(pool_size, 25)).max(1);
    let batch_chunk_concurrency = config.performance.batch_chunk_concurrency.unwrap_or(4).max(1);
    let batch_chunk_write_concurrency = config.performance.batch_chunk_write_concurrency.unwrap_or(4).max(1);

    // One dictionary cache per tenant, shared by the temporal write path (which
    // mints missing entries) and the q-filter read path (which must not).
    let attribute_name_cache = Arc::new(AttributeNameCache::new(&pool));
    if config.performance.preload_attribute_names
        && let Err(error) = attribute_name_cache.preload().await
    {
        tracing::warn!(error = %error, event = "attribute_names_preload_failed");
    }
    let pool_arc = Arc::new(pool.clone()); // clone: Arc-wrap the per-tenant pool for the materializer worker
    let (materializer, attribute_current_queue, attribute_current_barrier) = AttributeCurrentMaterializer::new(
        Arc::clone(&pool_arc),
        std::time::Duration::from_secs(config.spatial.lag_log_interval_secs.max(1)),
    );
    if let Err(e) = materializer.reconcile_on_startup().await {
        tracing::warn!(error = %e, event = "attribute_current_reconcile_failed");
    }
    worker_handles.push(tokio::spawn(materializer.run()));

    let bulk_write_mode = match config.performance.bulk_write_mode {
        crate::core::config::BulkWriteMode::Unnest => andromeda_postgresql_temporal_store::bulk_write_mode::BulkWriteMode::Unnest,
        crate::core::config::BulkWriteMode::CopyStaging => andromeda_postgresql_temporal_store::bulk_write_mode::BulkWriteMode::CopyStaging,
    };
    let temporal_repo = Arc::new(
        TemporalRepository::new(&pool, bulk_write_mode, Arc::clone(&attribute_name_cache) as Arc<dyn AttributeNameLookup>)
            .with_features(timescale_features)
            .with_barrier(Arc::new(attribute_current_barrier.clone()) as Arc<dyn ProjectionBarrier>), // clone: shared with entity store and temporal service
    );

    // One entity store per tenant. It owns the attribute-name dictionary and
    // the projection barrier because both describe how *this* store pushes a
    // `q` predicate down; nothing above the port sees either.
    let entity_store: Arc<dyn EntityStore> = Arc::new(EntityRepository::new(
        &pool,
        Arc::clone(&attribute_name_cache) as Arc<dyn AttributeNameLookup>,
        Arc::new(attribute_current_barrier.clone()) as Arc<dyn ProjectionBarrier>, // clone: the temporal service still waits on the same watermark until the temporal read path moves behind its port
    ));

    let registration_store: Arc<dyn RegistrationStore> = Arc::new(CsourceRegistrationRepository::new(&pool));
    let snapshot_store: Arc<dyn SnapshotStore> = Arc::new(SnapshotRepository::new(&pool));

    let subscription_cache = Arc::new(SubscriptionCache::new(tenant.clone()));
    if let Err(e) = subscription_cache.reload(&SubscriptionRepository::new(&pool)).await {
        tracing::error!("Failed to load subscription cache: {}", e);
    }

    let spatial_index = Arc::new(SpatialEntityIndex::new());
    if let Err(e) = warmup_location(&spatial_index, &pool, &tenant).await {
        tracing::warn!("spatial warmup failed: {}", e);
    }

    let temporal_recorder = Arc::new(TemporalRecorder::new(Arc::clone(&temporal_repo)));

    // Sole owner of the materializer producer handle: every write path reaches
    // the projection through an atomic write, so no other service needs one.
    // The anchor also owns the bulk-write mode, which is a statement shape
    // rather than a caller's choice.
    let writes: Arc<dyn WriteCoordinator> = Arc::new(AnchoredWriteCoordinator::new(PostgresAnchorStore::new(
        &pool,
        Arc::clone(&temporal_recorder),
        config.performance.bulk_write_mode,
        attribute_current_queue,
    )));

    if config.performance.batch_async_side_effect_threshold.is_some() {
        let worker = PendingSideEffectsWorker::new(
            Arc::new(PendingSideEffectsRepository::new(&pool)),
            Arc::clone(&writes),
            Some(change_tx.clone()),
            crate::worker::pending_side_effects::WorkerConfig {
                poll_interval: std::time::Duration::from_secs(config.performance.pending_side_effects_poll_secs.max(1)),
                lease_seconds: i32::try_from(config.performance.pending_side_effects_lease_secs.max(1)).unwrap_or(i32::MAX),
                batch_size: i32::try_from(config.performance.pending_side_effects_batch_size.max(1)).unwrap_or(i32::MAX),
                max_attempts: config.performance.pending_side_effects_max_attempts.max(1),
                base_backoff: std::time::Duration::from_secs(config.performance.pending_side_effects_base_backoff_secs.max(1)),
                worker_id: format!("{}-{}", config.info.source_identity, uuid::Uuid::new_v4()),
            },
        );
        worker_handles.push(tokio::spawn(worker.run()));
    }

    let entity_service = Arc::new(EntityService::new(
        Arc::clone(&entity_store),
        Arc::clone(&temporal_recorder),
        Arc::clone(&writes),
        engine.clone(),
        Some(change_tx.clone()),
        BatchExecutionConfig {
            concurrency: batch_concurrency,
            chunk_concurrency: batch_chunk_concurrency,
            chunk_write_concurrency: batch_chunk_write_concurrency,
            chunk_size: config.performance.batch_chunk_size.max(1),
            large_threshold: config.performance.batch_large_threshold.max(1),
            prepare_blocking_threshold: config.performance.batch_prepare_blocking_threshold.max(1),
            async_side_effect_threshold: config.performance.batch_async_side_effect_threshold,
        },
        batch_activity.clone(),
        if config.outbox.enabled {
            Some(OutboxWriteConfig {
                subscription_cache: Arc::clone(&subscription_cache),
            })
        } else {
            None
        },
    ));
    let temporal_service = Arc::new(TemporalService::new(
        Arc::clone(&entity_store),
        Arc::clone(&temporal_repo) as Arc<dyn andromeda_postgresql_temporal_store::store::TemporalHistoryReader>,
        Arc::clone(&temporal_repo) as Arc<dyn andromeda_postgresql_temporal_store::store::TemporalMutator>,
        Arc::clone(&temporal_recorder),
        Arc::clone(&writes),
        engine.clone(),
        config.temporal.default_last_n,
    ));
    let context_service = Arc::new(JsonLdContextService::new(&pool, engine.clone()));
    let subscription_service = Arc::new(SubscriptionService::new(&pool, engine.clone(), Some(sub_change_tx.clone())));
    let csr_service = Arc::new(CsourceRegistrationService::new(
        Arc::clone(&registration_store),
        engine.clone(),
        Some(csr_change_tx.clone()),
    ));
    let csr_subscription_service = Arc::new(CsourceSubscriptionService::new(&pool, engine.clone(), Some(csr_sub_change_tx.clone())));

    let sse_registry: Arc<ConnectionRegistry<SseNotificationEvent>> = Arc::new(ConnectionRegistry::new());
    let ws_registry: Arc<ConnectionRegistry<WsNotificationMessage>> = Arc::new(ConnectionRegistry::new());
    let transport = Arc::new(
        TransportDispatcher::new(
            HttpTransportConfig::default(),
            MqttPoolConfig::default(),
            Arc::clone(&sse_registry),
            Arc::clone(&ws_registry),
        )
        .map_err(|e| crate::core::error::AndromedaError::internal(format!("Failed to create transport dispatcher: {e}")))?,
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Shared health probe: run only for the unnamed default runtime.
    if matches!(tenant, TenantRef::UnnamedDefault) {
        worker_handles.push(transport.mqtt_pool().spawn_health_probe(shutdown_rx));
    }

    // Keep shutdown sender alive so an eventual graceful shutdown can signal the probe.
    let _shutdown_tx = shutdown_tx;

    let spatial_change_rx = change_tx.subscribe();
    let notification_entity_query = EntityQueryService::new(Arc::clone(&entity_store), engine.clone(), Arc::clone(&spatial_index), tenant.clone());
    let notification_worker = NotificationWorker::new(
        change_tx.subscribe(),
        sub_change_tx.subscribe(),
        crate::worker::notification::NotificationWorkerDeps {
            sub_repo: SubscriptionRepository::new(&pool),
            entity_repo: Arc::clone(&entity_store),
            entity_query: notification_entity_query,
            engine: engine.clone(),
            transport: transport.clone(),
            tenant: tenant.clone(),
            subscription_cache: Arc::clone(&subscription_cache),
        },
    );
    worker_handles.push(tokio::spawn(notification_worker.run()));

    if config.outbox.enabled {
        let outbox_worker = OutboxDeliveryWorker::new(Arc::new(OutboxRepository::new(&pool)), &config.outbox);
        worker_handles.push(tokio::spawn(outbox_worker.run()));
    }

    let expiration_interval = config.temporal.expiration_check_interval_secs;
    let subscription_expiration_worker = SubscriptionExpirationWorker::new(
        SubscriptionRepository::new(&pool),
        sub_change_tx,
        CsourceSubscriptionRepository::new(&pool),
        csr_sub_change_tx.clone(),
        expiration_interval,
    );
    worker_handles.push(tokio::spawn(subscription_expiration_worker.run()));

    let interval_worker = IntervalWorker::new(
        SubscriptionRepository::new(&pool),
        Arc::clone(&entity_store),
        engine.clone(),
        transport.clone(),
        tenant.clone(),
    );
    worker_handles.push(tokio::spawn(interval_worker.run()));

    let csr_notification_worker = CsrNotificationWorker::new(
        csr_change_tx.subscribe(),
        csr_sub_change_tx.subscribe(),
        CsourceSubscriptionRepository::new(&pool),
        Arc::clone(&registration_store),
        transport.clone(),
        tenant.clone(),
    );
    worker_handles.push(tokio::spawn(csr_notification_worker.run()));

    // Combined entity and attribute expiration worker (§4.22).
    let data_expiration_worker = DataExpirationWorker::new(
        Arc::clone(&entity_store),
        Arc::clone(&writes),
        Some(change_tx),
        expiration_interval,
        batch_activity,
    );
    worker_handles.push(tokio::spawn(data_expiration_worker.run()));

    // Snapshot expiration worker (NGSI-LD 5.16 -- clean up expired snapshots)
    let snapshot_expiration_worker = SnapshotExpirationWorker::new(Arc::clone(&snapshot_store), expiration_interval);
    worker_handles.push(tokio::spawn(snapshot_expiration_worker.run()));

    // Spatial index: event-driven incremental sync + periodic bulk rebuild.
    let spatial_sync_worker = SpatialSyncWorker::new(
        spatial_change_rx,
        Arc::clone(&spatial_index),
        Arc::clone(&entity_store),
        tenant.clone(),
        std::time::Duration::from_secs(config.spatial.lag_log_interval_secs.max(1)),
    );
    worker_handles.push(tokio::spawn(spatial_sync_worker.run()));

    let rebuild_secs = config.spatial.rebuild_interval_secs;
    if rebuild_secs > 0 {
        let rebuild_index = Arc::clone(&spatial_index);
        let rebuild_pool = pool.clone();
        let rebuild_tenant = tenant.clone();
        worker_handles.push(tokio::spawn(periodic_rebuild(
            rebuild_index,
            rebuild_pool,
            rebuild_tenant,
            std::time::Duration::from_secs(rebuild_secs),
        )));
    }

    let snapshot_transport = transport.clone();
    let transport_handle = transport.clone();
    let csr_interval_worker = CsrIntervalWorker::new(
        CsourceSubscriptionRepository::new(&pool),
        Arc::clone(&registration_store),
        transport,
        tenant.clone(),
    );
    worker_handles.push(tokio::spawn(csr_interval_worker.run()));

    let entity_query_service = Arc::new(EntityQueryService::new(
        Arc::clone(&entity_store),
        engine.clone(),
        Arc::clone(&spatial_index),
        tenant,
    ));
    let entity_discovery_service = Arc::new(EntityDiscoveryService::new(entity_store, engine.clone()));

    let forwarding_service = Arc::new(crate::service::csource_forwarding::CsourceForwardingService::new(
        registration_store,
        engine,
        config.info.source_identity.clone(),
        config.performance.forward_csr_concurrency,
    )?);

    let entity_map_service = Arc::new(EntityMapService::new(Arc::new(InMemoryEntityMapStore::new())));

    let snapshot_service = Arc::new(crate::service::snapshot::SnapshotService::new(
        snapshot_store,
        Some(snapshot_transport),
        Arc::clone(&entity_query_service),
        Arc::clone(&temporal_service),
        Arc::clone(&forwarding_service),
        config.performance.snapshot_concurrency,
    ));

    Ok((
        AndromedaState {
            entities: entity_service,
            entity_query: entity_query_service,
            entity_discovery: entity_discovery_service,
            temporal: temporal_service,
            subscriptions: subscription_service,
            contexts: context_service,
            csr: csr_service,
            csr_subscriptions: csr_subscription_service,
            forwarding: forwarding_service,
            entity_maps: entity_map_service,
            snapshots: snapshot_service,
            sse_registry,
            sse_channel_buffer_size: config.notifications.sse.channel_buffer_size,
            sse_keep_alive_interval_secs: config.notifications.sse.keep_alive_interval_secs,
            ws_registry,
            ws_channel_buffer_size: config.notifications.ws.channel_buffer_size,
            ws_ping_interval_secs: config.notifications.ws.ping_interval_secs,
        },
        worker_handles,
        transport_handle,
        pool,
    ))
}
