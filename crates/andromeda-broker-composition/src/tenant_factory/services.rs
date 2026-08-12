//! Construction of one tenant's request-facing service graph.

use crate::{
    blocking_executor::TokioBlockingExecutor,
    event::{
        entity_change_sink::BroadcastEntityChangeSink,
        registration_change_sink::BroadcastRegistrationChangeSink,
        snapshot_notification_sink::DeliverySnapshotNotificationSink,
        subscription_change_sink::BroadcastSubscriptionChangeSink,
        subscription_endpoint_validator::DeliverySubscriptionEndpointValidator,
    },
    linked_entity_forwarding::ForwardedLinkedEntitySource,
    retry_delay::TokioRetryDelay,
    temporal_linked_forwarding::ForwardedTemporalLinkedSource,
    tenant_factory::{error::TenantAssemblyError, tasks::TenantSignals, transport::TenantTransports},
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
    config::EntityConfig,
    discovery::EntityDiscoveryService,
    port::{attribute_history::AttributeDatasetHistory, discovery::EntityDiscoveryStore, query::EntityQueryStore, reader::EntityReader},
    query::{EntityQueryService, remote_linked::RemoteLinkedEntitySource},
};
use andromeda_entity_map::{memory_store::InMemoryEntityMapStore, service::EntityMapService};
use andromeda_federation::{FederationService, config::FederationConfig};
use andromeda_http_source_identity::config::InfoConfig;
use andromeda_http_subscription::config::NotificationConfig;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_notification::delivery_config::DeliveryConfig;
use andromeda_postgresql_context_store::{
    config::DatabaseConfig,
    csource_subscription::repo::CsourceSubscriptionRepository,
    jsonld_context::repo::JsonLdContextRepository,
    subscription::repo::SubscriptionRepository,
};
use andromeda_postgresql_temporal_store::repo::TemporalRepository;
use andromeda_registration::{RegistrationService, store::RegistrationStore};
use andromeda_snapshot::{
    config::SnapshotConfig,
    executor::SnapshotPreparation,
    notification::SnapshotNotificationSink,
    service::SnapshotService,
    store::SnapshotStore,
};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_subscription::{
    cache::SubscriptionCache,
    csource::CsrSubscriptionVariant,
    entity::EntitySubscriptionVariant,
    lifecycle::{SubscriptionEndpointValidator, SubscriptionService},
};
use andromeda_temporal::{
    config::TemporalConfig,
    port::{history_reader::TemporalHistoryReader, mutation_barrier::TemporalMutationBarrier, mutator::TemporalMutator},
    remote_linked::RemoteTemporalLinkedSource,
    service::TemporalService,
};
use andromeda_tenancy::{config::TenancyConfig, tenant_ref::TenantRef};
use sqlx::PgPool;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// Values used once to compose a tenant's request services.
#[derive(TypedBuilder)]
pub(super) struct ServiceConstructionInputs<'a> {
    pub(super) database_config: &'a DatabaseConfig,
    pub(super) entity_config: &'a EntityConfig,
    pub(super) federation_config: &'a FederationConfig,
    pub(super) info_config: &'a InfoConfig,
    pub(super) notification_config: &'a NotificationConfig,
    pub(super) delivery_config: &'a DeliveryConfig,
    pub(super) snapshot_config: &'a SnapshotConfig,
    pub(super) temporal_config: &'a TemporalConfig,
    pub(super) tenancy_config: &'a TenancyConfig,
    pub(super) pool: &'a PgPool,
    pub(super) tenant: &'a TenantRef,
    pub(super) signals: &'a TenantSignals,
    pub(super) transports: &'a TenantTransports,
    pub(super) engine: &'a JsonLdEngine,
    pub(super) temporal_repo: &'a Arc<TemporalRepository>,
    pub(super) temporal_reader: &'a Arc<dyn TemporalHistoryReader>,
    pub(super) temporal_mutation_barrier: &'a Arc<dyn TemporalMutationBarrier>,
    pub(super) entity_reader: &'a Arc<dyn EntityReader>,
    pub(super) entity_query_store: &'a Arc<dyn EntityQueryStore>,
    pub(super) entity_discovery_store: &'a Arc<dyn EntityDiscoveryStore>,
    pub(super) registration_store: &'a Arc<dyn RegistrationStore>,
    pub(super) snapshot_store: &'a Arc<dyn SnapshotStore>,
    pub(super) subscription_cache: &'a Arc<SubscriptionCache>,
    pub(super) spatial_index: &'a Arc<SpatialEntityIndex>,
    pub(super) writes: &'a Arc<dyn ContextCommitter>,
    pub(super) batch_activity: &'a BatchActivityTracker,
}

