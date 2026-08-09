//! Initialization of one tenant's database-backed storage graph.

use crate::{
    api::context_hit_flush::ContextHitFlushWorker,
    config::{performance::BulkWriteMode, runtime::RuntimeConfig},
    error::AndromedaError,
    service::snapshot::store::SnapshotStore,
    worker::spatial_warmup::warmup_location,
};
use andromeda_context_registry::hit_store::ContextHitStore;
use andromeda_entity::port::{
    discovery::EntityDiscoveryStore,
    expiry::EntityExpirySource,
    query::EntityQueryStore,
    reader::EntityReader,
    spatial_snapshot::EntitySpatialSnapshotSource,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_postgresql_context_store::{
    attribute_current::{materializer::AttributeCurrentMaterializer, queue::AttributeCurrentQueue},
    attribute_names::AttributeNameCache,
    csource_registration::registration_store::PostgresRegistrationStore,
    entity::repo::EntityRepository,
    jsonld_context::repo::JsonLdContextRepository,
    snapshot::repo::SnapshotRepository,
    subscription::repo::SubscriptionRepository,
    tenant_database::{configure_pool_options, migrate_tenant_db},
};
use andromeda_postgresql_query::{attribute_name_lookup::AttributeNameLookup, projection_barrier::ProjectionBarrier};
use andromeda_postgresql_temporal_store::{repo::TemporalRepository, timescale_features::TimescaleFeatures};
use andromeda_registration::store::RegistrationStore;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_subscription::cache::SubscriptionCache;
use andromeda_tenancy::tenant_ref::TenantRef;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use url::Url;

/// Validated repositories, indexes, and storage workers for one tenant.
pub(super) struct TenantStorage {
    pub(super) pool: PgPool,
    pub(super) engine: JsonLdEngine,
    pub(super) context_hit_flush: Option<ContextHitFlushWorker>,
    pub(super) materializer: AttributeCurrentMaterializer,
    pub(super) attribute_current_queue: AttributeCurrentQueue,
    pub(super) temporal_repo: Arc<TemporalRepository>,
    pub(super) entity_ports: EntityStoragePorts,
    pub(super) registration_store: Arc<dyn RegistrationStore>,
    pub(super) snapshot_store: Arc<dyn SnapshotStore>,
    pub(super) subscription_cache: Arc<SubscriptionCache>,
    pub(super) spatial_index: Arc<SpatialEntityIndex>,
}

/// Focused capabilities supplied by the tenant's entity repository.
pub(super) struct EntityStoragePorts {
    pub(super) reader: Arc<dyn EntityReader>,
    pub(super) query: Arc<dyn EntityQueryStore>,
    pub(super) discovery: Arc<dyn EntityDiscoveryStore>,
    pub(super) expiry: Arc<dyn EntityExpirySource>,
    pub(super) spatial_snapshots: Arc<dyn EntitySpatialSnapshotSource>,
}

/// Open, migrate, validate, and warm every storage component for a tenant.
pub(super) async fn initialize_storage(config: &RuntimeConfig, database_url: &Url, tenant: &TenantRef) -> Result<TenantStorage, AndromedaError> {
    let max_connections = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections,
        TenantRef::Named(_) => config.tenancy.tenant_max_connections,
    };
    let pool = configure_pool_options(max_connections, config.database.synchronous_commit.as_postgres_setting())
        .connect(database_url.as_str())
        .await?;
    migrate_tenant_db(&pool).await?;
    let timescale_features = TimescaleFeatures::detect(&pool)
        .await
        .map_err(|error| AndromedaError::internal(format!("Failed to probe TimescaleDB features: {error}")))?;
    tracing::info!(
        has_continuous_aggregates = timescale_features.has_continuous_aggregates,
        "detected TimescaleDB capabilities"
    );

    let context_repo = Arc::new(JsonLdContextRepository::new(&pool));
    let engine = config.jsonld.build_engine(Arc::clone(&context_repo))?;
    let context_hit_flush = engine.hit_counter().map(|hit_counter| {
        ContextHitFlushWorker::spawn(
            hit_counter,
            Arc::clone(&context_repo) as Arc<dyn ContextHitStore>,
            Duration::from_secs(config.performance.context_hit_flush_secs.max(1)),
        )
    });

    let attribute_names = Arc::new(AttributeNameCache::new(&pool));
    if config.performance.preload_attribute_names
        && let Err(error) = attribute_names.preload().await
    {
        tracing::warn!(%error, event = "attribute_names_preload_failed");
    }
    let (materializer, attribute_current_queue, attribute_current_barrier) = AttributeCurrentMaterializer::new(
        Arc::new(pool.clone()), // clone: the materializer and storage holder share the pool.
        Duration::from_secs(config.spatial.lag_log_interval_secs.max(1)),
    );
    if let Err(error) = materializer.reconcile_on_startup().await {
        tracing::warn!(%error, event = "attribute_current_reconcile_failed");
    }
    let bulk_write_mode = match config.performance.bulk_write_mode {
        BulkWriteMode::Unnest => andromeda_postgresql_temporal_store::bulk_write_mode::BulkWriteMode::Unnest,
        BulkWriteMode::CopyStaging => andromeda_postgresql_temporal_store::bulk_write_mode::BulkWriteMode::CopyStaging,
    };
    let temporal_repo = Arc::new(
        TemporalRepository::new(&pool, bulk_write_mode, Arc::clone(&attribute_names) as Arc<dyn AttributeNameLookup>)
            .with_features(timescale_features)
            .with_barrier(Arc::new(attribute_current_barrier.clone()) as Arc<dyn ProjectionBarrier>), // clone: both read stores wait on one watermark.
    );
    let entity_repository = Arc::new(EntityRepository::new(
        &pool,
        Arc::clone(&attribute_names) as Arc<dyn AttributeNameLookup>,
        Arc::new(attribute_current_barrier) as Arc<dyn ProjectionBarrier>,
    ));
    let entity_ports = EntityStoragePorts {
        reader: Arc::clone(&entity_repository) as Arc<dyn EntityReader>, // clone: every focused port shares one repository and pool.
        query: Arc::clone(&entity_repository) as Arc<dyn EntityQueryStore>, // clone: every focused port shares one repository and pool.
        discovery: Arc::clone(&entity_repository) as Arc<dyn EntityDiscoveryStore>, // clone: every focused port shares one repository and pool.
        expiry: Arc::clone(&entity_repository) as Arc<dyn EntityExpirySource>, // clone: every focused port shares one repository and pool.
        spatial_snapshots: entity_repository as Arc<dyn EntitySpatialSnapshotSource>,
    };
    let registration_store = Arc::new(PostgresRegistrationStore::new(&pool)) as Arc<dyn RegistrationStore>;
    let snapshot_store = Arc::new(SnapshotRepository::new(&pool)) as Arc<dyn SnapshotStore>;
    let subscription_cache = Arc::new(SubscriptionCache::new(tenant.clone()));
    if let Err(error) = subscription_cache.reload(&SubscriptionRepository::new(&pool)).await {
        tracing::error!(%error, "failed to load subscription cache");
    }
    let spatial_index = Arc::new(SpatialEntityIndex::new());
    if let Err(error) = warmup_location(&spatial_index, entity_ports.spatial_snapshots.as_ref(), tenant).await {
        tracing::warn!(%error, "spatial warmup failed");
    }
    Ok(TenantStorage {
        pool,
        engine,
        context_hit_flush,
        materializer,
        attribute_current_queue,
        temporal_repo,
        entity_ports,
        registration_store,
        snapshot_store,
        subscription_cache,
        spatial_index,
    })
}
