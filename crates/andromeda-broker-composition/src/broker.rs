//! Broker composition and process-lifecycle coordination.

use crate::{
    component_report::{ComponentReportError, collect as collect_component_reports},
    config::{
        management::{ManagementMode, MetricsExposure},
        runtime::RuntimeConfig,
    },
    listeners,
    logging,
    outcome::ProcessExitOutcome,
    postgresql_maintenance::{MaintenanceRegistrationError, register_postgresql_maintenance},
    routes::{
        context_registry::BrokerContextRegistryStateResolver,
        entity::BrokerEntityStateResolver,
        entity_map::BrokerEntityMapStateResolver,
        notification_receiver::BrokerNotificationReceiverResolver,
        registration::BrokerRegistrationStateResolver,
        snapshot::BrokerSnapshotStateResolver,
        source_identity::source_identity_routes,
        subscription::BrokerSubscriptionStateResolver,
        temporal::BrokerTemporalStateResolver,
    },
    shutdown_signal::shutdown_signal,
    startup_report::report_startup,
    tenant_factory::BrokerTenantFactory,
    tenant_waker::{TenantWakerRegistrationError, register_tenant_waker},
};
use andromeda_delivery::mqtt::indicator::MqttHealthIndicator;
use andromeda_extensions::{enabled::EnabledExtensions, startup_log::log_conformance_override};
use andromeda_health::aggregate::Health;
use andromeda_http_api::{
    api::HttpApi,
    management::{ManagementApi, ManagementRoutes},
    routes::ApiRoutes,
};
use andromeda_http_component_report::routes::ComponentReportRoutes;
use andromeda_http_context_registry::routes::ContextRegistryRoutes;
use andromeda_http_entity::routes::EntityRoutes;
use andromeda_http_entity_map::routes::EntityMapRoutes;
use andromeda_http_extraction::api_root::AdvertisedApiRoot;
use andromeda_http_health::routes::HealthRoutes;
use andromeda_http_notification_receiver::routes::NotificationReceiverRoutes;
use andromeda_http_registration::routes::RegistrationRoutes;
use andromeda_http_snapshot::routes::SnapshotRoutes;
use andromeda_http_stack::stack::{HttpStack, HttpStackBuildError};
use andromeda_http_subscription::routes::SubscriptionRoutes;
use andromeda_http_telemetry::{error::TelemetryHttpError, routes::TelemetryRoutes};
use andromeda_http_temporal::routes::TemporalRoutes;
use andromeda_http_tenancy::routes::{TenancyRoutes, TenantManagement};
use andromeda_postgresql_context_store::{
    error::ContextStoreError,
    forwarded_subscription_routes::PostgresForwardedSubscriptionRouter,
    health::DatabaseHealthIndicator,
    pending_work::PostgresPendingWorkProbe,
    tenant_database::DatabaseUrl,
    tenant_registry::PostgresTenantRegistry,
    tenant_usage::PostgresTenantUsageProbe,
    tenant_wake_schedule::PostgresTenantWakeSchedule,
};
use andromeda_postgresql_pool::pool_options::configure_pool_options;
use andromeda_subscription_forwarding::mapping::ForwardedSubscriptionRouter;
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_telemetry::{error::TelemetryError, recorder::TelemetryRecorder};
use andromeda_tenancy::{
    administration::TenantAdministration,
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    pending_work::PendingWorkProbe,
    runtime::TenantRuntimeManager,
    tenant_ref::TenantRef,
    usage_probe::TenantUsageProbe,
    wake_schedule::TenantWakeSchedule,
};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use thiserror::Error;
use tokio::runtime::Handle;
use tracing::{error, info};

/// Connections the maintenance controller's direct platform pool may open.
///
/// A maintenance pass is fleet-serialized and holds exactly one platform
/// connection across its sweep, so two is ample headroom while a lapsed pass's
/// connection is being reclaimed.
const MAINTENANCE_PLATFORM_POOL_CONNECTIONS: u32 = 2;

