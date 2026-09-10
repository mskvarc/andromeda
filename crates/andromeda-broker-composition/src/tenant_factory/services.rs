//! Construction of one tenant's request-facing service graph.

use crate::{
    blocking_executor::TokioBlockingExecutor,
    event::{entity_change_sink::BroadcastEntityChangeSink, registration_change_sink::BroadcastRegistrationChangeSink},
    linked_entity_forwarding::ForwardedLinkedEntitySource,
    registered_entity_probe::EntityStoreRegistrationProbe,
    retry_delay::TokioRetryDelay,
    temporal_linked_forwarding::ForwardedTemporalLinkedSource,
    tenant_factory::{
        TenantFactoryConfig,
        error::TenantAssemblyError,
        storage::TenantStorage,
        subscription_services::{DistributedSubscriptionEdge, compose_subscription_services},
        tasks::TenantSignals,
        temporal_history::TemporalPorts,
        transport::TenantTransports,
    },
    tenant_state::AndromedaState,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_registry::{service::ContextRegistryService, store::ContextRegistryStore};
use andromeda_entity::{
    EntityService,
    NotificationWriteConfig,
    batch::execution::BatchExecutionConfig,
    batch_activity::BatchActivityTracker,
    change_sink::EntityChangeSink,
    discovery::EntityDiscoveryService,
    port::{attribute_history::AttributeDatasetHistory, service_ports::EntityServicePorts},
    query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource},
};
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_postgresql_context_store::jsonld_context::repo::JsonLdContextRepository;
use andromeda_registration::{RegistrationService, entity_probe::RegisteredEntityProbe};
use andromeda_snapshot::{
    config::SnapshotConfig,
    executor::{SnapshotPreparation, SnapshotPreparationConfig, capture_sources::SnapshotCaptureSources},
    service::SnapshotService,
};
use andromeda_subscription_forwarding::mapping::ForwardedSubscriptionRouter;
use andromeda_temporal::{
    port::{mutator::TemporalMutator, service_ports::TemporalServicePorts},
    remote_linked::RemoteTemporalLinkedSource,
    service::TemporalService,
};
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use std::{sync::Arc, time::Duration};

/// Request-facing state and the scheduled snapshot preparation operation composed with it.
pub(super) struct ConstructedServices {
    pub(super) state: AndromedaState,
    pub(super) snapshot_preparation: Arc<SnapshotPreparation>,
}

/// Construct all request-facing services for one tenant.
pub(super) fn construct_services(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    signals: &TenantSignals,
    transports: &TenantTransports,
    temporal_ports: &TemporalPorts,
    writes: &Arc<dyn ContextCommitter>,
    subscription_router: &Arc<dyn ForwardedSubscriptionRouter>,
) -> Result<ConstructedServices, TenantAssemblyError> {
    let pool_size = match storage.tenant {
        TenantRef::UnnamedDefault => config.database.max_connections as usize,
        TenantRef::Named(_) => config.tenancy.tenant_max_connections as usize,
    };
    let entities = compose_entity_service(config, storage, signals, writes, pool_size);
    // Table 5.2.40-1: the alias this tenant writes into the outgoing `Via`
    // chain identifies the tenant within this context source, so a request that
    // has traversed one tenant is not mistaken for a loop at another (clause
    // 4.3.6.4 considers each Tenant of a registered Context Source separately).
    let aliases = ContextSourceAliases::new(config.info.source_alias.clone()); // clone: the alias family owns the configured default-tenant alias.
    // Forwarding is constructed ahead of the services that join through it:
    // both the entity and the temporal join resolve an `objectType`-annotated
    // Linked Entity this broker does not hold through the same registrations.
    let forwarding = Arc::new(
        FederationService::new(
            Arc::clone(&storage.registration_store),
            storage.engine.clone(), // clone: each tenant service owns its JSON-LD engine handle.
            aliases.for_tenant(&storage.tenant),
            &config.federation,
        )
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?,
    );
    let temporal = compose_temporal_service(config, storage, temporal_ports, writes, &forwarding);
    let entity_query = compose_entity_query_service(storage, &forwarding);
    let context_store = Arc::new(JsonLdContextRepository::new(&storage.pool)) as Arc<dyn ContextRegistryStore>;
    let contexts = Arc::new(ContextRegistryService::new(context_store, storage.engine.clone()));
    let (snapshot_preparation, snapshots) = compose_snapshot_services(&config.snapshot, storage, writes, &entity_query, &temporal, &forwarding);
    let edge = DistributedSubscriptionEdge::resolve(&config.info, subscription_router, &forwarding, &storage.tenant);
    let subscriptions = compose_subscription_services(storage, signals, transports, edge, &entity_query, &contexts);
    let state = AndromedaState {
        live_target: storage.live_target.clone(), // clone: request state owns the immutable typed live-branch selection.
        entities,
        entity_query,
        entity_discovery: Arc::new(EntityDiscoveryService::new(
            Arc::clone(&storage.entity_ports.discovery),
            Arc::clone(&storage.registry_type_source),
            storage.engine.clone(), // clone: discovery owns its engine handle.
        )),
        temporal,
        subscriptions: subscriptions.entity,
        contexts,
        csr: compose_registration_service(storage, signals),
        csr_subscriptions: subscriptions.csource,
        forwarding,
        entity_maps: Arc::new(EntityMapService::new(Arc::clone(&storage.entity_map_store))),
        subscription_forwarding: subscriptions.forwarding,
        snapshots,
        sse_registry: Arc::clone(&transports.sse_registry),
        sse_channel_buffer_size: config.notifications.sse.channel_buffer_size,
        sse_keep_alive_interval_secs: config.notifications.sse.keep_alive_interval_secs,
        ws_registry: Arc::clone(&transports.ws_registry),
        ws_channel_buffer_size: config.notifications.ws.channel_buffer_size,
        ws_ping_interval_secs: config.notifications.ws.ping_interval_secs,
    };
    Ok(ConstructedServices { state, snapshot_preparation })
}

