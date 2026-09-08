//! Construction of one tenant's request-facing service graph.

use crate::{
    blocking_executor::TokioBlockingExecutor,
    event::{
        entity_change_sink::BroadcastEntityChangeSink,
        registration_change_sink::BroadcastRegistrationChangeSink,
        subscription_change_sink::{BroadcastSubscriptionChangeSink, CoherentSubscriptionChangeSink},
        subscription_endpoint_validator::DeliverySubscriptionEndpointValidator,
    },
    linked_entity_forwarding::ForwardedLinkedEntitySource,
    registered_entity_probe::EntityStoreRegistrationProbe,
    retry_delay::TokioRetryDelay,
    temporal_linked_forwarding::ForwardedTemporalLinkedSource,
    tenant_factory::{TenantFactoryConfig, error::TenantAssemblyError, storage::TenantStorage, tasks::TenantSignals, transport::TenantTransports},
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
    port::attribute_history::AttributeDatasetHistory,
    query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource},
};
use andromeda_entity_map::service::EntityMapService;
use andromeda_federation::FederationService;
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_postgresql_context_store::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    jsonld_context::repo::JsonLdContextRepository,
    subscription::repo::SubscriptionRepository,
    subscription_forwarding::repo::SubscriptionForwardingRepository,
};
use andromeda_registration::{RegistrationService, entity_probe::RegisteredEntityProbe};
use andromeda_snapshot::{
    config::SnapshotConfig,
    executor::{SnapshotPreparation, SnapshotPreparationConfig},
    service::SnapshotService,
};
use andromeda_subscription::{
    csource::CsrSubscriptionVariant,
    entity::EntitySubscriptionVariant,
    forwarding_hook::SubscriptionForwardingHook,
    lifecycle::{SubscriptionEndpointValidator, SubscriptionService},
    store::SubscriptionStore,
};
use andromeda_subscription_forwarding::{
    mapping::{ForwardedSubscriptionRouter, SubscriptionForwardingStore},
    receiver::ReceiverEndpoint,
    service::SubscriptionForwardingService,
};
use andromeda_temporal::{
    port::{history_reader::TemporalHistoryReader, mutation_barrier::TemporalMutationBarrier, mutator::TemporalMutator},
    remote_linked::RemoteTemporalLinkedSource,
    service::TemporalService,
};
use andromeda_tenancy::tenant_ref::TenantRef;
use std::{sync::Arc, time::Duration};

/// Request-facing state and the scheduled snapshot preparation operation composed with it.
pub(super) struct ConstructedServices {
    pub(super) state: AndromedaState,
    pub(super) snapshot_preparation: Arc<SnapshotPreparation>,
}

/// The subscription services one tenant answers requests with.
struct SubscriptionServices {
    /// Entity subscriptions (clause 5.8).
    entity: Arc<SubscriptionService<EntitySubscriptionVariant>>,
    /// Context Source Registration subscriptions (clause 5.11).
    csource: Arc<SubscriptionService<CsrSubscriptionVariant>>,
    /// The distributed side of subscriptions, absent when nothing advertises
    /// an origin peers could deliver notifications back to.
    forwarding: Option<Arc<SubscriptionForwardingService>>,
}

