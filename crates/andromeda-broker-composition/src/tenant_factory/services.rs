//! Construction of one tenant's request-facing service graph.

use crate::{
    blocking_executor::TokioBlockingExecutor,
    event::{entity_change_sink::BroadcastEntityChangeSink, registration_change_sink::BroadcastRegistrationChangeSink},
    linked_forwarding::LinkedForwarding,
    local_entity_type::EntityStoreTypeResolver,
    registered_entity_probe::EntityStoreRegistrationProbe,
    retry_delay::TokioRetryDelay,
    tenant_factory::{
        TenantFactoryConfig,
        assembly_invariant::AssemblyInvariant,
        error::TenantAssemblyError,
        storage::TenantStorage,
        subscription_services::{DistributedSubscriptionEdge, compose_subscription_services},
        tasks::TenantSignals,
        temporal_history::TemporalPorts,
        transport::TenantTransports,
    },
    tenant_state::AndromedaState,
};
use andromeda_configuration_scalars::human_byte_size::HumanByteSize;
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_registry::{service::ContextRegistryService, store::ContextRegistryStore};
use andromeda_entity::{
    EntityService,
    NotificationWriteConfig,
    batch::execution::BatchExecutionConfig,
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    change_sink::EntityChangeSink,
    discovery::EntityDiscoveryService,
    port::{attribute_history::AttributeDatasetHistory, service_ports::EntityServicePorts},
    query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource},
};
use andromeda_entity_map::service::EntityMapService;
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_federation::FederationService;
use andromeda_http_context_registry::tenant_context_hosting::TenantContextHosting;
use andromeda_linked_retrieval::admission::TemporalJoin;
use andromeda_ngsi_ld_query::geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand};
use andromeda_postgresql_context_store::jsonld_context::repo::JsonLdContextRepository;
use andromeda_registration::{RegistrationService, entity_probe::RegisteredEntityProbe, removal_hook::RegistrationRemovalHook};
use andromeda_snapshot::{
    executor::{SnapshotPreparation, SnapshotPreparationConfig, capture_sources::SnapshotCaptureSources},
    service::{SnapshotService, SnapshotServiceConfig},
    status_delivery_signal::StatusDeliverySignal,
};
use andromeda_subscription_forwarding::{mapping::ForwardedSubscriptionRouter, service::SubscriptionForwardingService};
use andromeda_temporal::{
    port::{mutator::TemporalMutator, service_ports::TemporalServicePorts},
    remote_linked::RemoteTemporalLinkedSource,
    service::TemporalService,
    service_limits::TemporalServiceLimits,
};
use andromeda_tenancy::context_source_alias::ContextSourceAliases;
use std::sync::Arc;

/// Request-facing state and the scheduled snapshot preparation operation composed with it.
pub(super) struct ConstructedServices {
    pub(super) state: AndromedaState,
    pub(super) snapshot_preparation: Arc<SnapshotPreparation>,
}

/// Broker-level collaborators handed down to every tenant's services: the atomic
/// commit implementation each write path reaches, the router outgoing forwarded
/// subscriptions traverse, and the wake signal that keeps create-time and
/// updated Snapshot notifications prompt.
pub(super) struct TenantServiceCollaborators<'a> {
    /// The complete atomic commit implementation every write path reaches.
    pub(super) writes: &'a Arc<dyn ContextCommitter>,
    /// The router outgoing forwarded subscriptions traverse.
    pub(super) subscription_router: &'a Arc<dyn ForwardedSubscriptionRouter>,
    /// Wakes the Snapshot status-delivery drain after a create-time preparation
    /// or a metadata update enqueues notifications outside the scheduled sweep.
    pub(super) snapshot_delivery_signal: Arc<dyn StatusDeliverySignal>,
}

