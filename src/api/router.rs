//! The HTTP surface: which route reaches which handler.

use crate::{
    api::{state::AppState, tenant_factory::BrokerTenantFactory},
    core::{config::RuntimeConfig, error::AndromedaError},
    http::handler::{
        attribute,
        csource_registration,
        csource_subscription,
        entity,
        entity_map,
        entity_operations,
        health,
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
    version::service::Version,
};
use andromeda_delivery::mqtt::indicator::MqttHealthIndicator;
use andromeda_health::aggregate::Health;
use andromeda_postgresql_context_store::{
    health::DatabaseHealthIndicator,
    tenant_database::{DatabaseUrl, configure_pool_options},
    tenant_registry::PostgresTenantRegistry,
};
use andromeda_tenancy::{creation_policy::TenantCreationPolicy, runtime::TenantRuntimeManager, tenant_ref::TenantRef};
use axum::{
    Extension,
    Router,
    extract::DefaultBodyLimit,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::{delete, get, patch, post, put},
};
use std::sync::Arc;
use tower_helmet::HelmetLayer;
use tracing::info;

pub async fn create_router(config: Arc<RuntimeConfig>) -> Result<Router, AndromedaError> {
    info!("Constructing router!");

    let database_url = DatabaseUrl::parse(&config.database.url)?;
    let main_pool = configure_pool_options(config.database.max_connections, config.database.synchronous_commit.as_postgres_setting())
        .connect(&config.database.url)
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
    let factory = Arc::new(BrokerTenantFactory::new(Arc::clone(&config), database_url));
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
    let health_router: Router<AppState> = Router::new().route("/health", get(health::check)).layer(Extension(health_builder.build()));

    let version_service = Version::new(registry.main_pool().clone()); // clone: `PgPool` is a handle
    let version_router: Router<AppState> = Router::new().route("/version", get(version::check)).layer(version_service);

    let app_state = AppState {
        tenants,
        source_identity: config
            .info
            .source_identity
            .parse()
            .map_err(|e| AndromedaError::internal(format!("info.source_identity is not a valid context source id: {e}")))?,
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
    let tenants_routes = Router::new().route("/", get(tenant::query)).route("/evictOrphans", post(tenant::evict_orphans));

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
}
