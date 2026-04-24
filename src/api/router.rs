use crate::{
    api::state::{AndromedaState, AppState},
    core::{batch_activity::BatchActivityTracker, config::RuntimeConfig, error::AndromedaError},
    domain::notification::{ChangeEvent, CsourceSubscriptionChange, CsrChangeEvent, SubscriptionChange},
    health::{Health, database::DatabaseHealthIndicator, mqtt::MqttHealthIndicator},
    http::handler::{
        attribute,
        csource_registration,
        csource_subscription,
        entity,
        entity_map,
        entity_operations,
        health,
        i18n,
        info,
        jsonld_context,
        snapshot,
        sse,
        subscription,
        temporal,
        temporal_operations,
        tenant,
        types,
        version,
        ws,
    },
    repository::{
        csource_registration::repo::CsourceRegistrationRepository,
        csource_subscription::repo::CsourceSubscriptionRepository,
        entity::repo::EntityRepository,
        jsonld_context::repo::JsonLdContextRepository,
        outbox::repo::OutboxRepository,
        snapshot::repo::SnapshotRepository,
        subscription::repo::SubscriptionRepository,
        temporal::repo::TemporalRepository,
    },
    service::{
        csource_registration::CsourceRegistrationService,
        csource_subscription::CsourceSubscriptionService,
        entity::{
            BatchExecutionConfig,
            BatchSideEffectDispatcherConfig,
            EntityService,
            OutboxWriteConfig,
            discovery::EntityDiscoveryService,
            query::EntityQueryService,
        },
        jsonld_context::JsonLdContextService,
        spatial::{entity_index::SpatialEntityIndex, warmup},
        subscription::SubscriptionService,
        subscription_cache::SubscriptionCache,
        temporal::TemporalService,
    },
    tenancy::{
        database::migrate_tenant_db,
        runtime::TenantRuntimeManager,
        types::{TenantCreationPolicy, TenantRef},
    },
    transport::{dispatch::TransportDispatcher, http::HttpTransportConfig, mqtt::MqttPoolConfig, sse::SseConnectionRegistry, ws::WsConnectionRegistry},
    version::Version,
    worker::{
        batch_side_effects::BatchSideEffectWorker,
        csr_interval::CsrIntervalWorker,
        csr_notification::CsrNotificationWorker,
        data_expiration::DataExpirationWorker,
        interval::IntervalWorker,
        notification::NotificationWorker,
        outbox_delivery::OutboxDeliveryWorker,
        snapshot_expiration::SnapshotExpirationWorker,
        spatial_sync::SpatialSyncWorker,
        subscription_expiration::SubscriptionExpirationWorker,
    },
};
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, patch, post, put},
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tower_helmet::HelmetLayer;
use tracing::info;

pub async fn create_router(config: Arc<RuntimeConfig>) -> Result<Router, AndromedaError> {
    info!("Constructing router!");
    let tenants = TenantRuntimeManager::new(Arc::clone(&config)).await?;

    // Ensure unnamed default runtime is initialized at startup.
    let _ = tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?;

    let db_indicator = DatabaseHealthIndicator::new(tenants.main_pool().clone());
    let mut health_builder = Health::builder().with_indicator(db_indicator);
    if let Some(mqtt_pool) = tenants.default_mqtt_pool() {
        health_builder = health_builder.with_indicator(MqttHealthIndicator::new(mqtt_pool));
    }
    let health_router: Router<AppState> = Router::new().route("/health", get(health::check)).layer(health_builder.build());

    let version_service = Version::new(tenants.main_pool().clone());
    let version_router: Router<AppState> = Router::new().route("/version", get(version::check)).layer(version_service);

    let app_state = AppState {
        tenants,
        source_identity: config.info.source_identity.clone(),
    };
    let max_body = config.performance.max_request_body_bytes;
    let router = Router::new()
        .route("/favicon.ico", get(favicon))
        .nest("/ngsi-ld/v1", build_v1_router().merge(health_router).merge(version_router))
        .layer(DefaultBodyLimit::max(max_body))
        .layer(HelmetLayer::with_defaults())
        .with_state(app_state);
    Ok(router)
}