/// Request-facing state and the scheduled snapshot preparation operation composed with it.
pub(super) struct ConstructedServices {
    pub(super) state: AndromedaState,
    pub(super) snapshot_preparation: Arc<SnapshotPreparation>,
}

/// Construct all request-facing services for one tenant.
pub(super) fn construct_services(inputs: ServiceConstructionInputs<'_>) -> Result<ConstructedServices, TenantAssemblyError> {
    let ServiceConstructionInputs {
        database_config,
        entity_config,
        federation_config,
        info_config,
        notification_config,
        delivery_config,
        snapshot_config,
        temporal_config,
        tenancy_config,
        pool,
        tenant,
        signals,
        transports,
        engine,
        temporal_repo,
        temporal_reader,
        temporal_mutation_barrier,
        entity_reader,
        entity_query_store,
        entity_discovery_store,
        registration_store,
        snapshot_store,
        subscription_cache,
        spatial_index,
        writes,
        batch_activity,
    } = inputs;
    let pool_size = match tenant {
        TenantRef::UnnamedDefault => database_config.max_connections as usize,
        TenantRef::Named(_) => tenancy_config.tenant_max_connections as usize,
    };
    let entity_change_sink = Arc::new(BroadcastEntityChangeSink::new(signals.entity.clone())) as Arc<dyn EntityChangeSink>; // clone: the runtime and policy each own a sender handle.
    let attribute_history = Arc::clone(temporal_repo) as Arc<dyn AttributeDatasetHistory>; // clone: temporal and entity policy share the concrete history adapter.
    let entities = Arc::new(EntityService::new(
        Arc::clone(entity_reader),
        Arc::new(TokioBlockingExecutor),
        Arc::new(TokioRetryDelay),
        attribute_history,
        Arc::clone(writes),
        engine.clone(),
        Some(entity_change_sink),
        BatchExecutionConfig {
            concurrency: entity_config.batch_concurrency.unwrap_or_else(|| usize::min(pool_size, 25)),
            chunk_concurrency: entity_config.batch_chunk_concurrency.unwrap_or(4),
            chunk_write_concurrency: entity_config.batch_chunk_write_concurrency.unwrap_or(4),
            chunk_size: entity_config.batch_chunk_size,
            large_threshold: entity_config.batch_large_threshold,
            prepare_blocking_threshold: entity_config.batch_prepare_blocking_threshold,
            async_side_effect_threshold: entity_config.batch_async_side_effect_threshold,
        },
        batch_activity.clone(), // clone: request services and worker supervision share activity state.
        delivery_config.enabled.then(|| NotificationWriteConfig {
            subscription_cache: Arc::clone(subscription_cache),
        }),
    ));
    // Forwarding is constructed ahead of the services that join through it:
    // both the entity and the temporal join resolve an `objectType`-annotated
    // Linked Entity this broker does not hold through the same registrations.
    let forwarding = Arc::new(
        FederationService::new(
            Arc::clone(registration_store),
            engine.clone(),
            info_config.source_alias.clone(), // clone: each forwarding service owns the validated alias.
            federation_config.forward_concurrency,
        )
        .map_err(|error| TenantAssemblyError::invariant(error.to_string()))?,
    );
    let temporal = Arc::new(TemporalService::new(
        Arc::clone(entity_reader),
        Arc::clone(temporal_reader),
        Arc::clone(temporal_mutation_barrier),
        Arc::clone(temporal_repo) as Arc<dyn TemporalMutator>,
        Arc::clone(writes),
        engine.clone(),
        temporal_config.default_last_n,
        // Clause 4.5.23.1 applied to the temporal API: an annotated Linked
        // Entity whose history this broker does not record is fetched from the
        // context sources registered for it.
        Some(Arc::new(ForwardedTemporalLinkedSource::new(Arc::clone(&forwarding))) as Arc<dyn RemoteTemporalLinkedSource>), // clone: request handling and linked resolution share one forwarding service.
    ));
    let entity_query = Arc::new(EntityQueryService::new(
        Arc::clone(entity_reader),
        Arc::clone(entity_query_store),
        engine.clone(),
        Arc::clone(spatial_index),
        tenant.clone(), // clone: the query service owns its tenant partition.
        // Clause 4.5.23.1: an `objectType`-annotated Linked Entity this broker
        // does not store is fetched from the context sources registered for it.
        Some(Arc::new(ForwardedLinkedEntitySource::new(Arc::clone(&forwarding))) as Arc<dyn RemoteLinkedEntitySource>), // clone: request handling and linked resolution share one forwarding service.
    ));
    let context_store = Arc::new(JsonLdContextRepository::new(pool)) as Arc<dyn ContextRegistryStore>;
    let contexts = Arc::new(ContextRegistryService::new(context_store, engine.clone()));
    let snapshot_notifications: Arc<dyn SnapshotNotificationSink> = Arc::new(DeliverySnapshotNotificationSink::new(Arc::clone(&transports.dispatcher)));
    let snapshots = Arc::new(SnapshotService::new(Arc::clone(snapshot_store), Some(Arc::clone(&snapshot_notifications))));
    let snapshot_preparation = Arc::new(SnapshotPreparation::new(
        Arc::clone(snapshot_store),
        Arc::clone(&entity_query),
        Arc::clone(&temporal),
        Arc::clone(&forwarding),
        Some(snapshot_notifications),
        snapshot_config.preparation_concurrency,
    ));
    let subscription_endpoint_validator: Arc<dyn SubscriptionEndpointValidator> = Arc::new(DeliverySubscriptionEndpointValidator);
    let state = AndromedaState {
        entities,
        entity_query,
        entity_discovery: Arc::new(EntityDiscoveryService::new(Arc::clone(entity_discovery_store), engine.clone())),
        temporal,
        subscriptions: Arc::new(SubscriptionService::new(
            Arc::new(SubscriptionRepository::new(pool)),
            engine.clone(),
            EntitySubscriptionVariant::new(engine.clone(), Arc::clone(&contexts)),
            Some(Arc::new(BroadcastSubscriptionChangeSink::new(signals.subscription.clone()))),
            Arc::clone(&subscription_endpoint_validator),
        )),
        contexts,
        csr: Arc::new(RegistrationService::new(
            Arc::clone(registration_store),
            engine.clone(),
            Some(Arc::new(BroadcastRegistrationChangeSink::new(signals.registration.clone()))), // clone: each tenant service and worker own a sender handle.
        )),
        csr_subscriptions: Arc::new(SubscriptionService::new(
            Arc::new(CsourceSubscriptionRepository::new(pool)),
            engine.clone(),
            CsrSubscriptionVariant::new(engine.clone()),
            Some(Arc::new(BroadcastSubscriptionChangeSink::new(signals.csource_subscription.clone()))),
            subscription_endpoint_validator,
        )),
        forwarding,
        entity_maps: Arc::new(EntityMapService::new(Arc::new(InMemoryEntityMapStore::new()))),
        snapshots,
        sse_registry: Arc::clone(&transports.sse_registry),
        sse_channel_buffer_size: notification_config.sse.channel_buffer_size,
        sse_keep_alive_interval_secs: notification_config.sse.keep_alive_interval_secs,
        ws_registry: Arc::clone(&transports.ws_registry),
        ws_channel_buffer_size: notification_config.ws.channel_buffer_size,
        ws_ping_interval_secs: notification_config.ws.ping_interval_secs,
    };
    Ok(ConstructedServices { state, snapshot_preparation })
}
