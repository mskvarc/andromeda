//! Broker composition and process-lifecycle coordination.

use crate::{
    broker_health::broker_health,
    component_report::{ComponentReportError, collect as collect_component_reports},
    configuration::{
        section_admission::{database_defaults, expected_deployment, machine_host_name},
        source_identity::{SourceIdentity, SourceIdentityError},
    },
    extension_routing::{accept_language_reading, tenant_management_mode},
    listeners,
    logging,
    outcome::ProcessExitOutcome,
    pending_work_probe::{PendingWorkProbeError, pending_work_probe},
    planes::assemble_planes,
    postgresql_maintenance::{MaintenanceRegistrationError, register_postgresql_maintenance},
    routes::{
        context_registry::BrokerContextRegistryStateResolver,
        entity::BrokerEntityStateResolver,
        entity_map::BrokerEntityMapStateResolver,
        notification_receiver::BrokerNotificationReceiverResolver,
        registration::BrokerRegistrationStateResolver,
        snapshot::BrokerSnapshotStateResolver,
        source_identity::{SourceIdentityRouteError, source_identity_routes},
        subscription::BrokerSubscriptionStateResolver,
        temporal::BrokerTemporalStateResolver,
    },
    startup_report::report_startup,
    stop_deadline::StopClock,
    tenancy_startup::{TenancyStartupError, start_tenancy},
    tenant_factory::BrokerTenantFactory,
};
use andromeda_configuration::{broker_config::BrokerConfig, load::LoadedConfiguration, schema::feature_mode::FeatureMode};
use andromeda_extensions::{enabled::EnabledExtensions, startup_log::log_conformance_override};
use andromeda_http_api::{api::HttpApi, management::ManagementApi, routes::ApiRoutes};
use andromeda_http_component_report::routes::ComponentReportRoutes;
use andromeda_http_context_registry::routes::ContextRegistryRoutes;
use andromeda_http_entity::routes::EntityRoutes;
use andromeda_http_entity_map::routes::EntityMapRoutes;
use andromeda_http_extraction::api_root::ConfiguredApiRoot;
use andromeda_http_notification_receiver::routes::NotificationReceiverRoutes;
use andromeda_http_registration::routes::RegistrationRoutes;
use andromeda_http_snapshot::routes::SnapshotRoutes;
use andromeda_http_stack::stack::{HttpStack, HttpStackBuildError};
use andromeda_http_subscription::routes::SubscriptionRoutes;
use andromeda_http_telemetry::{error::TelemetryHttpError, routes::TelemetryRoutes};
use andromeda_http_temporal::routes::TemporalRoutes;
use andromeda_http_tenancy::routes::TenancyRoutes;
use andromeda_ngsi_ld_request::pagination::policy::PaginationPolicy;
use andromeda_postgresql_context_store::{
    error::ContextStoreError,
    forwarded_subscription_routes::PostgresForwardedSubscriptionRouter,
    tenant_extension_versions::PostgresTenantExtensionVersions,
    tenant_registry::PostgresTenantRegistry,
    tenant_usage::PostgresTenantUsageProbe,
    tenant_wake_schedule::PostgresTenantWakeSchedule,
};
use andromeda_postgresql_pool::{database_url::DatabaseUrl, pool_options::configure_pool_options, tenant_probe_urls::TenantProbeUrls};
use andromeda_subscription_forwarding::mapping::ForwardedSubscriptionRouter;
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_telemetry::{build_info::BrokerVersion, error::TelemetryError, recorder::TelemetryRecorder};
use andromeda_tenancy::{
    administration::TenantAdministration,
    runtime::TenantRuntimeManager,
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
/// connection across its sweep, and the repack task holds one more for as
/// long as a repack runs, beside the passes. A third leaves headroom while a
/// lapsed pass's connection is being reclaimed.
pub(crate) const MAINTENANCE_PLATFORM_POOL_CONNECTIONS: u32 = 3;

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
    /// The tenant runtime cache could not be started.
    #[error(transparent)]
    Tenancy(#[from] TenancyStartupError),
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
    /// A fallible inbound HTTP middleware layer could not be built.
    #[error(transparent)]
    HttpStack(#[from] HttpStackBuildError),
    /// The `/info/sourceIdentity` route could not be built from the configured identity.
    #[error(transparent)]
    SourceIdentity(#[from] SourceIdentityRouteError),
    /// The dormant-tenant wake probe could not be assembled.
    #[error(transparent)]
    PendingWorkProbe(#[from] PendingWorkProbeError),
    /// The identity in effect is not one the broker can report.
    #[error(transparent)]
    Identity(#[from] SourceIdentityError),
}

/// Construct the broker, serve HTTP, then close process resources in order.
pub(crate) async fn run(loaded: LoadedConfiguration) -> ProcessExitOutcome {
    let LoadedConfiguration { config, ignored } = loaded;
    // Held for the whole process: dropping the guard flushes the non-blocking
    // file appender, so it lives until after the shutdown logs below are written.
    let _log_guard = match logging::initialize(&config) {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!("tracing initialization failed: {error}");
            return ProcessExitOutcome::StartupFailure;
        }
    };
    report_startup(&config, &ignored);
    let config = Arc::new(config);
    let telemetry = match install_telemetry(config.observability.metrics.prometheus.mode) {
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
    let stop = StopClock::default();
    let serve_result = listeners::serve(config.as_ref(), api, management, stop.signal()).await;
    match &serve_result {
        Ok(()) => info!(event = "http_traffic_drained", "HTTP listener stopped after draining accepted requests"),
        Err(server_error) => error!(error = %server_error, event = "http_server_failed"),
    }
    let deadline = stop.deadline(config.server.graceful_drain_timeout.duration(), config.notifications.evaluation.drain_timeout());
    // Cancelling the process scope cancels every tenant's tasks too, so the
    // tenants and the process tasks all count their grace from here and share
    // the one deadline.
    process_tasks.request_cancellation();
    tokio::join!(tenants.shutdown_all(deadline), process_tasks.shutdown_by(deadline));
    info!(
        event = "broker_resources_closed",
        "Tenant tasks, transports, context flushers, and pools are closed"
    );
    match serve_result {
        Ok(()) => ProcessExitOutcome::Success,
        Err(_) => ProcessExitOutcome::RuntimeFailure,
    }
}

/// Install the process metrics recorder when the Prometheus exporter is on.
///
/// The recorder is installed before any pool or worker opens so the first
/// statement the broker runs is already observed.
fn install_telemetry(prometheus: FeatureMode) -> Result<Option<Arc<TelemetryRecorder>>, BrokerStartupError> {
    match prometheus {
        FeatureMode::Disabled => Ok(None),
        FeatureMode::Enabled => Ok(Some(Arc::new(TelemetryRecorder::install(BrokerVersion::from_static(env!(
            "CARGO_PKG_VERSION"
        )))?))),
    }
}

/// The manager as the HTTP tenant-administration capability.
///
/// A focused coercion helper: the HTTP tenant routes and process shutdown share
/// the one runtime manager, which implements the administration port.
fn tenant_administration(tenants: &Arc<TenantRuntimeManager<PostgresTenantRegistry, BrokerTenantFactory>>) -> Arc<dyn TenantAdministration> {
    tenants.clone()
}

/// The tenant-management usage probe.
///
/// Reads a tenant's own database over a transient request-path connection (the
/// same shape as the wake probe) and reports the configured snapshot-count
/// ceiling as `snapshotQuota`.
fn tenant_usage_probe(request_url: DatabaseUrl, config: &BrokerConfig) -> Arc<dyn TenantUsageProbe> {
    Arc::new(PostgresTenantUsageProbe::new(
        request_url,
        config.database.acquire_timeout(),
        config
            .snapshot
            .quota
            .max_per_tenant
            .limit()
            .map(|quota| u64::try_from(quota.get()).unwrap_or(u64::MAX)),
    ))
}

/// Open the platform pool over the tenant registry.
///
/// The platform database (the tenant registry every cold load reads, the
/// wake schedule, the forwarded-subscription routes) is one database, so a
/// pooler has nothing to multiplex for it: routed through one, every replica
/// shares the pooler's handful of server connections for that one database,
/// and provisioning holds one of them across `CREATE DATABASE`, which queues
/// every cold load's registry read behind it. It is served directly, bounded
/// by `database.platform_max_connections`.
async fn connect_registry(config: &BrokerConfig, direct_url: &DatabaseUrl) -> Result<Arc<PostgresTenantRegistry>, BrokerStartupError> {
    Ok(Arc::new(
        PostgresTenantRegistry::connect(
            configure_pool_options(config.database.platform_max_connections.get(), config.database.acquire_timeout()),
            direct_url.clone(),
            config.tenancy.databases.clone(),
            &database_defaults(config),
            config.database.acquire_timeout(),
        )
        .await?,
    ))
}

/// Construct tenant composition, focused route fragments and the complete HTTP API.
async fn construct(
    config: Arc<BrokerConfig>,
    telemetry: Option<Arc<TelemetryRecorder>>,
    started_at: DateTime<Utc>,
) -> Result<BrokerExecution, BrokerStartupError> {
    let source_identity = SourceIdentity::resolve(&config.federation.identity, config.server.public_url.as_ref(), machine_host_name)?;
    source_identity.report();
    // Two bases. A tenant's request path connects through the pooler when one
    // is configured; provisioning, migration and maintenance always reach
    // PostgreSQL directly, because each keeps session state (a `CREATE DATABASE`
    // outside any transaction, the migrator's session advisory lock, the
    // maintenance sweep's session lock) that a transaction-pooling proxy breaks.
    let request_url = DatabaseUrl::new(config.database.request_url().clone());
    let direct_url = DatabaseUrl::new(config.database.direct_url().clone());
    let registry = connect_registry(&config, &direct_url).await?;
    let process_tasks = TaskSupervisor::process(Handle::current());
    // Maintenance holds a session advisory lock across a whole sweep, so it gets
    // its own direct platform pool rather than sharing the request-path pool.
    let maintenance_platform_pool = configure_pool_options(MAINTENANCE_PLATFORM_POOL_CONNECTIONS, config.database.acquire_timeout())
        .connect(config.database.direct_url().as_str())
        .await?;
    register_postgresql_maintenance(
        &process_tasks,
        maintenance_platform_pool,
        direct_url.clone(), // clone: direct_url is still needed to build the tenant factory.
        config.database.maintenance,
        config.database.acquire_timeout(),
        expected_deployment(&config),
    )?;
    // The platform route from minted subscription identifiers to tenants is
    // shared by every tenant's forwarding service and by the receiver.
    let forwarded_subscription_router: Arc<dyn ForwardedSubscriptionRouter> = Arc::new(PostgresForwardedSubscriptionRouter::new(registry.main_pool()));
    // The dormant-tenant wake ports: the probe reads a tenant's own database
    // over a transient connection, and the schedule lives in the platform
    // database beside the registry.
    let pending_work_probe = pending_work_probe(
        &TenantProbeUrls::new(
            request_url.clone(), // clone: request_url is still needed to build the tenant factory.
            direct_url.clone(),  // clone: direct_url is still needed to build the tenant factory.
        ),
        &config,
    )?;
    let tenant_usage_probe = tenant_usage_probe(request_url.clone(), &config); // clone: request_url is still needed to build the tenant factory.
    let wake_schedule: Arc<dyn TenantWakeSchedule> = Arc::new(PostgresTenantWakeSchedule::new(registry.main_pool().clone()));
    // Every open records the extension versions it left, and the waker reads
    // the record to bring dormant tenants up to date after an image upgrade.
    let extension_versions = Arc::new(PostgresTenantExtensionVersions::new(registry.main_pool().clone()));
    let factory = Arc::new(BrokerTenantFactory::new(
        &config,
        source_identity.alias.clone(), // clone: source_identity is consumed by the route assembly.
        request_url,
        direct_url,
        process_tasks.clone(),
        Arc::clone(&forwarded_subscription_router),
        Arc::clone(&extension_versions),
    ));
    let tenants = start_tenancy(
        &config.tenancy,
        &process_tasks,
        &registry,
        &factory,
        pending_work_probe,
        wake_schedule,
        extension_versions,
    )
    .await?;

    let health = broker_health(&registry, &factory, &process_tasks, &tenants);
    let component_reports = collect_component_reports(registry.main_pool(), &config).await?;

    let enabled_extensions = EnabledExtensions::from(&config.extensions);
    log_conformance_override(&config.extensions);
    let pagination = PaginationPolicy::from(&config.extensions);

    let accept_language = accept_language_reading(enabled_extensions);
    let entity_routes = EntityRoutes::new(
        Arc::new(BrokerEntityStateResolver::new(Arc::clone(&tenants))),
        enabled_extensions,
        accept_language,
    );
    let temporal_routes = TemporalRoutes::new(
        Arc::new(BrokerTemporalStateResolver::new(Arc::clone(&tenants))),
        enabled_extensions,
        accept_language,
    );
    let context_registry_routes = ContextRegistryRoutes::new(Arc::new(BrokerContextRegistryStateResolver::new(Arc::clone(&tenants))));
    let registration_routes = RegistrationRoutes::new(Arc::new(BrokerRegistrationStateResolver::new(Arc::clone(&tenants))), accept_language);
    let subscription_routes = SubscriptionRoutes::new(Arc::new(BrokerSubscriptionStateResolver::new(Arc::clone(&tenants))));
    let entity_map_routes = EntityMapRoutes::new(Arc::new(BrokerEntityMapStateResolver::new(Arc::clone(&tenants))), enabled_extensions);
    let snapshot_routes = SnapshotRoutes::new(Arc::new(BrokerSnapshotStateResolver::new(Arc::clone(&tenants))));
    let telemetry_routes = telemetry.map(TelemetryRoutes::new).transpose()?;
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
        .source_identity(source_identity_routes(source_identity, started_at, enabled_extensions, &tenants)?)
        .notification_receiver(NotificationReceiverRoutes::new(Arc::new(BrokerNotificationReceiverResolver::new(
            Arc::clone(&tenants),
            forwarded_subscription_router,
        ))))
        .tenancy(TenancyRoutes::new(tenant_administration, tenant_usage_probe, tenant_management))
        .component_report(ComponentReportRoutes::new(component_reports))
        .configured_api_root(config.server.public_url.clone().map(ConfiguredApiRoot::new))
        .pagination(pagination)
        .build();
    let stack = HttpStack::new(config.http.clone(), config.request_limits)?;
    let (api, management) = assemble_planes(config.management.mode, routes, health, telemetry_routes, &stack);
    Ok(BrokerExecution {
        api,
        management,
        tenants,
        process_tasks,
    })
}