/// Build the verification-method resolver from the `[provenance]` config
/// section. Static entries are decoded eagerly at startup so that
/// invalid multibase literals fail fast. When the HTTPS resolver is
/// enabled a chained resolver is returned, preferring static config.
fn build_provenance_resolver(
    config: &crate::core::config::ProvenanceConfig,
) -> Result<std::sync::Arc<dyn crate::service::provenance::VerificationMethodResolver>, AndromedaError> {
    use crate::service::provenance::{ChainedResolver, HttpsResolver, StaticResolver, VerificationMethodResolver};
    let static_resolver = StaticResolver::new();
    for (uri, multibase) in &config.verification_methods {
        static_resolver
            .insert_multibase(uri.clone(), multibase)
            .map_err(|e| AndromedaError::internal(format!("invalid publicKeyMultibase for {uri}: {e}")))?;
    }
    let static_arc: std::sync::Arc<dyn VerificationMethodResolver> = std::sync::Arc::new(static_resolver);
    if !config.https_resolver_enabled {
        return Ok(static_arc);
    }
    let https = HttpsResolver::new(reqwest::Client::new());
    Ok(std::sync::Arc::new(ChainedResolver::new(vec![static_arc, std::sync::Arc::new(https)])))
}

async fn favicon() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/x-icon")],
        include_bytes!("favicon.ico").as_slice(),
    )
}