/// Compose the entity policy service every write and read request enters.
///
/// The batch activity tracker is created here because the entity policy is its
/// only observer: the batch paths record on it and entity expiry consults it
/// through the same service.
///
/// A durable notification write is configured only when delivery is enabled;
/// with live delivery the notification worker evaluates subscriptions itself.
fn compose_entity_service(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    signals: &TenantSignals,
    writes: &Arc<dyn ContextCommitter>,
    pool_size: usize,
) -> Arc<EntityService> {
    let entity_change_sink = Arc::new(BroadcastEntityChangeSink::new(signals.entity.clone())) as Arc<dyn EntityChangeSink>; // clone: the runtime and policy each own a sender handle.
    let attribute_history = Arc::clone(&storage.temporal_repo) as Arc<dyn AttributeDatasetHistory>; // clone: temporal and entity policy share the concrete history adapter.
    Arc::new(EntityService::new(
        EntityServicePorts {
            reader: Arc::clone(&storage.entity_ports.reader),
            blocking_executor: Arc::new(TokioBlockingExecutor),
            retry_delay: Arc::new(TokioRetryDelay),
            attribute_history,
            writes: Arc::clone(writes),
        },
        storage.engine.clone(), // clone: entity policy owns its engine handle.
        Some(entity_change_sink),
        BatchExecutionConfig {
            concurrency: config.entity.batch_concurrency.unwrap_or_else(|| usize::min(pool_size, 25)),
            chunk_concurrency: config.entity.batch_chunk_concurrency.unwrap_or(4),
            chunk_write_concurrency: config.entity.batch_chunk_write_concurrency.unwrap_or(4),
            chunk_size: config.entity.batch_chunk_size,
            large_threshold: config.entity.batch_large_threshold,
            prepare_blocking_threshold: config.entity.batch_prepare_blocking_threshold,
            async_side_effect_threshold: config.entity.batch_async_side_effect_threshold,
        },
        BatchActivityTracker::new(),
        config.notification_delivery.enabled.then(|| NotificationWriteConfig {
            subscription_cache: Arc::clone(&storage.subscription_cache),
        }),
    ))
}

/// Compose the temporal service the temporal API is answered from.
fn compose_temporal_service(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    ports: &TemporalPorts,
    writes: &Arc<dyn ContextCommitter>,
    forwarding: &Arc<FederationService>,
) -> Arc<TemporalService> {
    Arc::new(TemporalService::new(
        TemporalServicePorts {
            entity_reader: Arc::clone(&storage.entity_ports.reader),
            reader: Arc::clone(&ports.reader),
            mutation_barrier: Arc::clone(&ports.mutation_barrier),
            mutator: Arc::clone(&storage.temporal_repo) as Arc<dyn TemporalMutator>,
            writes: Arc::clone(writes),
        },
        storage.engine.clone(), // clone: temporal policy owns its engine handle.
        config.temporal.default_last_n,
        // Clause 4.5.23.1 applied to the temporal API: an annotated Linked
        // Entity whose history this broker does not record is fetched from the
        // context sources registered for it.
        Some(Arc::new(ForwardedTemporalLinkedSource::new(
            Arc::clone(forwarding),      // clone: request handling and linked resolution share one forwarding service.
            storage.live_target.clone(), // clone: linked resolution runs against the tenant's live branch.
        )) as Arc<dyn RemoteTemporalLinkedSource>),
    ))
}

