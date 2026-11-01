//! Initialization of one tenant's database-backed storage graph.

use crate::{
    context_hit_flush::ContextHitFlushWorker,
    tenant_factory::{
        TenantFactoryConfig,
        assembly_invariant::AssemblyInvariant,
        error::TenantAssemblyError,
        snapshot_subscriptions::load_snapshot_subscriptions,
        spatial::initialize_spatial,
    },
};
use andromeda_configuration::schema::{database::name_cache::NameLoading, temporal::storage::TemporalStorageConfig};
use andromeda_context_registry::{cached_context_invalidation::CachedContextInvalidation, hit_store::ContextHitStore};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    port::{
        deletion_outline::DeletionOutlineReader,
        discovery::EntityDiscoveryStore,
        entity_types::EntityTypeReader,
        expiry::EntityExpirySource,
        payload_pages::EntityPayloadPages,
        query::EntityQueryStore,
        reader::EntityReader,
        registry_types::RegistryTypeSource,
        spatial_snapshot::EntitySpatialSnapshotSource,
    },
    spatial_acceleration::SpatialAcceleration,
};
use andromeda_entity_map::store::EntityMapStore;
use andromeda_extensions::enabled::EnabledExtensions;
use andromeda_http_context_registry::hosted_context_reference::TenantHostedContexts;
use andromeda_jsonld::{configured_engine::build_engine, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_context_store::{
    context_space::resolve_live_target,
    csource_registration::{pattern_forms_refresh::refresh_stale_pattern_forms, registration_store::PostgresRegistrationStore},
    database_defaults::{DatabaseDefaults, DefaultsReconciliation, reconcile_database_defaults},
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
use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_lookup::NameLookup, schema_epoch::read_established_epoch};
use andromeda_postgresql_pool::{pool_options::configure_pool_options, read_share::ReadShare};
use andromeda_postgresql_temporal_store::{columnstore_capability::ColumnstoreSupport, repo::TemporalRepository};
use andromeda_registration::store::RegistrationStore;
use andromeda_snapshot::store::SnapshotStore;
use andromeda_subscription::cache::SubscriptionCache;
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
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
    /// Which spatial index, if any, the tenant's geo queries are narrowed
    /// through, seeded from the store.
    pub(super) spatial: SpatialAcceleration,
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
    pub(super) payload_pages: Arc<dyn EntityPayloadPages>,
    pub(super) expiry: Arc<dyn EntityExpirySource>,
    pub(super) spatial_snapshots: Arc<dyn EntitySpatialSnapshotSource>,
    pub(super) deletion_outline: Arc<dyn DeletionOutlineReader>,
    pub(super) entity_types: Arc<dyn EntityTypeReader>,
}

/// Migrate a tenant whose schema is behind this binary, or upgrade its
/// Snapshot branches when only they are.
///
/// A current schema can still hold Snapshot branches behind this binary: the
/// tenant was migrated by a replica that has not upgraded them, or an earlier
/// upgrade quarantined one. One `EXISTS` probe over the serving pool decides
/// whether the direct connection is opened at all.
async fn bring_schema_current(pool: &PgPool, migration_url: &Url, defaults: &DatabaseDefaults, acquire_timeout: Duration) -> Result<(), TenantAssemblyError> {
    match tenant_schema_state(pool).await? {
        TenantSchemaState::Current => match snapshot_branch_state(pool).await? {
            SnapshotBranchState::Current => {}
            SnapshotBranchState::Behind => upgrade_single_tenant_snapshot_branches(migration_url, acquire_timeout).await?,
        },
        TenantSchemaState::NeedsMigration => migrate_single_tenant_db(migration_url, defaults, acquire_timeout).await?,
    }
    Ok(())
}

/// Open the tenant's serving pool with its database's session defaults in
/// force.
///
/// A tenant database receives its defaults over a direct connection when it
/// is provisioned or migrated, before any pooled session reaches it. They are
/// reconciled here too, over the pool itself, which works through a pooler as
/// well, so a configuration change reaches a database whose schema is already
/// current. When they had to be written, the pool's own sessions started under
/// the values they replaced, so the pool is closed and opened again before it
/// serves anything. Behind a transaction pooler that reopening reaches only
/// new server sessions: the pooler's existing ones keep the old values until
/// it replaces them.
async fn open_serving_pool(config: &TenantFactoryConfig, serving_url: &Url, max_connections: u32) -> Result<PgPool, TenantAssemblyError> {
    let options = configure_pool_options(max_connections, config.database.acquire_timeout());
    let pool = options.clone().connect(serving_url.as_str()).await?; // clone: the options are needed again when the pool is reopened.
    match reconcile_database_defaults(&pool, &config.database_defaults).await? {
        DefaultsReconciliation::Unchanged => Ok(pool),
        DefaultsReconciliation::Adjusted => {
            pool.close().await;
            Ok(options.connect(serving_url.as_str()).await?)
        }
    }
}

