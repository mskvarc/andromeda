//! Initialization of one tenant's database-backed storage graph.

use crate::{
    context_hit_flush::ContextHitFlushWorker,
    tenant_factory::{TenantFactoryConfig, error::TenantAssemblyError},
};
use andromeda_context_registry::{cached_context_invalidation::CachedContextInvalidation, hit_store::ContextHitStore};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    port::{
        discovery::EntityDiscoveryStore,
        expiry::EntityExpirySource,
        query::EntityQueryStore,
        reader::EntityReader,
        registry_types::RegistryTypeSource,
        spatial_snapshot::EntitySpatialSnapshotSource,
    },
    spatial_reconciliation::reconcile_location,
};
use andromeda_entity_map::store::EntityMapStore;
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_atomic_write::config::BulkWriteMode;
use andromeda_postgresql_context_store::{
    context_space::resolve_live_target,
    csource_registration::registration_store::PostgresRegistrationStore,
    entity::repo::EntityRepository,
    entity_map::repo::EntityMapRepository,
    jsonld_context::repo::JsonLdContextRepository,
    snapshot::{
        branch_upgrade::{SnapshotBranchState, snapshot_branch_state},
        repo::SnapshotRepository,
    },
    subscription::repo::SubscriptionRepository,
    tenant_migration::{TenantSchemaState, migrate_single_tenant_db, tenant_schema_state, upgrade_single_tenant_snapshot_branches},
};
use andromeda_postgresql_name_dictionary::{
    name_cache::{NameCache, read_established_epoch},
    name_lookup::NameLookup,
};
use andromeda_postgresql_pool::configure_pool_options;
use andromeda_postgresql_temporal_store::{
    bulk_write_mode::BulkWriteMode as TemporalBulkWriteMode,
    columnstore_capability::ColumnstoreSupport,
    repo::TemporalRepository,
};
use andromeda_registration::store::RegistrationStore;
use andromeda_snapshot::store::SnapshotStore;
use andromeda_spatial_index::entity::index::SpatialEntityIndex;
use andromeda_subscription::cache::SubscriptionCache;
use andromeda_tenancy::tenant_ref::TenantRef;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use url::Url;

/// Validated repositories, indexes, and storage workers for one tenant.
pub(super) struct TenantStorage {
    /// The tenant this storage graph was opened, migrated and warmed for.
    pub(super) tenant: TenantRef,
    pub(super) pool: PgPool,
    pub(super) live_target: ContextTarget,
    pub(super) engine: JsonLdEngine,
    pub(super) context_workers: TenantContextWorkers,
    /// The clause 5.13.1 invalidation port, driven by a supervised scheduled task.
    pub(super) cached_context_invalidation: Arc<dyn CachedContextInvalidation>,
    pub(super) attribute_names: Arc<dyn NameLookup<AttributeName>>,
    pub(super) entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
    pub(super) temporal_repo: Arc<TemporalRepository>,
    pub(super) entity_ports: EntityStoragePorts,
    pub(super) registration_store: Arc<dyn RegistrationStore>,
    pub(super) registry_type_source: Arc<dyn RegistryTypeSource>,
    pub(super) entity_map_store: Arc<dyn EntityMapStore>,
    pub(super) snapshot_store: Arc<dyn SnapshotStore>,
    pub(super) subscription_cache: Arc<SubscriptionCache>,
    pub(super) spatial_index: Arc<SpatialEntityIndex>,
}

/// The background workers one tenant runs over its stored `@contexts`.
///
/// Both are driven by clause 5.13 and by the same `jsonld_contexts` table, and
/// both are stopped together when the tenant is torn down, so they travel as
/// one rather than as two more parameters threaded through the runtime.
pub(super) struct TenantContextWorkers {
    /// Clause 5.13.3.5 hit accounting, when the engine attached a counter.
    hit_flush: Option<ContextHitFlushWorker>,
}

impl TenantContextWorkers {
    /// Stop the hit-counter flush, waiting for its final drain.
    ///
    /// The clause 5.13.1 invalidation pass is a supervised scheduled task rather
    /// than a worker held here, so this stops only the hit flush, which still
    /// owes the store its accumulated deltas.
    pub(super) async fn shutdown(self) {
        if let Some(worker) = self.hit_flush {
            worker.shutdown().await;
        }
    }
}

/// Focused capabilities supplied by the tenant's entity repository.
pub(super) struct EntityStoragePorts {
    pub(super) reader: Arc<dyn EntityReader>,
    pub(super) query: Arc<dyn EntityQueryStore>,
    pub(super) discovery: Arc<dyn EntityDiscoveryStore>,
    pub(super) expiry: Arc<dyn EntityExpirySource>,
    pub(super) spatial_snapshots: Arc<dyn EntitySpatialSnapshotSource>,
}

