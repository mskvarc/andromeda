//! Broker composition and process-lifecycle coordination.

use crate::{
    component_report::{ComponentReportError, collect as collect_component_reports},
    config::{observability::MetricsExposure, runtime::RuntimeConfig},
    outcome::ProcessExitOutcome,
    postgresql_maintenance::{MaintenanceRegistrationError, register_postgresql_maintenance},
    routes::{
        context_registry::BrokerContextRegistryStateResolver,
        entity::BrokerEntityStateResolver,
        entity_map::BrokerEntityMapStateResolver,
        notification_receiver::BrokerNotificationReceiverResolver,
        registration::BrokerRegistrationStateResolver,
        snapshot::BrokerSnapshotStateResolver,
        subscription::BrokerSubscriptionStateResolver,
        temporal::BrokerTemporalStateResolver,
    },
    tenant_factory::BrokerTenantFactory,
};
use andromeda_delivery::mqtt::indicator::MqttHealthIndicator;
use andromeda_health::aggregate::Health;
use andromeda_http_api::{api::HttpApi, routes::ApiRoutes};
use andromeda_http_component_report::routes::ComponentReportRoutes;
use andromeda_http_context_registry::routes::ContextRegistryRoutes;
use andromeda_http_entity::routes::EntityRoutes;
use andromeda_http_entity_map::routes::EntityMapRoutes;
use andromeda_http_health::routes::HealthRoutes;
use andromeda_http_notification_receiver::routes::NotificationReceiverRoutes;
use andromeda_http_registration::routes::RegistrationRoutes;
use andromeda_http_server::execution;
use andromeda_http_snapshot::routes::SnapshotRoutes;
use andromeda_http_source_identity::{routes::SourceIdentityRoutes, state::SourceIdentityState};
use andromeda_http_stack::stack::HttpStack;
use andromeda_http_subscription::routes::SubscriptionRoutes;
use andromeda_http_telemetry::{error::TelemetryHttpError, routes::TelemetryRoutes};
use andromeda_http_temporal::routes::TemporalRoutes;
use andromeda_http_tenancy::routes::TenancyRoutes;
use andromeda_postgresql_context_store::{
    error::ContextStoreError,
    forwarded_subscription_routes::PostgresForwardedSubscriptionRouter,
    health::DatabaseHealthIndicator,
    tenant_database::{DatabaseUrl, configure_pool_options},
    tenant_registry::PostgresTenantRegistry,
};
use andromeda_subscription_forwarding::mapping::ForwardedSubscriptionRouter;
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_telemetry::{error::TelemetryError, recorder::TelemetryRecorder};
use andromeda_tenancy::{
    administration::{OrphanTenantEvictor, TenantDirectory},
    context_source_alias::ContextSourceAliases,
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    runtime::TenantRuntimeManager,
    tenant_ref::TenantRef,
};
use chrono::{DateTime, Utc};
use std::{io::Error as IoError, sync::Arc};
use thiserror::Error;
use tokio::runtime::Handle;
use tracing::{
    error,
    info,
    subscriber::{SetGlobalDefaultError, set_global_default},
};
use tracing_subscriber::fmt;

/// HTTP API and process resources whose shutdown begins after traffic drains.
struct BrokerExecution {
    /// Fully assembled state-complete HTTP API.
    api: HttpApi,
    /// Tenant runtimes served by the API.
    tenants: Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>,
    /// Process task scope coordinating cancellation across tenants.
    process_tasks: TaskSupervisor,
}