/// Compose the entity query service every entity read is answered from.
fn compose_entity_query_service(storage: &TenantStorage, forwarding: &Arc<FederationService>) -> Arc<EntityQueryService> {
    Arc::new(EntityQueryService::new(
        Arc::clone(&storage.entity_ports.reader),
        Arc::clone(&storage.entity_ports.query),
        storage.engine.clone(), // clone: the query service owns its engine handle.
        Arc::clone(&storage.spatial_index),
        storage.tenant.clone(), // clone: the query service owns its tenant partition.
        // Clause 4.5.23.1: an `objectType`-annotated Linked Entity this broker
        // does not store is fetched from the context sources registered for it.
        Some(Arc::new(ForwardedLinkedEntitySource::new(
            Arc::clone(forwarding),      // clone: request handling and linked resolution share one forwarding service.
            storage.live_target.clone(), // clone: linked resolution runs against the tenant's live branch.
        )) as Arc<dyn RemoteLinkedEntitySource>),
    ))
}

/// Compose the snapshot preparation operation and the service scheduling it.
fn compose_snapshot_services(
    config: &SnapshotConfig,
    storage: &TenantStorage,
    writes: &Arc<dyn ContextCommitter>,
    entity_query: &Arc<EntityQueryService>,
    temporal: &Arc<TemporalService>,
    forwarding: &Arc<FederationService>,
) -> (Arc<SnapshotPreparation>, Arc<SnapshotService>) {
    let preparation = Arc::new(SnapshotPreparation::new(
        SnapshotCaptureSources {
            live_target: storage.live_target.clone(), // clone: capture queries execute against the tenant's ordinary live branch.
            entity_query: Arc::clone(entity_query),   // clone: a capture reads through the same query service requests do.
            temporal: Arc::clone(temporal),           // clone: a capture reads history through the same temporal service.
            forwarding: Arc::clone(forwarding),       // clone: a capture reaches context sources through one forwarding service.
        },
        Arc::clone(&storage.snapshot_store),
        Arc::clone(writes),
        SnapshotPreparationConfig {
            capture_page_size: config.capture_page_size,
            max_per_pass: config.preparation_concurrency,
            lease_duration: Duration::from_secs(config.preparation_lease_secs),
            tenant_count_quota: config.tenant_count_quota,
            tenant_byte_quota: config.tenant_byte_quota,
        },
    ));
    let service = Arc::new(SnapshotService::new(
        Arc::clone(&storage.snapshot_store),
        Some(Arc::clone(&preparation)), // clone: the service schedules the same preparation the worker drives.
        Duration::from_secs(config.default_lifetime_secs),
        Duration::from_secs(config.maximum_lifetime_secs),
        Duration::from_secs(config.scoped_operation_wait_secs),
        config.tenant_count_quota,
        config.tenant_byte_quota,
    ));
    (preparation, service)
}

/// Compose registration policy with the admission probe clauses 5.9.2.4 and
/// 5.9.3.4 require: a proxied registration is refused when this broker already
/// holds the context data it claims.
fn compose_registration_service(storage: &TenantStorage, signals: &TenantSignals) -> Arc<RegistrationService> {
    let probe = Arc::new(EntityStoreRegistrationProbe::new(
        Arc::clone(&storage.entity_ports.reader),
        Arc::clone(&storage.entity_ports.query),
        storage.engine.clone(),      // clone: the probe compiles registration selectors against its own engine handle.
        storage.live_target.clone(), // clone: admission inspects the tenant's live branch.
    )) as Arc<dyn RegisteredEntityProbe>;
    Arc::new(RegistrationService::new(
        Arc::clone(&storage.registration_store),
        probe,
        storage.engine.clone(), // clone: registration policy owns its engine handle.
        Some(Arc::new(BroadcastRegistrationChangeSink::new(signals.registration.clone()))), // clone: each tenant service and worker own a sender handle.
    ))
}