/// Migrate a tenant whose schema is behind this binary, or upgrade its
/// Snapshot branches when only they are.
///
/// A current schema can still hold Snapshot branches behind this binary: the
/// tenant was migrated by a replica that has not upgraded them, or an earlier
/// upgrade quarantined one. One `EXISTS` probe over the serving pool decides
/// whether the direct connection is opened at all.
async fn bring_schema_current(pool: &PgPool, migration_url: &Url, acquire_timeout: Duration) -> Result<(), TenantAssemblyError> {
    match tenant_schema_state(pool).await? {
        TenantSchemaState::Current => match snapshot_branch_state(pool).await? {
            SnapshotBranchState::Current => {}
            SnapshotBranchState::Behind => upgrade_single_tenant_snapshot_branches(migration_url, acquire_timeout).await?,
        },
        TenantSchemaState::NeedsMigration => migrate_single_tenant_db(migration_url, acquire_timeout).await?,
    }
    Ok(())
}

/// Open, migrate, validate, and warm every storage component for a tenant.
///
/// The database is migrated over `migration_url`, which reaches PostgreSQL
/// directly, because the migrator holds a session advisory lock a
/// transaction-pooling proxy would break. Serving then runs over `serving_url`,
/// which is the pooler when one is configured. The two URLs name the same
/// database and are identical when no pooler is configured.
pub(super) async fn initialize_storage(
    config: &TenantFactoryConfig,
    serving_url: &Url,
    migration_url: &Url,
    tenant: &TenantRef,
) -> Result<TenantStorage, TenantAssemblyError> {
    let max_connections = match tenant {
        TenantRef::UnnamedDefault => config.database.max_connections,
        // Through a pooler a named tenant's serving pool holds only a couple of
        // client connections -- the pooler does the real backend pooling -- so a
        // working set larger than the runtime cache cannot churn the host's
        // sockets into ephemeral-port exhaustion. Direct, it is the real backend
        // pool and takes the tenancy-configured size.
        TenantRef::Named(_) => config.database.named_serving_max_connections(config.tenancy.tenant_max_connections),
    };
    // Open the serving pool first -- the pooler when one is configured -- and
    // migrate only when the schema is not already current. A warm-schema cold
    // load then never opens the direct migrator connection or takes its session
    // advisory lock, which a transaction-pooling proxy would break; the pooler
    // is left to serve every warm reopen.
    let pool = configure_pool_options(max_connections, config.database.acquire_timeout())
        .connect(serving_url.as_str())
        .await?;
    bring_schema_current(&pool, migration_url, config.database.acquire_timeout()).await?;
    let live_target = resolve_live_target();
    let context_repo = Arc::new(JsonLdContextRepository::new(&pool));
    let engine = config.jsonld.build_engine(Arc::clone(&context_repo))?;
    let context_workers = TenantContextWorkers {
        hit_flush: engine.hit_counter().map(|hit_counter| {
            ContextHitFlushWorker::spawn(
                hit_counter,
                Arc::clone(&context_repo) as Arc<dyn ContextHitStore>,
                config.database.context_hit_flush.duration(),
            )
        }),
    };
    // Clause 5.13.1: "Implementations shall periodically invalidate the "Cached"
    // @contexts." The obligation is the broker's and not the resolver's, so the
    // pass runs unconditionally as a supervised scheduled task registered from
    // this port, whether or not a hit counter was attached.
    // clone: the pass and every other Context capability share one repository.
    let cached_context_invalidation = Arc::clone(&context_repo) as Arc<dyn CachedContextInvalidation>;

    let attribute_names = Arc::new(NameCache::<AttributeName>::new(&pool));
    let entity_type_names = Arc::new(NameCache::<EntityTypeName>::new(&pool));
    // Read the schema stamp once and hand it to both dictionaries as their
    // baseline, so a later out-of-band drop and recreate of this tenant is
    // detected as a change from a known baseline rather than adopted silently,
    // and the two caches share one catalogue read instead of each issuing its own.
    match read_established_epoch(&pool).await {
        Ok(epoch) => {
            attribute_names.adopt_epoch(epoch);
            entity_type_names.adopt_epoch(epoch);
        }
        Err(error) => tracing::warn!(%error, event = "name_dictionary_epoch_unestablished"),
    }
    if config.database.preload_name_dictionaries {
        if let Err(error) = attribute_names.preload().await {
            tracing::warn!(%error, event = "attribute_names_preload_failed");
        }
        if let Err(error) = entity_type_names.preload().await {
            tracing::warn!(%error, event = "entity_type_names_preload_failed");
        }
    }
    let bulk_write_mode = match config.atomic_write.bulk_mode {
        BulkWriteMode::Unnest => TemporalBulkWriteMode::Unnest,
        BulkWriteMode::CopyStaging => TemporalBulkWriteMode::CopyStaging,
    };
    // One cache per dictionary per tenant, shared by every path that resolves
    // an attribute or type IRI: the temporal catalogue, the entity repository's
    // read path and the entity rows and projection the commit writes.
    let attribute_names = Arc::clone(&attribute_names) as Arc<dyn NameLookup<AttributeName>>; // clone: the cache outlives each port that holds it.
    let entity_type_names = Arc::clone(&entity_type_names) as Arc<dyn NameLookup<EntityTypeName>>; // clone: the cache outlives each port that holds it.
    // Probe the edition once: the TSL build converts aged chunks to the hypercore
    // columnstore, the Apache-2.0 core runs the hypertables uncompressed. The
    // reconciler below gates its columnstore statements on this so an Apache
    // tenant never calls a procedure its edition does not ship.
    let columnstore = ColumnstoreSupport::detect(&pool).await?;
    let temporal_repo = Arc::new(TemporalRepository::new(
        &pool,
        bulk_write_mode,
        Arc::clone(&attribute_names),
        Arc::clone(&entity_type_names),
        columnstore,
    ));
    // The migrations declare the default chunk interval and columnstore lag;
    // an operator override is applied here, after migration and before any
    // history is written through this tenant's repository.
    let storage_policy = temporal_repo.reconcile_storage_policy(&config.temporal.storage_policy()).await?;
    if !storage_policy.is_unchanged() {
        tracing::info!(outcome = ?storage_policy, event = "temporal_storage_policy_reconciled");
    }
    let entity_repository = Arc::new(EntityRepository::new(&pool, Arc::clone(&attribute_names), Arc::clone(&entity_type_names)));
    let entity_ports = EntityStoragePorts {
        reader: Arc::clone(&entity_repository) as Arc<dyn EntityReader>, // clone: every focused port shares one repository and pool.
        query: Arc::clone(&entity_repository) as Arc<dyn EntityQueryStore>, // clone: every focused port shares one repository and pool.
        discovery: Arc::clone(&entity_repository) as Arc<dyn EntityDiscoveryStore>, // clone: every focused port shares one repository and pool.
        expiry: Arc::clone(&entity_repository) as Arc<dyn EntityExpirySource>, // clone: every focused port shares one repository and pool.
        spatial_snapshots: entity_repository as Arc<dyn EntitySpatialSnapshotSource>,
    };
    // clone: the routing projection interns registered attribute names through the tenant's one dictionary.
    let registration_repository = Arc::new(PostgresRegistrationStore::new(&pool, Arc::clone(&attribute_names)));
    // clone: the registration store and the clause 5.7.11 registry view share one repository and pool.
    let registry_type_source = Arc::clone(&registration_repository) as Arc<dyn RegistryTypeSource>;
    let registration_store = registration_repository as Arc<dyn RegistrationStore>;
    let entity_map_store = Arc::new(EntityMapRepository::new(&pool)) as Arc<dyn EntityMapStore>;
    let snapshot_store = Arc::new(SnapshotRepository::new(&pool)) as Arc<dyn SnapshotStore>;
    let subscription_cache = Arc::new(SubscriptionCache::new(tenant.clone()));
    // clone: the cache reload reads through a repository sharing the tenant's one dictionary.
    if let Err(error) = subscription_cache
        .reload(&live_target, &SubscriptionRepository::new(&pool, Arc::clone(&attribute_names)))
        .await
    {
        tracing::error!(%error, "failed to load subscription cache");
    }
    let spatial_index = Arc::new(SpatialEntityIndex::new());
    if let Err(error) = reconcile_location(&spatial_index, entity_ports.spatial_snapshots.as_ref(), tenant).await {
        tracing::warn!(%error, "spatial warmup failed");
    }
    Ok(TenantStorage {
        tenant: tenant.clone(), // clone: the storage graph names the tenant it was opened for.
        pool,
        live_target,
        engine,
        context_workers,
        cached_context_invalidation,
        attribute_names,
        entity_type_names,
        temporal_repo,
        entity_ports,
        registration_store,
        registry_type_source,
        entity_map_store,
        snapshot_store,
        subscription_cache,
        spatial_index,
    })
}