/// Construct all request-facing services for one tenant.
pub(super) fn construct_services(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    tenant: &TenantRef,
    signals: &TenantSignals,
    transports: &TenantTransports,
    temporal_reader: &Arc<dyn TemporalHistoryReader>,
    temporal_mutation_barrier: &Arc<dyn TemporalMutationBarrier>,
    subscription_router: &Arc<dyn ForwardedSubscriptionRouter>,
    writes: &Arc<dyn ContextCommitter>,
    batch_activity: &BatchActivityTracker,
) -> Result<ConstructedServices, TenantAssemblyError> {
    let pool_size = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections as usize,
        TenantRef::Named(_) => config.tenancy.tenant_max_connections as usize,
    };
    let entities = compose_entity_service(config, storage, signals, writes, batch_activity, pool_size);
    // Forwarding is constructed ahead of the services that join through it:
    // both the entity and the temporal join resolve an `objectType`-annotated
    // Linked Entity this broker does not hold through the same registrations.
    let forwarding = Arc::new(
        FederationService::new(
            Arc::clone(&storage.registration_store),
            storage.engine.clone(),           // clone: each tenant service owns its JSON-LD engine handle.
            config.info.source_alias.clone(), // clone: each forwarding service owns the validated alias.
            &config.federation,
        )
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?,
    );
    let temporal = compose_temporal_service(config, storage, temporal_reader, temporal_mutation_barrier, writes, &forwarding);
    let entity_query = compose_entity_query_service(storage, tenant, &forwarding);
    let context_store = Arc::new(JsonLdContextRepository::new(&storage.pool)) as Arc<dyn ContextRegistryStore>;
    let contexts = Arc::new(ContextRegistryService::new(context_store, storage.engine.clone()));
    let (snapshot_preparation, snapshots) = compose_snapshot_services(&config.snapshot, storage, writes, &entity_query, &temporal, &forwarding);
    let subscriptions = compose_subscription_services(
        config,
        storage,
        tenant,
        signals,
        transports,
        subscription_router,
        &forwarding,
        &entity_query,
        &contexts,
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
/// A durable notification write is configured only when delivery is enabled;
/// with live delivery the notification worker evaluates subscriptions itself.
fn compose_entity_service(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    signals: &TenantSignals,
    writes: &Arc<dyn ContextCommitter>,
    batch_activity: &BatchActivityTracker,
    pool_size: usize,
) -> Arc<EntityService> {
    let entity_change_sink = Arc::new(BroadcastEntityChangeSink::new(signals.entity.clone())) as Arc<dyn EntityChangeSink>; // clone: the runtime and policy each own a sender handle.
    let attribute_history = Arc::clone(&storage.temporal_repo) as Arc<dyn AttributeDatasetHistory>; // clone: temporal and entity policy share the concrete history adapter.
    Arc::new(EntityService::new(
        Arc::clone(&storage.entity_ports.reader),
        Arc::new(TokioBlockingExecutor),
        Arc::new(TokioRetryDelay),
        attribute_history,
        Arc::clone(writes),
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
        batch_activity.clone(), // clone: request services and worker supervision share activity state.
        config.notification_delivery.enabled.then(|| NotificationWriteConfig {
            subscription_cache: Arc::clone(&storage.subscription_cache),
        }),
    ))
}

/// Compose the temporal service the temporal API is answered from.
fn compose_temporal_service(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    reader: &Arc<dyn TemporalHistoryReader>,
    mutation_barrier: &Arc<dyn TemporalMutationBarrier>,
    writes: &Arc<dyn ContextCommitter>,
    forwarding: &Arc<FederationService>,
) -> Arc<TemporalService> {
    Arc::new(TemporalService::new(
        Arc::clone(&storage.entity_ports.reader),
        Arc::clone(reader),
        Arc::clone(mutation_barrier),
        Arc::clone(&storage.temporal_repo) as Arc<dyn TemporalMutator>,
        Arc::clone(writes),
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
fn compose_entity_query_service(storage: &TenantStorage, tenant: &TenantRef, forwarding: &Arc<FederationService>) -> Arc<EntityQueryService> {
    Arc::new(EntityQueryService::new(
        Arc::clone(&storage.entity_ports.reader),
        Arc::clone(&storage.entity_ports.query),
        storage.engine.clone(), // clone: the query service owns its engine handle.
        Arc::clone(&storage.spatial_index),
        tenant.clone(), // clone: the query service owns its tenant partition.
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
        storage.live_target.clone(), // clone: capture queries execute against the tenant's ordinary live branch.
        Arc::clone(&storage.snapshot_store),
        Arc::clone(writes),
        Arc::clone(entity_query),
        Arc::clone(temporal),
        Arc::clone(forwarding),
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

/// Compose both subscription services and the distributed side joining them.
fn compose_subscription_services(
    config: &TenantFactoryConfig,
    storage: &TenantStorage,
    tenant: &TenantRef,
    signals: &TenantSignals,
    transports: &TenantTransports,
    router: &Arc<dyn ForwardedSubscriptionRouter>,
    federation: &Arc<FederationService>,
    entity_query: &Arc<EntityQueryService>,
    contexts: &Arc<ContextRegistryService>,
) -> SubscriptionServices {
    let endpoint_validator: Arc<dyn SubscriptionEndpointValidator> = Arc::new(DeliverySubscriptionEndpointValidator);
    let csource = Arc::new(SubscriptionService::new(
        Arc::new(CsourceSubscriptionRepository::new(&storage.pool)),
        storage.engine.clone(),                              // clone: each subscription service owns its engine handle.
        CsrSubscriptionVariant::new(storage.engine.clone()), // clone: the variant expands against its own engine handle.
        Some(Arc::new(BroadcastSubscriptionChangeSink::new(signals.csource_subscription.clone()))), // clone: service and worker each own a sender handle.
        Arc::clone(&endpoint_validator),                     // clone: both subscription services share one endpoint validator.
    ));
    let store = Arc::new(SubscriptionRepository::new(&storage.pool)) as Arc<dyn SubscriptionStore>;
    let forwarding = compose_subscription_forwarding(&config.info, storage, tenant, transports, router, federation, entity_query, &csource, &store);
    let mut entity = SubscriptionService::new(
        Arc::clone(&store),
        storage.engine.clone(), // clone: each subscription service owns its engine handle.
        EntitySubscriptionVariant::new(storage.engine.clone(), Arc::clone(contexts)), // clone: the variant expands against its own engine handle.
        Some(Arc::new(CoherentSubscriptionChangeSink::new(
            signals.subscription.clone(), // clone: service and worker each own a sender handle.
            Arc::clone(&storage.subscription_cache),
            store,
        ))),
        endpoint_validator,
    );
    if let Some(forwarding_service) = &forwarding {
        entity = entity.with_forwarding_hook(Arc::clone(forwarding_service) as Arc<dyn SubscriptionForwardingHook>);
    }
    SubscriptionServices {
        entity: Arc::new(entity),
        csource,
        forwarding,
    }
}

/// Compose the distributed side of subscriptions for one tenant.
///
/// Clauses 5.8.1.4 and 5.8.6 need a notification endpoint peers can reach;
/// without an advertised origin the distributed side stays off, and the broker
/// says so once per tenant.
fn compose_subscription_forwarding(
    info: &InfoConfig,
    storage: &TenantStorage,
    tenant: &TenantRef,
    transports: &TenantTransports,
    router: &Arc<dyn ForwardedSubscriptionRouter>,
    federation: &Arc<FederationService>,
    entity_query: &Arc<EntityQueryService>,
    csource_subscriptions: &Arc<SubscriptionService<CsrSubscriptionVariant>>,
    subscriptions: &Arc<dyn SubscriptionStore>,
) -> Option<Arc<SubscriptionForwardingService>> {
    let Some(receiver) = info.advertised_url.as_ref().and_then(ReceiverEndpoint::from_advertised) else {
        tracing::info!(tenant = ?tenant, "subscription forwarding disabled: ANDROMEDA_INFO_ADVERTISED_URL is not set");
        return None;
    };
    Some(Arc::new(
        SubscriptionForwardingService::builder()
            .live_target(storage.live_target.clone()) // clone: forwarding is defined only over the tenant's live branch.
            .receiver(receiver)
            .tenant(tenant.clone()) // clone: the forwarding service owns its tenant
            .subscriptions(Arc::clone(subscriptions))
            .csource_subscriptions(Arc::clone(csource_subscriptions))
            .mappings(Arc::new(SubscriptionForwardingRepository::new(&storage.pool)) as Arc<dyn SubscriptionForwardingStore>)
            .router(Arc::clone(router))
            .federation(Arc::clone(federation))
            .entity_query(Arc::clone(entity_query))
            .transport(Arc::clone(&transports.dispatcher))
            .engine(storage.engine.clone()) // clone: forwarding owns its engine handle.
            .build(),
    ))
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
