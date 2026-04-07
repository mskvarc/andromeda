use crate::{
    api::{
        handler::{
            attribute,
            context_source_registration,
            csource_subscription,
            entity,
            entity_map,
            entity_operations,
            info,
            jsonld_context,
            snapshot,
            subscription,
            temporal,
            temporal_operations,
            types,
        },
        state::{AndromedaState, AppState},
    },
    core::config::Configuration,
    domain::notification::{ChangeEvent, CsourceSubscriptionChange, CsrChangeEvent, SubscriptionChange},
    error::AndromedaError,
    repository::{
        context_source_registration::ContextSourceRegistrationRepository,
        csource_subscription::CsourceSubscriptionRepository,
        entity::EntityRepository,
        jsonld_context::JsonLdContextRepository,
        subscription::SubscriptionRepository,
    },
    service::{
        context_source_registration::ContextSourceRegistrationService,
        csource_subscription::CsourceSubscriptionService,
        entity::EntityService,
        jsonld_context::JsonLdContextService,
        subscription::SubscriptionService,
        temporal::TemporalService,
    },
    tenancy::{
        database::migrate_tenant_db,
        runtime::TenantRuntimeManager,
        types::{TenantCreationPolicy, TenantRef},
    },
    transport::{dispatch::TransportDispatcher, http::HttpTransportConfig, mqtt::MqttPoolConfig},
    worker::{
        csr_interval::CsrIntervalWorker,
        csr_notification::CsrNotificationWorker,
        data_expiration::DataExpirationWorker,
        interval::IntervalWorker,
        notification::NotificationWorker,
        subscription_expiration::SubscriptionExpirationWorker,
    },
};
use axum::{
    Router,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, patch, post, put},
};
use axum_health::{Health, database::DatabaseHealthIndicator};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::info;

pub async fn create_router(config: Arc<Configuration>) -> Result<Router, AndromedaError> {
    info!("Constructing router!");
    let tenants = TenantRuntimeManager::new(Arc::clone(&config)).await?;

    // Ensure unnamed default runtime is initialized at startup.
    let _ = tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?;

    let health_indicator = DatabaseHealthIndicator::new("postgres".to_owned(), tenants.main_pool().clone());
    let health_router: Router<AppState> = Router::new()
        .route("/health", get(axum_health::health))
        .layer(Health::builder().with_indicator(health_indicator).build());

    let app_state = AppState {
        tenants,
        source_identity: config.info.source_identity.clone(),
    };
    let router = Router::new().nest("/ngsi-ld/v1", build_v1_router().merge(health_router)).with_state(app_state);
    Ok(router)
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

    let types_routes = Router::new().route("/", get(types::list)).route("/{type_}", get(types::get));

    let attributes_routes = Router::new().route("/", get(attribute::list)).route("/{attr_id}", get(attribute::get));

    let jsonld_context_routes = Router::new()
        .route("/", post(jsonld_context::create))
        .route("/", get(jsonld_context::list))
        .route("/{context_id}", get(jsonld_context::get))
        .route("/{context_id}", delete(jsonld_context::delete));

    let temporal_entity_routes = Router::new()
        .route("/", post(temporal::create_or_update))
        .route("/", get(temporal::query))
        .route("/{id}", get(temporal::get))
        .route("/{id}", delete(temporal::delete))
        .route("/{id}/attrs", post(temporal::add_attrs))
        .route("/{id}/attrs/{attr_id}", delete(temporal::delete_attr))
        .route("/{id}/attrs/{attr_id}/{instance_id}", patch(temporal::modify_attr_instance))
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
        .route("/{subscription_id}", delete(subscription::delete));

    let csr_routes = Router::new()
        .route("/", post(context_source_registration::create))
        .route("/", get(context_source_registration::query))
        .route("/{registration_id}", get(context_source_registration::get))
        .route("/{registration_id}", patch(context_source_registration::update))
        .route("/{registration_id}", delete(context_source_registration::delete));

    let csr_subscription_routes = Router::new()
        .route("/", post(csource_subscription::create))
        .route("/", get(csource_subscription::query))
        .route("/{subscription_id}", get(csource_subscription::get))
        .route("/{subscription_id}", patch(csource_subscription::update))
        .route("/{subscription_id}", delete(csource_subscription::delete));

    let snapshot_routes = Router::new()
        .route("/", post(snapshot::create))
        .route("/", delete(snapshot::purge))
        .route("/{snapshot_id}", get(snapshot::get_status))
        .route("/{snapshot_id}", patch(snapshot::update_status))
        .route("/{snapshot_id}", delete(snapshot::delete))
        .route("/{snapshot_id}/clone", post(snapshot::clone_snapshot));

    let info_routes = Router::new().route("/sourceIdentity", get(info::source_identity));

    Router::new()
        .nest("/entities", entity_routes)
        .nest("/entityOperations", entity_ops_routes)
        .nest("/types", types_routes)
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
        .route("/favicon.ico", get(favicon))
}