/// Construct all request-facing services for one tenant.
pub(super) fn construct_services(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    signals: &TenantSignals,
    transports: &TenantTransports,
    temporal_ports: &TemporalPorts,
    collaborators: TenantServiceCollaborators<'_>,
) -> Result<ConstructedServices, TenantAssemblyError> {
    let TenantServiceCollaborators {
        writes,
        subscription_router,
        snapshot_delivery_signal,
    } = collaborators;
    let notifier = compose_change_notifier(config, storage, signals);
    let entities = compose_entity_service(config, storage, &notifier, writes);
    // Table 5.2.40-1: the alias this tenant writes into the outgoing `Via`
    // chain identifies the tenant within this context source, so a request that
    // has traversed one tenant is not mistaken for a loop at another (clause
    // 4.3.6.4 considers each Tenant of a registered Context Source separately).
    let aliases = ContextSourceAliases::new(config.identity.source_alias.clone()); // clone: the alias family owns the configured default-tenant alias.
    // Forwarding is constructed ahead of the services that join through it:
    // both the entity and the temporal join resolve an `objectType`-annotated
    // Linked Entity this broker does not hold through the same registrations.
    let forwarding = Arc::new(
        FederationService::new(
            Arc::clone(&storage.registration_store),
            Arc::new(EntityStoreTypeResolver::new(Arc::clone(&storage.entity_ports.entity_types))),
            storage.engine.clone(), // clone: each tenant service owns its JSON-LD engine handle.
            aliases.for_tenant(&storage.tenant),
            &config.federation,
        )
        .map_err(|error| AssemblyInvariant::Federation(Box::new(error)))?,
    );
    let temporal = compose_temporal_service(config, storage, temporal_ports, &notifier, writes, &forwarding);
    let entity_query = compose_entity_query_service(config, storage, &forwarding);
    let context_store = Arc::new(JsonLdContextRepository::new(&storage.pool)) as Arc<dyn ContextRegistryStore>;
    let contexts = Arc::new(ContextRegistryService::new(context_store, storage.engine.clone()));
    let context_hosting = TenantContextHosting::new(contexts);
    let (snapshot_preparation, snapshots) = compose_snapshot_services(config, storage, writes, &entity_query, &temporal, &forwarding, snapshot_delivery_signal);
    let edge = DistributedSubscriptionEdge::resolve(&config.identity, subscription_router, &forwarding, &storage.tenant);
    let subscriptions = compose_subscription_services(
        storage,
        signals,
        transports,
        edge,
        &entity_query,
        geo_query_admission(config),
        config.extensions.resolve().language_targeting(),
    );
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
        context_hosting,
        csr: compose_registration_service(storage, signals, subscriptions.forwarding.as_ref(), geo_query_admission(config)),
        csr_subscriptions: subscriptions.csource,
        forwarding,
        entity_maps: Arc::new(EntityMapService::new(Arc::clone(&storage.entity_map_store))),
        subscription_forwarding: subscriptions.forwarding,
        snapshots,
        sse_registry: Arc::clone(&transports.sse_registry),
        sse_channel_buffer_size: config.notifications.streaming.sse.channel_buffer_size,
        sse_keep_alive_interval_secs: config.notifications.streaming.sse.keep_alive_interval.duration().as_secs(),
        ws_registry: Arc::clone(&transports.ws_registry),
        ws_channel_buffer_size: config.notifications.streaming.ws.channel_buffer_size,
        ws_ping_interval_secs: config.notifications.streaming.ws.ping_interval.duration().as_secs(),
    };
    Ok(ConstructedServices { state, snapshot_preparation })
}

/// Compose the one notifier every policy that changes current state raises its
/// notifications through, so a change is notified the same way whichever API
/// made it (clause 5.8.6).
///
/// A durable notification write is configured only when delivery is durable;
/// with live delivery the notification worker evaluates subscriptions itself.
fn compose_change_notifier(config: &TenantFactoryConfig, storage: &TenantStorage, signals: &TenantSignals) -> Arc<ChangeNotifier> {
    let entity_change_sink = Arc::new(BroadcastEntityChangeSink::new(signals.entity.clone())) as Arc<dyn EntityChangeSink>; // clone: the runtime and policy each own a sender handle.
    let subscription_cache = Arc::clone(&storage.subscription_cache);
    let matching = if config.notifications.delivery.is_durable() {
        SubscriptionMatching::AtCommit(NotificationWriteConfig { subscription_cache })
    } else {
        SubscriptionMatching::AtDelivery(subscription_cache)
    };
    Arc::new(ChangeNotifier::new(Some(entity_change_sink), matching))
}

