//! Construction of one tenant's request-facing service graph.

use crate::{
    api::{
        state::AndromedaState,
        tenant_factory::{transport::TenantTransports, workers::TenantSignals},
    },
    batch_activity::BatchActivityTracker,
    config::runtime::RuntimeConfig,
    error::AndromedaError,
    service::{
        csource_forwarding::CsourceForwardingService,
        csource_registration::{CsourceRegistrationService, store::RegistrationStore},
        csource_subscription::CsrSubscriptionVariant,
        entity::{
            EntityService,
            OutboxWriteConfig,
            batch::execution::BatchExecutionConfig,
            discovery::EntityDiscoveryService,
            query::EntityQueryService,
            store::EntityStore,
        },
        snapshot::{SnapshotService, store::SnapshotStore},
        subscription::EntitySubscriptionVariant,
        subscription_base::SubscriptionServiceCore,
        subscription_cache::SubscriptionCache,
        temporal::TemporalService,
    },
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_registry::{service::ContextRegistryService, store::ContextRegistryStore};
use andromeda_entity_map::{memory_store::InMemoryEntityMapStore, service::EntityMapService};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_postgresql_context_store::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    jsonld_context::repo::JsonLdContextRepository,
    subscription::repo::SubscriptionRepository,
};
use andromeda_postgresql_temporal_store::{
    recorder::TemporalRecorder,
    repo::TemporalRepository,
    store::{TemporalHistoryReader, TemporalMutator},
};
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_tenancy::tenant_ref::TenantRef;
use sqlx::PgPool;
use std::sync::Arc;
use typed_builder::TypedBuilder;

/// Values used once to compose a tenant's request services.
#[derive(TypedBuilder)]
pub(super) struct ServiceConstructionInputs<'a> {
    pub(super) config: &'a RuntimeConfig,
    pub(super) pool: &'a PgPool,
    pub(super) tenant: &'a TenantRef,
    pub(super) signals: &'a TenantSignals,
    pub(super) transports: &'a TenantTransports,
    pub(super) engine: &'a JsonLdEngine,
    pub(super) temporal_repo: &'a Arc<TemporalRepository>,
    pub(super) temporal_recorder: &'a Arc<TemporalRecorder>,
    pub(super) entity_store: &'a Arc<dyn EntityStore>,
    pub(super) registration_store: &'a Arc<dyn RegistrationStore>,
    pub(super) snapshot_store: &'a Arc<dyn SnapshotStore>,
    pub(super) subscription_cache: &'a Arc<SubscriptionCache>,
    pub(super) spatial_index: &'a Arc<SpatialEntityIndex>,
    pub(super) writes: &'a Arc<dyn ContextCommitter>,
    pub(super) batch_activity: &'a BatchActivityTracker,
}

/// Construct all request-facing services for one tenant.
pub(super) fn construct_services(inputs: ServiceConstructionInputs<'_>) -> Result<AndromedaState, AndromedaError> {
    let ServiceConstructionInputs {
        config,
        pool,
        tenant,
        signals,
        transports,
        engine,
        temporal_repo,
        temporal_recorder,
        entity_store,
        registration_store,
        snapshot_store,
        subscription_cache,
        spatial_index,
        writes,
        batch_activity,
    } = inputs;
    let pool_size = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections as usize,
        TenantRef::Named(_) => config.tenancy.tenant_max_connections as usize,
    };
    let entities = Arc::new(EntityService::new(
        Arc::clone(entity_store),
        Arc::clone(temporal_recorder),
        Arc::clone(writes),
        engine.clone(),
        Some(signals.entity.clone()),
        BatchExecutionConfig {
            concurrency: config.performance.batch_concurrency.unwrap_or_else(|| usize::min(pool_size, 25)).max(1),
            chunk_concurrency: config.performance.batch_chunk_concurrency.unwrap_or(4).max(1),
            chunk_write_concurrency: config.performance.batch_chunk_write_concurrency.unwrap_or(4).max(1),
            chunk_size: config.performance.batch_chunk_size.max(1),
            large_threshold: config.performance.batch_large_threshold.max(1),
            prepare_blocking_threshold: config.performance.batch_prepare_blocking_threshold.max(1),
            async_side_effect_threshold: config.performance.batch_async_side_effect_threshold,
        },
        batch_activity.clone(), // clone: request services and worker supervision share activity state.
        config.outbox.enabled.then(|| OutboxWriteConfig {
            subscription_cache: Arc::clone(subscription_cache),
        }),
    ));
    let temporal = Arc::new(TemporalService::new(
        Arc::clone(entity_store),
        Arc::clone(temporal_repo) as Arc<dyn TemporalHistoryReader>,
        Arc::clone(temporal_repo) as Arc<dyn TemporalMutator>,
        Arc::clone(temporal_recorder),
        Arc::clone(writes),
        engine.clone(),
        config.temporal.default_last_n,
    ));
    let entity_query = Arc::new(EntityQueryService::new(
        Arc::clone(entity_store),
        engine.clone(),
        Arc::clone(spatial_index),
        tenant.clone(), // clone: the query service owns its tenant partition.
    ));
    let forwarding = Arc::new(CsourceForwardingService::new(
        Arc::clone(registration_store),
        engine.clone(),
        config.info.source_alias.clone(), // clone: each forwarding service owns the validated alias.
        config.performance.forward_csr_concurrency,
    )?);
    let context_store = Arc::new(JsonLdContextRepository::new(pool)) as Arc<dyn ContextRegistryStore>;
    let contexts = Arc::new(ContextRegistryService::new(context_store, engine.clone()));
    let snapshots = Arc::new(SnapshotService::new(
        Arc::clone(snapshot_store),
        Some(Arc::clone(&transports.dispatcher)),
        Arc::clone(&entity_query),
        Arc::clone(&temporal),
        Arc::clone(&forwarding),
        config.performance.snapshot_concurrency,
    ));
    Ok(AndromedaState {
        entities,
        entity_query,
        entity_discovery: Arc::new(EntityDiscoveryService::new(Arc::clone(entity_store), engine.clone())),
        temporal,
        subscriptions: Arc::new(SubscriptionServiceCore::<SubscriptionRepository, EntitySubscriptionVariant>::new(
            pool,
            engine.clone(),
            Arc::clone(&contexts),
            Some(signals.subscription.clone()),
        )),
        contexts,
        csr: Arc::new(CsourceRegistrationService::new(
            Arc::clone(registration_store),
            engine.clone(),
            Some(signals.registration.clone()),
        )),
        csr_subscriptions: Arc::new(SubscriptionServiceCore::<CsourceSubscriptionRepository, CsrSubscriptionVariant>::new(
            pool,
            engine.clone(),
            Some(signals.csource_subscription.clone()),
        )),
        forwarding,
        entity_maps: Arc::new(EntityMapService::new(Arc::new(InMemoryEntityMapStore::new()))),
        snapshots,
        sse_registry: Arc::clone(&transports.sse_registry),
        sse_channel_buffer_size: config.notifications.sse.channel_buffer_size,
        sse_keep_alive_interval_secs: config.notifications.sse.keep_alive_interval_secs,
        ws_registry: Arc::clone(&transports.ws_registry),
        ws_channel_buffer_size: config.notifications.ws.channel_buffer_size,
        ws_ping_interval_secs: config.notifications.ws.ping_interval_secs,
    })
}
