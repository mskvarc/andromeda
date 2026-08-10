//! The HTTP surface: which route reaches which handler.

use crate::{
    api::{
        context_registry_binding::BrokerContextRegistryStateResolver,
        entity_binding::BrokerEntityStateResolver,
        entity_map_binding::BrokerEntityMapStateResolver,
        registration_binding::BrokerRegistrationStateResolver,
        snapshot_binding::BrokerSnapshotStateResolver,
        state::BrokerState,
        subscription_binding::BrokerSubscriptionStateResolver,
        temporal_binding::BrokerTemporalStateResolver,
        tenant_factory::BrokerTenantFactory,
    },
    config::runtime::RuntimeConfig,
    error::AndromedaError,
    http::handler::{health, info, tenant, version},
    version::service::Version,
};
use andromeda_delivery::mqtt::indicator::MqttHealthIndicator;
use andromeda_health::aggregate::Health;
use andromeda_http_context_registry::routes::ContextRegistryRoutes;
use andromeda_http_entity::routes::EntityRoutes;
use andromeda_http_entity_map::routes::EntityMapRoutes;
use andromeda_http_registration::routes::RegistrationRoutes;
use andromeda_http_snapshot::routes::SnapshotRoutes;
use andromeda_http_subscription::routes::SubscriptionRoutes;
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
    routing::{get, post},
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
    let context_registry_resolver = Arc::new(BrokerContextRegistryStateResolver::new(Arc::clone(&tenants)));
    let context_registry_routes = ContextRegistryRoutes::new(context_registry_resolver).into_router();
    let registration_resolver = Arc::new(BrokerRegistrationStateResolver::new(Arc::clone(&tenants)));
    let registration_routes = RegistrationRoutes::new(registration_resolver).into_router();
    let subscription_resolver = Arc::new(BrokerSubscriptionStateResolver::new(Arc::clone(&tenants)));
    let subscription_routes = SubscriptionRoutes::new(subscription_resolver).into_router();
    let entity_map_resolver = Arc::new(BrokerEntityMapStateResolver::new(Arc::clone(&tenants)));
    let entity_map_routes = EntityMapRoutes::new(entity_map_resolver).into_router();
    let snapshot_resolver = Arc::new(BrokerSnapshotStateResolver::new(Arc::clone(&tenants)));
    let snapshot_routes = SnapshotRoutes::new(snapshot_resolver).into_router();
    let router = Router::new()
        .nest(
            "/ngsi-ld/v1",
            build_v1_router(
                entity_routes,
                temporal_routes,
                context_registry_routes,
                registration_routes,
                subscription_routes,
                entity_map_routes,
                snapshot_routes,
            )
            .merge(health_router)
            .merge(version_router),
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

fn build_v1_router(
    entity_routes: Router,
    temporal_routes: Router,
    context_registry_routes: Router,
    registration_routes: Router,
    subscription_routes: Router,
    entity_map_routes: Router,
    snapshot_routes: Router,
) -> Router<BrokerState> {
    let entity_routes: Router<BrokerState> = entity_routes.with_state(());
    let temporal_routes: Router<BrokerState> = temporal_routes.with_state(());
    let context_registry_routes: Router<BrokerState> = context_registry_routes.with_state(());
    let registration_routes: Router<BrokerState> = registration_routes.with_state(());
    let subscription_routes: Router<BrokerState> = subscription_routes.with_state(());
    let entity_map_routes: Router<BrokerState> = entity_map_routes.with_state(());
    let snapshot_routes: Router<BrokerState> = snapshot_routes.with_state(());
    let tenants_routes = Router::new().route("/", get(tenant::query)).route("/evictOrphans", post(tenant::evict_orphans));

    let info_routes = Router::new().route("/sourceIdentity", get(info::get));

    Router::new()
        .merge(entity_routes)
        .nest("/temporal", temporal_routes)
        .nest("/tenants", tenants_routes)
        .merge(context_registry_routes)
        .merge(entity_map_routes)
        .merge(subscription_routes)
        .merge(registration_routes)
        .merge(snapshot_routes)
        .nest("/info", info_routes)
}