/// Render again the stored restatements of registered identifier patterns an
/// older rendering produced.
///
/// Runs before any request is served: discovery and forwarding evaluate the
/// stored restatements, which must be the ones this build renders.
async fn bring_pattern_forms_current(pool: &PgPool) -> Result<(), TenantAssemblyError> {
    let refreshed = refresh_stale_pattern_forms(pool).await?;
    if refreshed > 0 {
        tracing::info!(
            refreshed,
            event = "registration_pattern_forms_refreshed",
            "registered idPattern restatements rendered again"
        );
    }
    Ok(())
}

/// The tenant's JSON-LD engine, recognising the tenant's own stored contexts.
///
/// Clause 5.13.1: a reference to one of this tenant's own stored contexts is
/// resolved from its storage, never fetched over HTTP from this broker.
fn tenant_engine(config: &TenantFactoryConfig, context_repo: &Arc<JsonLdContextRepository>) -> Result<JsonLdEngine, TenantAssemblyError> {
    let recognizer = Arc::new(TenantHostedContexts::new(
        config.public_url.clone(), // clone: the recogniser owns the configured apiRoot
    ));
    Ok(build_engine(&config.jsonld, Arc::clone(context_repo), recognizer)?)
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
        TenantRef::UnnamedDefault => config.tenancy.default_tenant.max_connections.get(),
        // Through a pooler a named tenant's serving pool holds only a couple of
        // client connections -- the pooler does the real backend pooling -- so a
        // working set larger than the runtime cache cannot churn the host's
        // sockets into ephemeral-port exhaustion. Direct, it is the real backend
        // pool and takes the tenancy-configured size.
        TenantRef::Named(_) => config.database.named_serving_max_connections(config.tenancy.databases.max_connections.get()),
    };
    // Open the serving pool first -- the pooler when one is configured -- and
    // migrate only when the schema is not already current. A warm-schema cold
    // load then never opens the direct migrator connection or takes its session
    // advisory lock, which a transaction-pooling proxy would break; the pooler
    // is left to serve every warm reopen.
    let pool = open_serving_pool(config, serving_url, max_connections).await?;
    bring_schema_current(&pool, migration_url, &config.database_defaults, config.database.acquire_timeout()).await?;
    bring_pattern_forms_current(&pool).await?;
    let live_target = resolve_live_target();
    let context_repo = Arc::new(JsonLdContextRepository::new(&pool));
    let engine = tenant_engine(config, &context_repo)?;
    let context_workers = TenantContextWorkers {
        hit_flush: engine.hit_counter().map(|hit_counter| {
            ContextHitFlushWorker::spawn(
                hit_counter,
                Arc::clone(&context_repo) as Arc<dyn ContextHitStore>,
                config.jsonld.cache.hit_flush_interval.duration(),
            )
        }),
    };
    // Clause 5.13.1: "Implementations shall periodically invalidate the "Cached"
    // @contexts." The obligation is the broker's and not the resolver's, so the
    // pass runs unconditionally as a supervised scheduled task registered from
    // this port, whether or not a hit counter was attached.
    // clone: the pass and every other Context capability share one repository.
    let cached_context_invalidation = Arc::clone(&context_repo) as Arc<dyn CachedContextInvalidation>;

    let attribute_names = Arc::new(NameCache::<AttributeName>::new(&pool, &config.database.name_cache));
    let entity_type_names = Arc::new(NameCache::<EntityTypeName>::new(&pool, &config.database.name_cache));
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
    if matches!(config.database.name_cache.loading, NameLoading::Eager) {
        if let Err(error) = attribute_names.preload().await {
            tracing::warn!(%error, event = "attribute_names_preload_failed");
        }
        if let Err(error) = entity_type_names.preload().await {
            tracing::warn!(%error, event = "entity_type_names_preload_failed");
        }
    }
    // One cache per dictionary per tenant, shared by every path that resolves
    // an attribute or type IRI: the temporal catalogue, the entity repository's
    // read path and the entity rows and projection the commit writes.
    let attribute_names = Arc::clone(&attribute_names) as Arc<dyn NameLookup<AttributeName>>; // clone: the cache outlives each port that holds it.
    let entity_type_names = Arc::clone(&entity_type_names) as Arc<dyn NameLookup<EntityTypeName>>; // clone: the cache outlives each port that holds it.
    // Probe the edition once: the TSL build converts aged chunks to the hypercore
    // columnstore, the Apache-2.0 core runs the hypertables uncompressed. The
    // reconciler below enables the columnstore only where this finds one, so an
    // Apache tenant never calls a procedure its edition does not ship.
    let columnstore = ColumnstoreSupport::detect(&pool).await?;
    // One share for the tenant: entity and temporal read requests together hold
    // no more of the pool than the write reserve leaves them.
    let reads = ReadShare::of(&pool, config.database.write_reserve_percent);
    let temporal_repo = Arc::new(TemporalRepository::new(
        &pool,
        reads.clone(), // clone: the entity store's read requests draw on the same permits.
        Arc::clone(&attribute_names),
        Arc::clone(&entity_type_names),
        columnstore,
    ));
    reconcile_temporal_storage(&temporal_repo, &config.temporal.storage).await?;
    let entity_repository = Arc::new(EntityRepository::new(
        &pool,
        reads,
        Arc::clone(&attribute_names),
        Arc::clone(&entity_type_names),
    ));
    let entity_ports = EntityStoragePorts {
        reader: Arc::clone(&entity_repository) as Arc<dyn EntityReader>, // clone: every focused port shares one repository and pool.
        query: Arc::clone(&entity_repository) as Arc<dyn EntityQueryStore>, // clone: every focused port shares one repository and pool.
        discovery: Arc::clone(&entity_repository) as Arc<dyn EntityDiscoveryStore>, // clone: every focused port shares one repository and pool.
        payload_pages: Arc::clone(&entity_repository) as Arc<dyn EntityPayloadPages>, // clone: every focused port shares one repository and pool.
        expiry: Arc::clone(&entity_repository) as Arc<dyn EntityExpirySource>, // clone: every focused port shares one repository and pool.
        spatial_snapshots: Arc::clone(&entity_repository) as Arc<dyn EntitySpatialSnapshotSource>, // clone: every focused port shares one repository and pool.
        deletion_outline: Arc::clone(&entity_repository) as Arc<dyn DeletionOutlineReader>, // clone: every focused port shares one repository and pool.
        entity_types: entity_repository as Arc<dyn EntityTypeReader>,
    };
    // clone: the registration information rows intern registered attribute names through the tenant's one dictionary.
    let registration_repository = Arc::new(PostgresRegistrationStore::new(&pool, Arc::clone(&attribute_names)));
    // clone: the registration store and the clause 5.7.11 registry view share one repository and pool.
    let registry_type_source = Arc::clone(&registration_repository) as Arc<dyn RegistryTypeSource>;
    let registration_store = registration_repository as Arc<dyn RegistrationStore>;
    let entity_map_store = Arc::new(EntityMapRepository::new(&pool)) as Arc<dyn EntityMapStore>;
    let snapshot_store = Arc::new(SnapshotRepository::new(&pool)) as Arc<dyn SnapshotStore>;
    let stored_filter_matching = EnabledExtensions::from(&config.extensions).language_targeting().matching();
    let subscription_cache = Arc::new(SubscriptionCache::new(tenant.clone(), stored_filter_matching));
    // clone: the cache reload reads through a repository sharing the tenant's one dictionary.
    let subscription_repository = SubscriptionRepository::new(&pool, Arc::clone(&attribute_names));
    if let Err(error) = subscription_cache.reload(&live_target, &subscription_repository).await {
        tracing::error!(%error, "failed to load subscription cache");
    }
    load_snapshot_subscriptions(&subscription_cache, snapshot_store.as_ref(), &subscription_repository).await;
    let spatial = initialize_spatial(config.entity.spatial_index.consistency, &entity_ports.spatial_snapshots, tenant).await?;
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
        spatial,
    })
}

/// Bring the temporal hypertables to the storage policy `temporal` describes.
///
/// The migrations declare the default chunk interval and nothing about the
/// columnstore; the columnstore, its lag and an operator's chunk override are
/// applied here, after migration and before any history is written through the
/// tenant's repository.
///
/// # Errors
///
/// Returns [`TenantAssemblyError`] when the configuration names no valid
/// policy or the reconciliation fails.
async fn reconcile_temporal_storage(temporal_repo: &TemporalRepository, storage: &TemporalStorageConfig) -> Result<(), TenantAssemblyError> {
    let policy = TemporalStoragePolicy::try_from(storage).map_err(AssemblyInvariant::from)?;
    let outcome = temporal_repo.reconcile_storage_policy(&policy).await?;
    if !outcome.is_unchanged() {
        tracing::info!(outcome = ?outcome, event = "temporal_storage_policy_reconciled");
    }
    Ok(())
}
