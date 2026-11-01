//! Broker composition and process-lifecycle coordination.

use crate::{
    component_report::{ComponentReportError, collect as collect_component_reports},
    configuration::section_admission::{database_defaults, expected_deployment},
    extension_routing::{accept_language_reading, tenant_management_mode},
    listeners,
    logging,
    outcome::ProcessExitOutcome,
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
    shutdown_signal::shutdown_signal,
    startup_report::report_startup,
    tenant_factory::BrokerTenantFactory,
    tenant_waker::{TenantWakerRegistrationError, register_tenant_waker},
};
use andromeda_configuration::{
    broker_config::BrokerConfig,
    schema::{observability::MetricsMode, temporal::storage::StorageDaysError},
};
use andromeda_delivery::mqtt::indicator::MqttHealthIndicator;
use andromeda_extensions::{enabled::EnabledExtensions, startup_log::log_conformance_override};
use andromeda_health::aggregate::Health;
use andromeda_http_api::{api::HttpApi, management::ManagementApi, routes::ApiRoutes};
use andromeda_http_component_report::routes::ComponentReportRoutes;
use andromeda_http_context_registry::routes::ContextRegistryRoutes;
use andromeda_http_entity::routes::EntityRoutes;
use andromeda_http_entity_map::routes::EntityMapRoutes;
use andromeda_http_extraction::api_root::ConfiguredApiRoot;
use andromeda_http_health::routes::HealthRoutes;
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
    health::DatabaseHealthIndicator,
    pending_work::PostgresPendingWorkProbe,
    tenant_registry::PostgresTenantRegistry,
    tenant_usage::PostgresTenantUsageProbe,
    tenant_wake_schedule::PostgresTenantWakeSchedule,
};
use andromeda_postgresql_pool::{database_url::DatabaseUrl, pool_options::configure_pool_options};
use andromeda_postgresql_temporal_store::{
    history_due::HistorySchedule,
    history_work_probe::PostgresHistoryWorkProbe,
    retention_sweep::TemporalRetentionHorizonInvalid,
};
use andromeda_subscription_forwarding::mapping::ForwardedSubscriptionRouter;
use andromeda_task_runtime::supervisor::TaskSupervisor;
use andromeda_telemetry::{error::TelemetryError, recorder::TelemetryRecorder};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use andromeda_tenancy::{
    administration::TenantAdministration,
    creation_policy::TenantCreationPolicy,
    error::TenancyError,
    pending_work::PendingWorkProbe,
    pending_work_sources::PendingWorkSources,
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
pub(crate) const MAINTENANCE_PLATFORM_POOL_CONNECTIONS: u32 = 2;

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
    /// The `/info/sourceIdentity` route could not be built from the configured identity.
    #[error(transparent)]
    SourceIdentity(#[from] SourceIdentityRouteError),
    /// A `temporal.storage` horizon is not a positive whole number of days.
    #[error("invalid configuration: {0}")]
    TemporalStorage(#[from] StorageDaysError),
    /// The retention horizon reaches past the representable instants.
    #[error("invalid configuration: {0}")]
    TemporalRetention(#[from] TemporalRetentionHorizonInvalid),
}

/// Construct the broker, serve HTTP, then close process resources in order.
pub async fn run(config: BrokerConfig) -> ProcessExitOutcome {
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
    let telemetry = match install_telemetry(config.observability.metrics.mode) {
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
fn install_telemetry(exposure: MetricsMode) -> Result<Option<Arc<TelemetryRecorder>>, BrokerStartupError> {
    match exposure {
        MetricsMode::Disabled => Ok(None),
        MetricsMode::Prometheus => Ok(Some(Arc::new(TelemetryRecorder::install()?))),
    }
}

/// The health the probes report: the main database and, when the default
/// tenant delivers over MQTT, its MQTT pool.
fn broker_health(registry: &PostgresTenantRegistry, factory: &BrokerTenantFactory) -> Health {
    let database_indicator = DatabaseHealthIndicator::new(registry.main_pool().clone()); // clone: PgPool is a shared handle
    let mut health = Health::builder().with_indicator(database_indicator);
    if let Some(mqtt_pool) = factory.default_mqtt_pool() {
        health = health.with_indicator(MqttHealthIndicator::new(mqtt_pool));
    }
    health.build()
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
fn tenant_usage_probe(request_url: DatabaseUrl, config: &BrokerConfig) -> Arc<dyn TenantUsageProbe> {
    Arc::new(PostgresTenantUsageProbe::new(
        request_url,
        config.database.acquire_timeout(),
        config.snapshot.quota.max_per_tenant.map(|quota| u64::try_from(quota.get()).unwrap_or(u64::MAX)),
    ))
}

/// The dormant-tenant wake probe: every store answering for the work it owns.
///
/// The context store answers first, for its queues, expiries and periodic
/// notifications, and so vouches for the tenant's schema before the temporal
/// store reads its history chunks for the conversion and retention sweeps. Each
/// reads over a transient request-path connection of its own.
///
/// # Errors
///
/// Returns [`BrokerStartupError::TemporalStorage`] or
/// [`BrokerStartupError::TemporalRetention`] when `temporal.storage` names no
/// usable sweep horizon, which configuration validation refuses first.
fn pending_work_probe(request_url: &DatabaseUrl, config: &BrokerConfig) -> Result<Arc<dyn PendingWorkProbe>, BrokerStartupError> {
    let history = HistorySchedule::try_from(&TemporalStoragePolicy::try_from(&config.temporal.storage)?)?;
    Ok(Arc::new(PendingWorkSources::new(vec![
        Arc::new(PostgresPendingWorkProbe::new(
            request_url.clone(), // clone: each probe derives tenant URLs from its own copy of the request base
            config.database.acquire_timeout(),
        )),
        Arc::new(PostgresHistoryWorkProbe::new(
            request_url.clone(), // clone: each probe derives tenant URLs from its own copy of the request base
            config.database.acquire_timeout(),
            history,
        )),
    ])))
}

/// Construct tenant composition, focused route fragments and the complete HTTP API.
async fn construct(
    config: Arc<BrokerConfig>,
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
    let registry = Arc::new(
        PostgresTenantRegistry::connect(
            configure_pool_options(config.database.max_connections, config.database.acquire_timeout()),
            direct_url.clone(),               // clone: the registry provisions and migrates over the direct base
            config.tenancy.databases.clone(), // clone: the registry owns where tenant data lives
            &database_defaults(&config),
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
        expected_deployment(&config),
    )?;
    // The platform route from minted subscription identifiers to tenants is
    // shared by every tenant's forwarding service and by the receiver.
    let forwarded_subscription_router: Arc<dyn ForwardedSubscriptionRouter> = Arc::new(PostgresForwardedSubscriptionRouter::new(registry.main_pool()));
    // The dormant-tenant wake ports: the probe reads a tenant's own database
    // over a transient request-path connection, and the schedule lives in the
    // platform database beside the registry.
    let pending_work_probe = pending_work_probe(&request_url, &config)?;
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

    let health = broker_health(&registry, &factory);
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
        .source_identity(source_identity_routes(&config.federation.identity, started_at, enabled_extensions, &tenants)?)
        .notification_receiver(NotificationReceiverRoutes::new(Arc::new(BrokerNotificationReceiverResolver::new(
            Arc::clone(&tenants),
            forwarded_subscription_router,
        ))))
        .tenancy(TenancyRoutes::new(tenant_administration, tenant_usage_probe, tenant_management))
        .health(HealthRoutes::new(health.clone())) // clone: the data plane and the management plane each own a health-route fragment over the same indicators
        .component_report(ComponentReportRoutes::new(component_reports))
        .configured_api_root(config.server.public_url.clone().map(ConfiguredApiRoot::new)) // clone: clause 6.2, the configured apiRoot every minted URL is built on
        .pagination(pagination)
        .build();
    let stack = HttpStack::new(config.http.clone(), config.request_limits)?; // clone: the assembled stack owns its process-wide configuration
    let (api, management) = assemble_planes(config.management.mode, routes, health, telemetry_routes, &stack);
    Ok(BrokerExecution {
        api,
        management,
        tenants,
        process_tasks,
    })
}