/// HTTP API and process resources whose shutdown begins after traffic drains.
struct BrokerExecution {
    /// Fully assembled state-complete HTTP API.
    api: HttpApi,
    /// Standalone management-plane service, present only in `separate-port` mode.
    management: Option<ManagementApi>,
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
    /// The dormant-tenant wake tasks could not be registered.
    #[error(transparent)]
    WakerTask(#[from] TenantWakerRegistrationError),
    /// A fallible inbound HTTP middleware layer could not be built.
    #[error(transparent)]
    HttpStack(#[from] HttpStackBuildError),
}

/// Construct the broker, serve HTTP, then close process resources in order.
pub async fn run(config: RuntimeConfig) -> ProcessExitOutcome {
    // Held for the whole process: dropping the guard flushes the non-blocking
    // file appender, so it lives until after the shutdown logs below are written.
    let _log_guard = match logging::initialize(&config) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("tracing initialization failed: {error}");
            return ProcessExitOutcome::StartupFailure;
        }
    };
    report_startup(&config);
    let config = Arc::new(config);
    let telemetry = match install_telemetry(config.management.metrics) {
        Ok(telemetry) => telemetry,
        Err(startup_error) => {
            error!(error = %startup_error, event = "broker_startup_failed");
            return ProcessExitOutcome::StartupFailure;
        }
    };
    // Table 5.2.40-1: `contextSourceUptime` counts from the instant the
    // broker became available, captured once here.
    let started_at = Utc::now();
    let BrokerExecution {
        api,
        management,
        tenants,
        process_tasks,
    } = match construct(Arc::clone(&config), telemetry, started_at).await {
        Ok(execution) => execution,
        Err(startup_error) => {
            error!(error = %startup_error, event = "broker_startup_failed");
            return ProcessExitOutcome::StartupFailure;
        }
    };
    let serve_result = listeners::serve(config.as_ref(), api, management, shutdown_signal()).await;
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

/// The tenant-management routing switch the HTTP crate gates on.
///
/// Derived here rather than in the HTTP crate, which may not depend on the
/// extension set (the HTTP stack depends on it, so the reverse would be a cycle).
fn tenant_management_mode(enabled: EnabledExtensions) -> TenantManagement {
    if enabled.tenant_management_enabled() {
        TenantManagement::Enabled
    } else {
        TenantManagement::Disabled
    }
}

/// The manager as the HTTP tenant-administration capability.
///
/// A focused coercion helper: the HTTP tenant routes and process shutdown share
/// the one runtime manager, which implements the administration port.
fn tenant_administration(tenants: &Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> Arc<dyn TenantAdministration> {
    tenants.clone() // clone: HTTP administration and shutdown share the one manager
}

/// The tenant-management usage probe.
///
/// Reads a tenant's own database over a transient request-path connection (the
/// same shape as the wake probe) and reports the configured snapshot-count
/// ceiling as `snapshotQuota`.
fn tenant_usage_probe(request_url: DatabaseUrl, config: &RuntimeConfig) -> Arc<dyn TenantUsageProbe> {
    Arc::new(PostgresTenantUsageProbe::new(
        request_url,
        config.database.acquire_timeout(),
        config.snapshot.tenant_count_quota.map(|quota| u64::try_from(quota).unwrap_or(u64::MAX)),
    ))
}

/// Construct tenant composition, focused route fragments and the complete HTTP API.
async fn construct(
    config: Arc<RuntimeConfig>,
    telemetry: Option<Arc<TelemetryRecorder>>,
    started_at: DateTime<Utc>,
) -> Result<BrokerExecution, BrokerStartupError> {
    // Two bases. A tenant's request path connects through the pooler when one
    // is configured; provisioning, migration and maintenance always reach
    // PostgreSQL directly, because each keeps session state (a `CREATE DATABASE`
    // outside any transaction, the migrator's session advisory lock, the
    // maintenance sweep's session lock) that a transaction-pooling proxy breaks.
    let request_url = DatabaseUrl::new(config.database.request_url().clone()); // clone: the tenant factory retains the request base URL
    let direct_url = DatabaseUrl::new(config.database.direct_url().clone()); // clone: registry, maintenance and the tenant factory retain the direct base URL
    // The platform database -- the tenant registry every cold load reads, the
    // wake schedule, the forwarded-subscription routes -- is one database, so a
    // pooler has nothing to multiplex for it: routed through one, every replica
    // shares the pooler's handful of server connections for that one database,
    // and provisioning holds one of them across `CREATE DATABASE`, which queues
    // every cold load's registry read behind it. It is served directly, bounded
    // by `database.max_connections`.
    let main_pool = configure_pool_options(config.database.max_connections, config.database.acquire_timeout())
        .connect(config.database.direct_url().as_str())
        .await?;
    let registry = Arc::new(
        PostgresTenantRegistry::connect(
            main_pool,
            direct_url.clone(),     // clone: the registry provisions and migrates over the direct base
            config.tenancy.clone(), // clone: registry owns its runtime tenancy policy
            config.database.synchronous_commit.as_postgres_setting(),
            config.database.acquire_timeout(),
        )
        .await?,
    );
    let process_tasks = TaskSupervisor::process(Handle::current());
    // Maintenance holds a session advisory lock across a whole sweep, so it gets
    // its own direct platform pool rather than sharing the request-path pool.
    let maintenance_platform_pool = configure_pool_options(MAINTENANCE_PLATFORM_POOL_CONNECTIONS, config.database.acquire_timeout())
        .connect(config.database.direct_url().as_str())
        .await?;
    register_postgresql_maintenance(
        &process_tasks,
        maintenance_platform_pool,
        direct_url.clone(), // clone: the process task derives tenant URLs over the direct base
        config.database.maintenance,
        config.database.acquire_timeout(),
    )?;
    // The platform route from minted subscription identifiers to tenants is
    // shared by every tenant's forwarding service and by the receiver.
    let forwarded_subscription_router: Arc<dyn ForwardedSubscriptionRouter> = Arc::new(PostgresForwardedSubscriptionRouter::new(registry.main_pool()));
    // The dormant-tenant wake ports: the probe reads a tenant's own database
    // over a transient request-path connection, and the schedule lives in the
    // platform database beside the registry.
    let pending_work_probe: Arc<dyn PendingWorkProbe> = Arc::new(PostgresPendingWorkProbe::new(request_url.clone(), config.database.acquire_timeout())); // clone: the factory retains the request base URL
    let tenant_usage_probe = tenant_usage_probe(request_url.clone(), &config); // clone: the factory retains the request base URL
    let wake_schedule: Arc<dyn TenantWakeSchedule> = Arc::new(PostgresTenantWakeSchedule::new(registry.main_pool().clone())); // clone: PgPool is a shared handle
    let factory = Arc::new(BrokerTenantFactory::new(
        &config,
        request_url,
        direct_url,
        process_tasks.clone(), // clone: tenant scopes and process shutdown share supervision
        Arc::clone(&forwarded_subscription_router),
    ));
    let tenants = TenantRuntimeManager::new(
        config.tenancy.clone(), // clone: manager owns its policy
        Arc::clone(&registry),
        Arc::clone(&factory),
        pending_work_probe,
        Arc::clone(&wake_schedule),
    );
    drop(tenants.spawn_janitor());
    // The wake tasks share the schedule with the manager and run only when named
    // tenancy is enabled; registered after the janitor so a woken runtime is
    // subject to the same eviction sweeps.
    register_tenant_waker(&process_tasks, Arc::clone(&tenants), wake_schedule, &config.tenancy)?;
    // Warm the unnamed default so its runtime is built before the first
    // request. The returned lease is dropped at once: the default is never
    // evicted, so it needs no hold to stay resident.
    drop(tenants.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await?);

    let database_indicator = DatabaseHealthIndicator::new(registry.main_pool().clone()); // clone: PgPool is a shared handle
    let mut health = Health::builder().with_indicator(database_indicator);
    if let Some(mqtt_pool) = factory.default_mqtt_pool() {
        health = health.with_indicator(MqttHealthIndicator::new(mqtt_pool));
    }
    let component_reports = collect_component_reports(registry.main_pool()).await?;

    // Resolve the enabled extensions once, folding in the conformance override,
    // and log loudly if that override disabled a feature the operator turned on.
    let enabled_extensions = config.extensions.resolve();
    log_conformance_override(&config.extensions);

    let entity_routes = EntityRoutes::new(Arc::new(BrokerEntityStateResolver::new(Arc::clone(&tenants))), enabled_extensions);
    let temporal_routes = TemporalRoutes::new(Arc::new(BrokerTemporalStateResolver::new(Arc::clone(&tenants))), enabled_extensions);
    let context_registry_routes = ContextRegistryRoutes::new(Arc::new(BrokerContextRegistryStateResolver::new(Arc::clone(&tenants))));
    let registration_routes = RegistrationRoutes::new(Arc::new(BrokerRegistrationStateResolver::new(Arc::clone(&tenants))));
    let subscription_routes = SubscriptionRoutes::new(Arc::new(BrokerSubscriptionStateResolver::new(Arc::clone(&tenants))));
    let entity_map_routes = EntityMapRoutes::new(Arc::new(BrokerEntityMapStateResolver::new(Arc::clone(&tenants))));
    let snapshot_routes = SnapshotRoutes::new(Arc::new(BrokerSnapshotStateResolver::new(Arc::clone(&tenants))));
    let telemetry_routes = telemetry.map(TelemetryRoutes::new).transpose()?;
    let health = health.build();
    let tenant_administration = tenant_administration(&tenants);
    let tenant_management = tenant_management_mode(enabled_extensions);
    let routes = ApiRoutes::builder()
        .entity(entity_routes)
        .temporal(temporal_routes)
        .context_registry(context_registry_routes)
        .registration(registration_routes)
        .subscription(subscription_routes)
        .entity_map(entity_map_routes)
        .snapshot(snapshot_routes)
        .source_identity(source_identity_routes(&config.identity, started_at, enabled_extensions, &tenants))
        .notification_receiver(NotificationReceiverRoutes::new(Arc::new(BrokerNotificationReceiverResolver::new(
            Arc::clone(&tenants),
            forwarded_subscription_router,
        ))))
        .tenancy(TenancyRoutes::new(tenant_administration, tenant_usage_probe, tenant_management))
        .health(HealthRoutes::new(health.clone())) // clone: the data plane and the management plane each own a health-route fragment over the same indicators
        .component_report(ComponentReportRoutes::new(component_reports))
        .advertised_api_root(config.identity.advertised_url.clone().map(AdvertisedApiRoot::new)) // clone: clause 6.2, the configured apiRoot every minted URL is built on
        .build();
    let stack = HttpStack::new(config.http.clone())?; // clone: the assembled stack owns its process-wide configuration
    let (api, management) = assemble_planes(config.management.mode, routes, health, telemetry_routes, &stack);
    Ok(BrokerExecution {
        api,
        management,
        tenants,
        process_tasks,
    })
}

/// Assemble the data plane and, when it stands alone, the management plane, per
/// the configured topology.
///
/// `separate-port` isolates the observability endpoints behind their own
/// listener; `merged` mounts them at the root of the data-plane listener;
/// `disabled` serves no management plane, and validation has already forced
/// metrics off in that case. The management probes are backed by the same
/// registered indicators as the data-plane probes, over one shared `Arc`.
fn assemble_planes(
    mode: ManagementMode,
    routes: ApiRoutes,
    health: Health,
    metrics: Option<TelemetryRoutes>,
    stack: &HttpStack,
) -> (HttpApi, Option<ManagementApi>) {
    let management_routes = ManagementRoutes {
        health: HealthRoutes::new(health),
        metrics,
    };
    match mode {
        ManagementMode::SeparatePort => (HttpApi::assemble(routes, None, stack), Some(ManagementApi::assemble(management_routes, stack))),
        ManagementMode::Merged => (HttpApi::assemble(routes, Some(management_routes), stack), None),
        ManagementMode::Disabled => (HttpApi::assemble(routes, None, stack), None),
    }
}