pub async fn build_tenant_state(
    config: &Configuration,
    db_url: &str,
    tenant: TenantRef,
) -> Result<(AndromedaState, Vec<JoinHandle<()>>, Arc<TransportDispatcher>), AndromedaError> {
    let max_connections = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections,
        TenantRef::Named(_) => config.multitenancy.tenant_max_connections,
    };
    let pool = PgPoolOptions::new().max_connections(max_connections).connect(db_url).await?;

    migrate_tenant_db(&pool).await?;

    let (change_tx, _) = tokio::sync::broadcast::channel::<ChangeEvent>(4096);
    let (sub_change_tx, _) = tokio::sync::broadcast::channel::<SubscriptionChange>(256);
    let (csr_change_tx, _) = tokio::sync::broadcast::channel::<CsrChangeEvent>(256);
    let (csr_sub_change_tx, _) = tokio::sync::broadcast::channel::<CsourceSubscriptionChange>(256);

    let context_repo = Arc::new(JsonLdContextRepository::new(&pool));
    let engine = crate::jsonld::build_engine(&config.jsonld, context_repo)?;

    let batch_concurrency = config
        .performance
        .batch_concurrency
        .unwrap_or_else(|| usize::min(config.database.max_connections as usize, 25))
        .max(1);
    let entity_service = Arc::new(EntityService::new(&pool, engine.clone(), Some(change_tx.clone()), batch_concurrency));
    let temporal_service = Arc::new(TemporalService::new(&pool, engine.clone(), config.temporal.default_last_n));
    let context_service = Arc::new(JsonLdContextService::new(&pool, engine.clone()));
    let subscription_service = Arc::new(SubscriptionService::new(&pool, engine.clone(), Some(sub_change_tx.clone())));
    let csr_service = Arc::new(ContextSourceRegistrationService::new(&pool, engine.clone(), Some(csr_change_tx.clone())));
    let csr_subscription_service = Arc::new(CsourceSubscriptionService::new(&pool, engine.clone(), Some(csr_sub_change_tx.clone())));

    let transport = Arc::new(
        TransportDispatcher::new(HttpTransportConfig::default(), MqttPoolConfig::default())
            .map_err(|e| crate::error::AndromedaError::internal(format!("Failed to create transport dispatcher: {e}")))?,
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let mut worker_handles = Vec::new();

    // Shared health probe: run only for the unnamed default runtime.
    if matches!(tenant, TenantRef::UnnamedDefault) {
        worker_handles.push(transport.mqtt_pool().spawn_health_probe(shutdown_rx));
    }

    // Keep shutdown sender alive so an eventual graceful shutdown can signal the probe.
    let _shutdown_tx = shutdown_tx;

    let notification_entity_query = crate::service::entity::query::EntityQueryService::new(&pool, engine.clone());
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
        },
    );
    worker_handles.push(tokio::spawn(notification_worker.run()));

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
        ContextSourceRegistrationRepository::new(&pool),
        transport.clone(),
        tenant.clone(),
    );
    worker_handles.push(tokio::spawn(csr_notification_worker.run()));

    // Combined entity and attribute expiration worker (§4.22).
    let data_expiration_worker = DataExpirationWorker::new(EntityRepository::new(&pool), Some(change_tx), expiration_interval);
    worker_handles.push(tokio::spawn(data_expiration_worker.run()));

    let snapshot_transport = transport.clone();
    let transport_handle = transport.clone();
    let csr_interval_worker = CsrIntervalWorker::new(
        CsourceSubscriptionRepository::new(&pool),
        ContextSourceRegistrationRepository::new(&pool),
        transport,
        tenant,
    );
    worker_handles.push(tokio::spawn(csr_interval_worker.run()));

    let entity_query_service = Arc::new(crate::service::entity::query::EntityQueryService::new(&pool, engine.clone()));
    let entity_discovery_service = Arc::new(crate::service::entity::discovery::EntityDiscoveryService::new(&pool, engine.clone()));

    let forwarding_service = Arc::new(crate::service::context_source_forwarding::ContextSourceForwardingService::new(
        &pool,
        engine,
        config.info.source_identity.clone(),
    )?);

    let entity_map_store = crate::repository::entity_map::EntityMapStore::new();
    let entity_map_service = Arc::new(crate::service::entity_map::EntityMapService::new(entity_map_store));

    let snapshot_service = Arc::new(crate::service::snapshot::SnapshotService::new(&pool, Some(snapshot_transport)));

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
        },
        worker_handles,
        transport_handle,
    ))
}
