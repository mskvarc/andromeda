//! Initialization of one tenant's database-backed storage graph.

use crate::{context_hit_flush::ContextHitFlushWorker, tenant_factory::error::TenantAssemblyError};
use andromeda_context_registry::hit_store::ContextHitStore;
use andromeda_entity::{
    port::{
        discovery::EntityDiscoveryStore,
        expiry::EntityExpirySource,
        query::EntityQueryStore,
        reader::EntityReader,
        spatial_snapshot::EntitySpatialSnapshotSource,
    },
    spatial_reconciliation::reconcile_location,
};
use andromeda_jsonld::{config::JsonLdConfig, engine::processor::JsonLdEngine};
use andromeda_postgresql_atomic_write::config::{AtomicWriteConfig, BulkWriteMode};
use andromeda_postgresql_attribute_dictionary::{attribute_name_cache::AttributeNameCache, attribute_name_lookup::AttributeNameLookup};
use andromeda_postgresql_context_store::{
    config::DatabaseConfig,
    csource_registration::registration_store::PostgresRegistrationStore,
    entity::repo::EntityRepository,
    jsonld_context::repo::JsonLdContextRepository,
    snapshot::repo::SnapshotRepository,
    subscription::repo::SubscriptionRepository,
    tenant_database::{configure_pool_options, migrate_tenant_db},
};
use andromeda_postgresql_temporal_store::repo::TemporalRepository;
use andromeda_registration::store::RegistrationStore;
use andromeda_snapshot::store::SnapshotStore;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_subscription::cache::SubscriptionCache;
use andromeda_tenancy::{config::TenancyConfig, tenant_ref::TenantRef};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use url::Url;

/// Validated repositories, indexes, and storage workers for one tenant.
pub(super) struct TenantStorage {
    pub(super) pool: PgPool,
    pub(super) engine: JsonLdEngine,
    pub(super) context_hit_flush: Option<ContextHitFlushWorker>,
    pub(super) attribute_names: Arc<dyn AttributeNameLookup>,
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
pub(super) async fn initialize_storage(
    database_config: &DatabaseConfig,
    tenancy_config: &TenancyConfig,
    jsonld_config: &JsonLdConfig,
    atomic_write_config: &AtomicWriteConfig,
    database_url: &Url,
    tenant: &TenantRef,
) -> Result<TenantStorage, TenantAssemblyError> {
    let max_connections = match tenant {
        TenantRef::UnnamedDefault => database_config.max_connections,
        TenantRef::Named(_) => tenancy_config.tenant_max_connections,
    };
    let pool = configure_pool_options(max_connections, database_config.synchronous_commit.as_postgres_setting())
        .connect(database_url.as_str())
        .await?;
    migrate_tenant_db(&pool).await?;
    let context_repo = Arc::new(JsonLdContextRepository::new(&pool));
    let engine = jsonld_config.build_engine(Arc::clone(&context_repo))?;
    let context_hit_flush = engine.hit_counter().map(|hit_counter| {
        ContextHitFlushWorker::spawn(
            hit_counter,
            Arc::clone(&context_repo) as Arc<dyn ContextHitStore>,
            Duration::from_secs(database_config.context_hit_flush_secs),
        )
    });

    let attribute_names = Arc::new(AttributeNameCache::new(&pool));
    if database_config.preload_attribute_names
        && let Err(error) = attribute_names.preload().await
    {
        tracing::warn!(%error, event = "attribute_names_preload_failed");
    }
    let bulk_write_mode = match atomic_write_config.bulk_mode {
        BulkWriteMode::Unnest => andromeda_postgresql_temporal_store::bulk_write_mode::BulkWriteMode::Unnest,
        BulkWriteMode::CopyStaging => andromeda_postgresql_temporal_store::bulk_write_mode::BulkWriteMode::CopyStaging,
    };
    // One dictionary cache per tenant, shared by every path that resolves an
    // attribute IRI: the temporal catalogue, the entity repository's read path
    // and the projection the commit writes.
    let attribute_names = Arc::clone(&attribute_names) as Arc<dyn AttributeNameLookup>; // clone: the cache outlives each port that holds it.
    let temporal_repo = Arc::new(TemporalRepository::new(&pool, bulk_write_mode, Arc::clone(&attribute_names)));
    let entity_repository = Arc::new(EntityRepository::new(&pool, Arc::clone(&attribute_names)));
    let entity_ports = EntityStoragePorts {
        reader: Arc::clone(&entity_repository) as Arc<dyn EntityReader>, // clone: every focused port shares one repository and pool.
        query: Arc::clone(&entity_repository) as Arc<dyn EntityQueryStore>, // clone: every focused port shares one repository and pool.
        discovery: Arc::clone(&entity_repository) as Arc<dyn EntityDiscoveryStore>, // clone: every focused port shares one repository and pool.
        expiry: Arc::clone(&entity_repository) as Arc<dyn EntityExpirySource>, // clone: every focused port shares one repository and pool.
        spatial_snapshots: entity_repository as Arc<dyn EntitySpatialSnapshotSource>,
    };
    // clone: the routing projection interns registered attribute names through the tenant's one dictionary.
    let registration_store = Arc::new(PostgresRegistrationStore::new(&pool, Arc::clone(&attribute_names))) as Arc<dyn RegistrationStore>;
    let snapshot_store = Arc::new(SnapshotRepository::new(&pool)) as Arc<dyn SnapshotStore>;
    let subscription_cache = Arc::new(SubscriptionCache::new(tenant.clone()));
    if let Err(error) = subscription_cache.reload(&SubscriptionRepository::new(&pool)).await {
        tracing::error!(%error, "failed to load subscription cache");
    }
    let spatial_index = Arc::new(SpatialEntityIndex::new());
    if let Err(error) = reconcile_location(&spatial_index, entity_ports.spatial_snapshots.as_ref(), tenant).await {
        tracing::warn!(%error, "spatial warmup failed");
    }
    Ok(TenantStorage {
        pool,
        engine,
        context_hit_flush,
        attribute_names,
        temporal_repo,
        entity_ports,
        registration_store,
        snapshot_store,
        subscription_cache,
        spatial_index,
    })
}