fn build_v1_router() -> Router<AppState> {
    let entity_routes = Router::new()
        .route("/", post(entity::create))
        .route("/", get(entity::query))
        .route("/", delete(entity::delete_by_query))
        .route("/", patch(entity::patch_no_id))
        .route("/", put(entity::put_no_id))
        .route("/{id}", get(entity::get))
        .route("/{id}", delete(entity::delete))
        .route("/{id}", patch(entity::merge))
        .route("/{id}", put(entity::update))
        .route("/{id}/attrs", post(entity::append_attrs))
        .route("/{id}/attrs", patch(entity::update_attrs))
        .route("/{id}/attrs/{attr_id}", patch(entity::partial_update_attr))
        .route("/{id}/attrs/{attr_id}", delete(entity::delete_attr))
        .route("/{id}/attrs/{attr_id}", put(entity::replace_attr));

    let entity_ops_routes = Router::new()
        .route("/create", post(entity_operations::batch_create))
        .route("/upsert", post(entity_operations::batch_upsert))
        .route("/update", post(entity_operations::batch_update))
        .route("/merge", post(entity_operations::batch_merge))
        .route("/delete", post(entity_operations::batch_delete))
        .route("/query", post(entity_operations::query));

    let types_routes = Router::new().route("/", get(types::query)).route("/{type_}", get(types::get));
    let tenants_routes = Router::new().route("/", get(tenant::query));

    let attributes_routes = Router::new().route("/", get(attribute::query)).route("/{attr_id}", get(attribute::get));

    let jsonld_context_routes = Router::new()
        .route("/", post(jsonld_context::create))
        .route("/", get(jsonld_context::query))
        .route("/{context_id}", get(jsonld_context::get))
        .route("/{context_id}", delete(jsonld_context::delete));

    let temporal_entity_routes = Router::new()
        .route("/", post(temporal::create_or_update))
        .route("/", get(temporal::query))
        .route("/{id}", get(temporal::get))
        .route("/{id}", delete(temporal::delete))
        .route("/{id}/attrs", post(temporal::add_attrs))
        .route("/{id}/attrs/{attr_id}", delete(temporal::delete_attr))
        .route("/{id}/attrs/{attr_id}/{instance_id}", patch(temporal::update_attr_instance))
        .route("/{id}/attrs/{attr_id}/{instance_id}", delete(temporal::delete_attr_instance));

    let temporal_ops_routes = Router::new().route("/query", post(temporal_operations::query));

    let entity_map_routes = Router::new()
        .route("/", get(entity_map::create_from_query))
        .route("/", post(entity_map::create_from_query_post))
        .route("/{entity_map_id}", get(entity_map::get))
        .route("/{entity_map_id}", patch(entity_map::update))
        .route("/{entity_map_id}", delete(entity_map::delete));

    let temporal_entity_map_routes = Router::new()
        .route("/", get(entity_map::create_temporal_from_query))
        .route("/", post(entity_map::create_temporal_from_query_post));

    let subscription_routes = Router::new()
        .route("/", post(subscription::create))
        .route("/", get(subscription::query))
        .route("/{subscription_id}", get(subscription::get))
        .route("/{subscription_id}", patch(subscription::update))
        .route("/{subscription_id}", delete(subscription::delete))
        .route("/{subscription_id}/stream", get(sse::stream))
        .route("/{subscription_id}/ws", get(ws::stream));

    let csr_routes = Router::new()
        .route("/", post(csource_registration::create))
        .route("/", get(csource_registration::query))
        .route("/{registration_id}", get(csource_registration::get))
        .route("/{registration_id}", patch(csource_registration::update))
        .route("/{registration_id}", delete(csource_registration::delete));

    let csr_subscription_routes = Router::new()
        .route("/", post(csource_subscription::create))
        .route("/", get(csource_subscription::query))
        .route("/{subscription_id}", get(csource_subscription::get))
        .route("/{subscription_id}", patch(csource_subscription::update))
        .route("/{subscription_id}", delete(csource_subscription::delete));

    let snapshot_routes = Router::new()
        .route("/", post(snapshot::create))
        .route("/", delete(snapshot::purge))
        .route("/{snapshot_id}", get(snapshot::get))
        .route("/{snapshot_id}", patch(snapshot::update))
        .route("/{snapshot_id}", delete(snapshot::delete))
        .route("/{snapshot_id}/clone", post(snapshot::clone));

    let info_routes = Router::new().route("/sourceIdentity", get(info::get));
    let i18n_routes = Router::new().route("/entities", get(i18n::query)).route("/entities/{id}", get(i18n::get));

    Router::new()
        .nest("/entities", entity_routes)
        .nest("/entityOperations", entity_ops_routes)
        .nest("/types", types_routes)
        .nest("/tenants", tenants_routes)
        .nest("/attributes", attributes_routes)
        .nest("/jsonldContexts", jsonld_context_routes)
        .nest("/temporal/entities", temporal_entity_routes)
        .nest("/temporal/entityOperations", temporal_ops_routes)
        .nest("/entityMaps", entity_map_routes)
        .nest("/temporal/entityMaps", temporal_entity_map_routes)
        .nest("/subscriptions", subscription_routes)
        .nest("/csourceRegistrations", csr_routes)
        .nest("/csourceSubscriptions", csr_subscription_routes)
        .nest("/snapshots", snapshot_routes)
        .nest("/info", info_routes)
        .nest("/i18n", i18n_routes)
}

