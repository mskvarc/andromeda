//! The HTTP surface: which route reaches which handler.

use crate::{
    api::{entity_binding::BrokerEntityStateResolver, state::BrokerState, temporal_binding::BrokerTemporalStateResolver, tenant_factory::BrokerTenantFactory},
    config::runtime::RuntimeConfig,
    error::AndromedaError,
    http::handler::{csource_registration, csource_subscription, entity_map, health, info, jsonld_context, snapshot, sse, subscription, tenant, version, ws},
    version::service::Version,
};
use andromeda_delivery::mqtt::indicator::MqttHealthIndicator;
use andromeda_health::aggregate::Health;
use andromeda_http_entity::routes::EntityRoutes;
use andromeda_http_temporal::routes::TemporalRoutes;
use andromeda_postgresql_context_store::{
    health::DatabaseHealthIndicator,
    tenant_database::{DatabaseUrl, configure_pool_options},
    tenant_registry::PostgresTenantRegistry,
};
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, runtime::TenantRuntimeManager, tenant_ref::TenantRef};
use axum::{
    Extension,
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, patch, post},
};
use std::sync::Arc;
use tokio::runtime::Handle;
use tower_helmet::HelmetLayer;
use tracing::info;

/// HTTP router together with the tenant runtimes it serves.
pub struct BrokerHttp {
    /// Fully configured broker router.
    pub router: Router,
    /// Tenant manager to drain after the server stops accepting traffic.
    pub tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
    /// Process task scope coordinating cancellation across tenants.
    pub process_tasks: TaskSupervisor,
}

pub async fn create_router(config: Arc<RuntimeConfig>) -> Result<BrokerHttp, AndromedaError> {
    info!("Constructing router!");

    let database_url = DatabaseUrl::new(config.database.url.clone()); // clone: the factory owns the validated URL beyond this borrow
    let main_pool = configure_pool_options(config.database.max_connections, config.database.synchronous_commit.as_postgres_setting())
        .connect(config.database.url.as_str())
        .await?;
    let registry = Arc::new(
        PostgresTenantRegistry::connect(
            main_pool,
            // clone: the registry and the factory each hold the base URL
            database_url.clone(),
            config.tenancy.clone(), // clone: the registry keeps its own copy of the section
            config.database.synchronous_commit.as_postgres_setting(),
        )
        .await?,
    );
    let process_tasks = TaskSupervisor::process(Handle::current());
    let factory = Arc::new(BrokerTenantFactory::new(
        Arc::clone(&config),
        database_url,
        process_tasks.clone(), // clone: the factory derives tenant scopes while server shutdown retains the process scope.
    ));
    let tenants = TenantRuntimeManager::new(config.tenancy.clone(), Arc::clone(&registry), Arc::clone(&factory)); // clone: same

    // The janitor runs for the process lifetime; detaching its handle is the
    // intent, and a single-tenant broker starts none at all.
    drop(tenants.spawn_janitor());

    // The unnamed default runtime has to exist before the first request, and
    // building it is what records the MQTT pool `/health` reports on.
    let _default_state = tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?;

    let db_indicator = DatabaseHealthIndicator::new(registry.main_pool().clone()); // clone: `PgPool` is a handle
    let mut health_builder = Health::builder().with_indicator(db_indicator);
    if let Some(mqtt_pool) = factory.default_mqtt_pool() {
        health_builder = health_builder.with_indicator(MqttHealthIndicator::new(mqtt_pool));
    }
    // The aggregate reaches the handler as an axum extension. It used to reach
    // it through a `Layer` impl on `Health` itself, which the orphan rule
    // stopped being possible the moment the type left the binary -- and which
    // did nothing this line does not.
    let health_router: Router<BrokerState> = Router::new().route("/health", get(health::check)).layer(Extension(health_builder.build()));

    let version_service = Version::new(registry.main_pool().clone()); // clone: `PgPool` is a handle
    let version_router: Router<BrokerState> = Router::new().route("/version", get(version::check)).layer(version_service);

    let broker_state = BrokerState {
        tenants: Arc::clone(&tenants),                  // clone: the router and shutdown coordinator share the manager
        source_id: config.info.source_id.clone(),       // clone: configuration and router state own the validated identifier
        source_alias: config.info.source_alias.clone(), // clone: configuration and router state own the validated alias
    };
    let max_body = config.performance.max_request_body_bytes;
    let entity_resolver = Arc::new(BrokerEntityStateResolver::new(Arc::clone(&tenants)));
    let entity_routes = EntityRoutes::new(entity_resolver).into_router();
    let temporal_resolver = Arc::new(BrokerTemporalStateResolver::new(Arc::clone(&tenants)));
    let temporal_routes = TemporalRoutes::new(temporal_resolver).into_router();
    let router = Router::new()
        .nest(
            "/ngsi-ld/v1",
            build_v1_router(entity_routes, temporal_routes).merge(health_router).merge(version_router),
        )
        .layer(DefaultBodyLimit::max(max_body))
        .layer(HelmetLayer::with_defaults())
        .with_state(broker_state);
    Ok(BrokerHttp {
        router,
        tenants,
        process_tasks,
    })
}

fn build_v1_router(entity_routes: Router, temporal_routes: Router) -> Router<BrokerState> {
    let entity_routes: Router<BrokerState> = entity_routes.with_state(());
    let temporal_routes: Router<BrokerState> = temporal_routes.with_state(());
    let tenants_routes = Router::new().route("/", get(tenant::query)).route("/evictOrphans", post(tenant::evict_orphans));

    let jsonld_context_routes = Router::new()
        .route("/", post(jsonld_context::create))
        .route("/", get(jsonld_context::query))
        .route("/{context_id}", get(jsonld_context::get))
        .route("/{context_id}", delete(jsonld_context::delete));

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

    Router::new()
        .merge(entity_routes)
        .nest("/temporal", temporal_routes)
        .nest("/tenants", tenants_routes)
        .nest("/jsonldContexts", jsonld_context_routes)
        .nest("/entityMaps", entity_map_routes)
        .nest("/temporal/entityMaps", temporal_entity_map_routes)
        .nest("/subscriptions", subscription_routes)
        .nest("/csourceRegistrations", csr_routes)
        .nest("/csourceSubscriptions", csr_subscription_routes)
        .nest("/snapshots", snapshot_routes)
        .nest("/info", info_routes)
}