/// Compose the entity policy service every write and read request enters.
///
/// The batch activity tracker is created here because the entity policy is its
/// only observer: the batch paths record on it and entity expiry consults it
/// through the same service.
fn compose_entity_service(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    notifier: &Arc<ChangeNotifier>,
    writes: &Arc<dyn ContextCommitter>,
) -> Arc<EntityService> {
    let attribute_history = Arc::clone(&storage.temporal_repo) as Arc<dyn AttributeDatasetHistory>; // clone: temporal and entity policy share the concrete history adapter.
    Arc::new(EntityService::new(
        EntityServicePorts {
            reader: Arc::clone(&storage.entity_ports.reader),
            blocking_executor: Arc::new(TokioBlockingExecutor),
            retry_delay: Arc::new(TokioRetryDelay),
            attribute_history,
            writes: Arc::clone(writes),
            deletion_outline: Arc::clone(&storage.entity_ports.deletion_outline),
        },
        storage.engine.clone(), // clone: entity policy owns its engine handle.
        Arc::clone(notifier),
        BatchExecutionConfig {
            chunk_concurrency: config.entity.batch_chunk_concurrency.unwrap_or(4),
            chunk_write_concurrency: config.entity.batch_chunk_write_concurrency.unwrap_or(4),
            chunk_size: config.entity.batch_chunk_size,
            large_threshold: config.entity.batch_large_threshold,
            prepare_blocking_threshold: config.entity.batch_prepare_blocking_threshold,
            async_side_effect_threshold: config.entity.batch_async_side_effect_threshold,
        },
        config.entity.payload_budget(),
    ))
}

/// Compose the temporal service the temporal API is answered from.
fn compose_temporal_service(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    ports: &TemporalPorts,
    notifier: &Arc<ChangeNotifier>,
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
            retry_delay: Arc::new(TokioRetryDelay),
        },
        storage.engine.clone(), // clone: temporal policy owns its engine handle.
        Arc::clone(notifier),
        TemporalServiceLimits {
            instance_limits: config.temporal.instance_limits(),
            // The delete-by-query extension's `TooManyResults` bound; inert
            // unless the extension is routed, which composition gates
            // separately.
            delete_by_query_max_matches: config.extensions.temporal_delete_by_query_max_matches,
            // The grouped-aggregation extension's `TooManyResults` bound;
            // likewise inert unless the extension is routed.
            aggregation_grouping_max_groups: config.extensions.temporal_aggregation_grouping_max_groups,
            // A temporal write is admitted under the budget a Core write is.
            payload_budget: config.entity.payload_budget(),
            near_band: near_distance_band(config.extensions.resolve()),
            language_targeting: config.extensions.resolve().language_targeting(),
        },
        // Clause 4.5.23.1 applied to the temporal API: an annotated Linked
        // Entity whose history this broker does not record is fetched from the
        // context sources registered for it.
        Some(Arc::new(LinkedForwarding::new(
            Arc::clone(forwarding),      // clone: request handling and linked resolution share one forwarding service.
            storage.live_target.clone(), // clone: linked resolution runs against the tenant's live branch.
        )) as Arc<dyn RemoteTemporalLinkedSource>),
        temporal_linked_retrieval(config.extensions.resolve()),
    ))
}

/// Whether the temporal operations retrieve Linked Entities: only with the
/// `temporal_linked_retrieval` extension, which conformance mode forces off.
fn temporal_linked_retrieval(enabled: EnabledExtensions) -> TemporalJoin {
    if enabled.temporal_linked_retrieval_enabled() {
        TemporalJoin::Extended
    } else {
        TemporalJoin::Spec
    }
}

/// Whether a `near` may carry both distances: only with the
/// `near_distance_band` extension, which conformance mode forces off.
fn near_distance_band(enabled: EnabledExtensions) -> NearDistanceBand {
    if enabled.near_distance_band_enabled() {
        NearDistanceBand::Admitted
    } else {
        NearDistanceBand::Refused
    }
}

/// What every geo-query a request of the tenant writes is admitted under.
fn geo_query_admission(config: &TenantFactoryConfig) -> GeoQueryAdmission {
    GeoQueryAdmission::new(config.entity.geometry_census_limits(), near_distance_band(config.extensions.resolve()))
}