pub async fn build_tenant_state(
    config: &RuntimeConfig,
    db_url: &str,
    tenant: TenantRef,
) -> Result<(AndromedaState, Vec<JoinHandle<()>>, Arc<TransportDispatcher>), AndromedaError> {
    let max_connections = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections,
        TenantRef::Named(_) => config.tenancy.tenant_max_connections,
    };
    let pool = PgPoolOptions::new().max_connections(max_connections).connect(db_url).await?;

    migrate_tenant_db(&pool).await?;

    let timescale_features = crate::repository::timescale_features::TimescaleFeatures::detect(&pool)
        .await
        .map_err(|e| AndromedaError::internal(format!("Failed to probe TimescaleDB features: {}", e)))?;
    tracing::info!(
        has_continuous_aggregates = timescale_features.has_continuous_aggregates,
        "Detected TimescaleDB capabilities"
    );

    let (change_tx, _) = tokio::sync::broadcast::channel::<ChangeEvent>(4096);
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
    let engine = crate::jsonld::build_engine(&config.jsonld, context_repo)?;

    let batch_activity = BatchActivityTracker::new();

    let pool_size = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections as usize,
        TenantRef::Named(_) => config.tenancy.tenant_max_connections as usize,
    };
    let batch_concurrency = config.performance.batch_concurrency.unwrap_or_else(|| usize::min(pool_size, 25)).max(1);
    let batch_chunk_concurrency = config.performance.batch_chunk_concurrency.unwrap_or_else(|| (pool_size / 2).max(1)).max(1);
    let batch_side_effects = if let Some(threshold) = config.performance.batch_async_side_effect_threshold {
        let (sender, receiver) = tokio::sync::mpsc::channel(config.performance.batch_side_effect_queue_capacity.max(1));
        let temporal_repo = Arc::new(TemporalRepository::new(&pool).with_features(timescale_features));
        let worker = BatchSideEffectWorker::new(temporal_repo, Some(change_tx.clone()), receiver);
        worker_handles.push(tokio::spawn(worker.run()));
        Some(BatchSideEffectDispatcherConfig {
            threshold: threshold.max(1),
            sender,
        })
    } else {
        None
    };
    let subscription_cache = Arc::new(SubscriptionCache::new(tenant.clone()));
    if let Err(e) = subscription_cache.reload(&SubscriptionRepository::new(&pool)).await {
        tracing::error!("Failed to load subscription cache: {}", e);
    }

    let spatial_index = Arc::new(SpatialEntityIndex::new());
    if let Err(e) = warmup::warmup_location(&spatial_index, &pool, &tenant).await {
        tracing::warn!("spatial warmup failed: {}", e);
    }

    let entity_service = Arc::new(EntityService::new(
        &pool,
        engine.clone(),
        Some(change_tx.clone()),
        BatchExecutionConfig {
            concurrency: batch_concurrency,
            chunk_concurrency: batch_chunk_concurrency,
            chunk_size: config.performance.batch_chunk_size.max(1),
            large_threshold: config.performance.batch_large_threshold.max(1),
            prepare_blocking_threshold: config.performance.batch_prepare_blocking_threshold.max(1),
        },
        batch_side_effects,
        batch_activity.clone(),
        if config.outbox.enabled {
            Some(OutboxWriteConfig {
                subscription_cache: Arc::clone(&subscription_cache),
            })
        } else {
            None
        },
    ));
    let temporal_service = Arc::new(TemporalService::new(&pool, engine.clone(), config.temporal.default_last_n));
    let context_service = Arc::new(JsonLdContextService::new(&pool, engine.clone()));
    let subscription_service = Arc::new(SubscriptionService::new(&pool, engine.clone(), Some(sub_change_tx.clone())));
    let csr_service = Arc::new(CsourceRegistrationService::new(&pool, engine.clone(), Some(csr_change_tx.clone())));
    let csr_subscription_service = Arc::new(CsourceSubscriptionService::new(&pool, engine.clone(), Some(csr_sub_change_tx.clone())));

    let sse_registry = Arc::new(SseConnectionRegistry::new());
    let ws_registry = Arc::new(WsConnectionRegistry::new());
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
    let notification_entity_query = EntityQueryService::new(&pool, engine.clone(), Arc::clone(&spatial_index), tenant.clone());
    let notification_worker = NotificationWorker::new(
        change_tx.subscribe(),
        sub_change_tx.subscribe(),
        crate::worker::notification::NotificationWorkerDeps {
            sub_repo: SubscriptionRepository::new(&pool),
            entity_repo: EntityRepository::new(&pool),
            entity_query: notification_entity_query,
            engine: engine.clone(),
            transport: transport.clone(),
            tenant: tenant.clone(),
            subscription_cache: Arc::clone(&subscription_cache),
        },
    );
    worker_handles.push(tokio::spawn(notification_worker.run()));

    if config.outbox.enabled {
        let outbox_worker = OutboxDeliveryWorker::new(OutboxRepository::new(&pool), &config.outbox);
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
        EntityRepository::new(&pool),
        engine.clone(),
        transport.clone(),
        tenant.clone(),
    );
    worker_handles.push(tokio::spawn(interval_worker.run()));

    let csr_notification_worker = CsrNotificationWorker::new(
        csr_change_tx.subscribe(),
        csr_sub_change_tx.subscribe(),
        CsourceSubscriptionRepository::new(&pool),
        CsourceRegistrationRepository::new(&pool),
        transport.clone(),
        tenant.clone(),
    );
    worker_handles.push(tokio::spawn(csr_notification_worker.run()));

    // Combined entity and attribute expiration worker (§4.22).
    let data_expiration_worker = DataExpirationWorker::new(EntityRepository::new(&pool), Some(change_tx), expiration_interval, batch_activity);
    worker_handles.push(tokio::spawn(data_expiration_worker.run()));

    // Snapshot expiration worker (NGSI-LD 5.16 -- clean up expired snapshots)
    let snapshot_expiration_worker = SnapshotExpirationWorker::new(SnapshotRepository::new(&pool), expiration_interval);
    worker_handles.push(tokio::spawn(snapshot_expiration_worker.run()));

    // Spatial index: event-driven incremental sync + periodic bulk rebuild.
    let spatial_sync_worker = SpatialSyncWorker::new(spatial_change_rx, Arc::clone(&spatial_index), &pool, tenant.clone());
    worker_handles.push(tokio::spawn(spatial_sync_worker.run()));

    let rebuild_secs = config.spatial.rebuild_interval_secs;
    if rebuild_secs > 0 {
        let rebuild_index = Arc::clone(&spatial_index);
        let rebuild_pool = pool.clone();
        let rebuild_tenant = tenant.clone();
        worker_handles.push(tokio::spawn(warmup::periodic_rebuild(
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
        CsourceRegistrationRepository::new(&pool),
        transport,
        tenant.clone(),
    );
    worker_handles.push(tokio::spawn(csr_interval_worker.run()));

    let entity_query_service = Arc::new(EntityQueryService::new(&pool, engine.clone(), Arc::clone(&spatial_index), tenant));
    let entity_discovery_service = Arc::new(EntityDiscoveryService::new(&pool, engine.clone()));

    let forwarding_service = Arc::new(crate::service::csource_forwarding::CsourceForwardingService::new(
        &pool,
        engine,
        config.info.source_identity.clone(),
        config.performance.forward_csr_concurrency,
    )?);

    let entity_map_store = crate::repository::entity_map::EntityMapStore::new();
    let entity_map_service = Arc::new(crate::service::entity_map::EntityMapService::new(entity_map_store));

    let snapshot_service = Arc::new(crate::service::snapshot::SnapshotService::new(
        &pool,
        Some(snapshot_transport),
        Arc::clone(&entity_query_service),
        Arc::clone(&temporal_service),
        Arc::clone(&forwarding_service),
        config.performance.snapshot_concurrency,
    ));

    let provenance_service = Arc::new(crate::service::provenance::ProvenanceService::new(build_provenance_resolver(
        &config.provenance,
    )?));

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
            provenance: provenance_service,
            sse_registry,
            sse_channel_buffer_size: config.notifications.sse.channel_buffer_size,
            sse_keep_alive_interval_secs: config.notifications.sse.keep_alive_interval_secs,
            ws_registry,
            ws_channel_buffer_size: config.notifications.ws.channel_buffer_size,
            ws_ping_interval_secs: config.notifications.ws.ping_interval_secs,
        },
        worker_handles,
        transport_handle,
    ))
}