/// Failure while constructing the concrete broker graph before serving traffic.
#[derive(Debug, Error)]
enum BrokerStartupError {
    /// The primary PostgreSQL pool could not be opened.
    #[error("primary PostgreSQL startup failed: {0}")]
    Database(#[from] sqlx::Error),
    /// The PostgreSQL tenant registry could not be constructed.
    #[error("tenant registry startup failed: {0}")]
    ContextStore(#[from] ContextStoreError),
    /// The default tenant could not be constructed and published.
    #[error("default tenant startup failed: {0}")]
    Tenancy(#[from] TenancyError),
    /// Active component discovery failed.
    #[error(transparent)]
    ComponentReport(#[from] ComponentReportError),
    /// The metrics recorder could not be installed.
    #[error(transparent)]
    Telemetry(#[from] TelemetryError),
    /// The metrics scrape route could not be constructed.
    #[error(transparent)]
    TelemetryHttp(#[from] TelemetryHttpError),
    /// The process-scoped maintenance task could not be registered.
    #[error(transparent)]
    MaintenanceTask(#[from] MaintenanceRegistrationError),
}

/// Construct the broker, serve HTTP, then close process resources in order.
pub async fn run(config: RuntimeConfig) -> ProcessExitOutcome {
    if let Err(error) = initialize_tracing(&config) {
        eprintln!("tracing initialization failed: {error}");
        return ProcessExitOutcome::StartupFailure;
    }
    report_startup(&config);
    let config = Arc::new(config);
    let telemetry = match install_telemetry(config.observability.metrics) {
        Ok(telemetry) => telemetry,
        Err(startup_error) => {
            error!(error = %startup_error, event = "broker_startup_failed");
            return ProcessExitOutcome::StartupFailure;
        }
    };
    // Table 5.2.40-1: `contextSourceUptime` counts from the instant the
    // broker became available, captured once here.
    let started_at = Utc::now();
    let BrokerExecution { api, tenants, process_tasks } = match construct(Arc::clone(&config), telemetry, started_at).await {
        Ok(execution) => execution,
        Err(startup_error) => {
            error!(error = %startup_error, event = "broker_startup_failed");
            return ProcessExitOutcome::StartupFailure;
        }
    };
    let serve_result = execution::serve(&config.server, api.into_service(), shutdown_signal()).await;
    match &serve_result {
        Ok(()) => info!(event = "http_traffic_drained", "HTTP listener stopped after draining accepted requests"),
        Err(server_error) => error!(error = %server_error, event = "http_server_failed"),
    }
    process_tasks.request_cancellation();
    tenants.shutdown_all().await;
    process_tasks.shutdown().await;
    info!(
        event = "broker_resources_closed",
        "Tenant tasks, transports, context flushers, and pools are closed"
    );
    match serve_result {
        Ok(()) => ProcessExitOutcome::Success,
        Err(_) => ProcessExitOutcome::RuntimeFailure,
    }
}

/// Install the process metrics recorder when exposure is enabled.
///
/// The recorder is installed before any pool or worker opens so the first
/// statement the broker runs is already observed.
fn install_telemetry(exposure: MetricsExposure) -> Result<Option<Arc<TelemetryRecorder>>, BrokerStartupError> {
    match exposure {
        MetricsExposure::Disabled => Ok(None),
        MetricsExposure::Prometheus => Ok(Some(Arc::new(TelemetryRecorder::install()?))),
    }
}

/// Construct tenant composition, focused route fragments and the complete HTTP API.
async fn construct(
    config: Arc<RuntimeConfig>,
    telemetry: Option<Arc<TelemetryRecorder>>,
    started_at: DateTime<Utc>,
) -> Result<BrokerExecution, BrokerStartupError> {
    let database_url = DatabaseUrl::new(config.database.url.clone()); // clone: the tenant factory retains the validated base URL
    let main_pool = configure_pool_options(config.database.max_connections, config.database.synchronous_commit.as_postgres_setting())
        .connect(config.database.url.as_str())
        .await?;
    let registry = Arc::new(
        PostgresTenantRegistry::connect(
            main_pool,
            database_url.clone(),   // clone: registry and tenant factory independently retain the base URL
            config.tenancy.clone(), // clone: registry owns its runtime tenancy policy
            config.database.synchronous_commit.as_postgres_setting(),
        )
        .await?,
    );
    let process_tasks = TaskSupervisor::process(Handle::current());
    register_postgresql_maintenance(
        &process_tasks,
        registry.main_pool().clone(), // clone: the process task and registry share the platform pool
        database_url.clone(),         // clone: the process task and tenant factory derive independent tenant URLs
        config.database.maintenance,
        config.database.synchronous_commit.as_postgres_setting(),
    )?;
    // The platform route from minted subscription identifiers to tenants is
    // shared by every tenant's forwarding service and by the receiver.
    let forwarded_subscription_router: Arc<dyn ForwardedSubscriptionRouter> = Arc::new(PostgresForwardedSubscriptionRouter::new(registry.main_pool()));
    let factory = Arc::new(BrokerTenantFactory::new(
        &config,
        database_url,
        process_tasks.clone(), // clone: tenant scopes and process shutdown share supervision
        Arc::clone(&forwarded_subscription_router),
    ));
    let tenants = TenantRuntimeManager::new(config.tenancy.clone(), Arc::clone(&registry), Arc::clone(&factory)); // clone: manager owns its policy
    drop(tenants.spawn_janitor());
    let _default_state = tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?;

    let database_indicator = DatabaseHealthIndicator::new(registry.main_pool().clone()); // clone: PgPool is a shared handle
    let mut health = Health::builder().with_indicator(database_indicator);
    if let Some(mqtt_pool) = factory.default_mqtt_pool() {
        health = health.with_indicator(MqttHealthIndicator::new(mqtt_pool));
    }
    let component_reports = collect_component_reports(registry.main_pool()).await?;

    let entity_routes = EntityRoutes::new(Arc::new(BrokerEntityStateResolver::new(Arc::clone(&tenants))));
    let temporal_routes = TemporalRoutes::new(Arc::new(BrokerTemporalStateResolver::new(Arc::clone(&tenants))));
    let context_registry_routes = ContextRegistryRoutes::new(Arc::new(BrokerContextRegistryStateResolver::new(Arc::clone(&tenants))));
    let registration_routes = RegistrationRoutes::new(Arc::new(BrokerRegistrationStateResolver::new(Arc::clone(&tenants))));
    let subscription_routes = SubscriptionRoutes::new(Arc::new(BrokerSubscriptionStateResolver::new(Arc::clone(&tenants))));
    let entity_map_routes = EntityMapRoutes::new(Arc::new(BrokerEntityMapStateResolver::new(Arc::clone(&tenants))));
    let snapshot_routes = SnapshotRoutes::new(Arc::new(BrokerSnapshotStateResolver::new(Arc::clone(&tenants))));
    let directory: Arc<dyn TenantDirectory> = tenants.clone(); // clone: HTTP administration and shutdown share the manager
    let evictor: Arc<dyn OrphanTenantEvictor> = tenants.clone(); // clone: both focused capabilities share the manager
    let telemetry_routes = telemetry.map(TelemetryRoutes::new).transpose()?;
    let routes = ApiRoutes::builder()
        .entity(entity_routes)
        .temporal(temporal_routes)
        .context_registry(context_registry_routes)
        .registration(registration_routes)
        .subscription(subscription_routes)
        .entity_map(entity_map_routes)
        .snapshot(snapshot_routes)
        .source_identity(SourceIdentityRoutes::new(SourceIdentityState::new(
            config.info.source_id.clone(), // clone: configuration and immutable route state own the identifier
            // Table 5.2.40-1: the configured alias is the default tenant's, and
            // every other tenant's is derived from it.
            ContextSourceAliases::new(config.info.source_alias.clone()), // clone: configuration and the alias family own the configured alias
            started_at,
            config.info.source_extras.clone(), // clone: configuration and immutable route state own the raw extras
        )))
        .notification_receiver(NotificationReceiverRoutes::new(Arc::new(BrokerNotificationReceiverResolver::new(
            Arc::clone(&tenants),
            forwarded_subscription_router,
        ))))
        .tenancy(TenancyRoutes::new(directory, evictor))
        .health(HealthRoutes::new(health.build()))
        .component_report(ComponentReportRoutes::new(component_reports))
        .telemetry(telemetry_routes)
        .build();
    let stack = HttpStack::new(config.http.clone()); // clone: the assembled stack owns its process-wide configuration
    Ok(BrokerExecution {
        api: HttpApi::assemble(routes, &stack),
        tenants,
        process_tasks,
    })
}

/// Install composition-owned tracing before any runtime resource is opened.
fn initialize_tracing(config: &RuntimeConfig) -> Result<(), SetGlobalDefaultError> {
    let subscriber = fmt().compact().with_max_level(config.observability.log_level.as_tracing_level()).finish();
    set_global_default(subscriber)
}

/// Report the active construction profile before broker resources are opened.
fn report_startup(config: &RuntimeConfig) {
    info!(
        host = %config.server.host,
        port = config.server.port,
        tenancy_enabled = config.tenancy.enabled,
        notification_delivery_enabled = config.notification_delivery.enabled,
        log_level = %config.observability.log_level,
        metrics = %config.observability.metrics,
        precedence = RuntimeConfig::precedence_summary(),
        "Starting Andromeda"
    );
}

/// Wait for Ctrl-C or SIGTERM, whichever arrives first.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::{
            ctrl_c,
            unix::{SignalKind, signal},
        };

        match signal(SignalKind::terminate()) {
            Ok(mut terminate) => {
                tokio::select! {
                    result = ctrl_c() => report_ctrl_c(result),
                    received = terminate.recv() => report_sigterm(received),
                }
            }
            Err(error) => {
                tracing::error!(error = %error, "SIGTERM signal listener failed; waiting for Ctrl-C");
                report_ctrl_c(ctrl_c().await);
            }
        }
    }

    #[cfg(not(unix))]
    {
        report_ctrl_c(tokio::signal::ctrl_c().await);
    }
}

/// Report the terminal result of the Ctrl-C listener.
fn report_ctrl_c(result: Result<(), IoError>) {
    match result {
        Ok(()) => tracing::info!(signal = "ctrl-c", event = "shutdown_signal_received"),
        Err(error) => tracing::error!(error = %error, "Ctrl-C signal listener failed"),
    }
}

/// Report whether the SIGTERM stream produced a signal before closing.
#[cfg(unix)]
fn report_sigterm(received: Option<()>) {
    if let Some(()) = received {
        tracing::info!(signal = "sigterm", event = "shutdown_signal_received");
    } else {
        tracing::error!("SIGTERM signal stream ended before receiving a signal");
    }
}