/// Compose the entity query service every entity read is answered from.
fn compose_entity_query_service(config: &TenantFactoryConfig, storage: &TenantStorage, forwarding: &Arc<FederationService>) -> Arc<EntityQueryService> {
    Arc::new(
        EntityQueryService::builder()
            .reader(Arc::clone(&storage.entity_ports.reader))
            .query_store(Arc::clone(&storage.entity_ports.query))
            .engine(storage.engine.clone()) // clone: the query service owns its engine handle.
            .spatial(storage.spatial.clone()) // clone: request reads narrow through the tenant's one index.
            .tenant(storage.tenant.clone()) // clone: the query service owns its tenant partition.
            // Clause 4.5.23.1: an `objectType`-annotated Linked Entity this broker
            // does not store is fetched from the context sources registered for it.
            .remote_linked(Some(Arc::new(LinkedForwarding::new(
                Arc::clone(forwarding),      // clone: request handling and linked resolution share one forwarding service.
                storage.live_target.clone(), // clone: linked resolution runs against the tenant's live branch.
            )) as Arc<dyn RemoteLinkedEntitySource>))
            .geo_admission(geo_query_admission(config))
            .language_targeting(config.extensions.resolve().language_targeting())
            .build(),
    )
}

/// Compose the snapshot preparation operation and the service scheduling it.
fn compose_snapshot_services(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    writes: &Arc<dyn ContextCommitter>,
    entity_query: &Arc<EntityQueryService>,
    temporal: &Arc<TemporalService>,
    forwarding: &Arc<FederationService>,
    delivery_signal: Arc<dyn StatusDeliverySignal>,
) -> (Arc<SnapshotPreparation>, Arc<SnapshotService>) {
    let snapshot = &config.snapshot;
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
            capture_page_size: snapshot.capture_page_size,
            max_per_pass: snapshot.preparation_concurrency,
            lease_duration: snapshot.preparation_lease.duration(),
            tenant_count_quota: snapshot.tenant_count_quota,
            tenant_byte_quota: snapshot.tenant_storage_quota.map(HumanByteSize::as_u64),
            attribute_nesting: config.entity.payload_budget().attribute_nesting,
        },
    ));
    let service = Arc::new(SnapshotService::new(
        Arc::clone(&storage.snapshot_store),
        Some(Arc::clone(&preparation)), // clone: the service schedules the same preparation the worker drives.
        Some(delivery_signal),
        SnapshotServiceConfig {
            default_lifetime: snapshot.default_lifetime.duration(),
            maximum_lifetime: snapshot.maximum_lifetime.duration(),
            scoped_operation_wait: snapshot.scoped_operation_wait.duration(),
            tenant_count_quota: snapshot.tenant_count_quota,
            tenant_byte_quota: snapshot.tenant_storage_quota.map(HumanByteSize::as_u64),
        },
    ));
    (preparation, service)
}

/// Compose registration policy with the admission probe clauses 5.9.2.4 and
/// 5.9.3.4 require -- a proxied registration is refused when this broker
/// already holds the context data it claims -- and, when subscriptions are
/// forwarded, the teardown of the copies a removed registration's source holds.
fn compose_registration_service(
    storage: &TenantStorage,
    signals: &TenantSignals,
    forwarding: Option<&Arc<SubscriptionForwardingService>>,
    geo_admission: GeoQueryAdmission,
) -> Arc<RegistrationService> {
    let probe = Arc::new(EntityStoreRegistrationProbe::new(
        Arc::clone(&storage.entity_ports.query),
        storage.engine.clone(),      // clone: the probe compiles registration selectors against its own engine handle.
        storage.live_target.clone(), // clone: admission inspects the tenant's live branch.
    )) as Arc<dyn RegisteredEntityProbe>;
    let service = RegistrationService::new(
        Arc::clone(&storage.registration_store),
        probe,
        storage.engine.clone(), // clone: registration policy owns its engine handle.
        Some(Arc::new(BroadcastRegistrationChangeSink::new(signals.registration.clone()))), // clone: each tenant service and worker own a sender handle.
        geo_admission,
    );
    Arc::new(match forwarding {
        // clone: request handling, the expiry sweep and subscription forwarding share one service.
        Some(forwarding) => service.with_removal_hook(Arc::clone(forwarding) as Arc<dyn RegistrationRemovalHook>),
        None => service,
    })
}
