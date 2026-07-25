use crate::{
    api::{router::build_tenant_state, state::AndromedaState},
    core::{
        config::{RuntimeConfig, TenancyConfig},
        error::AndromedaError,
    },
    tenancy::{
        database::{DatabaseUrl, configure_pool_options, create_database_if_missing, generate_db_name, migrate_main_db, migrate_single_tenant_db},
        types::{TenantCreationPolicy, TenantName, TenantRef, TenantStatus},
    },
    transport::{dispatch::TransportDispatcher, mqtt::MqttTransportPool},
};
use dashmap::DashMap;
use sqlx::Row;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::task::JoinHandle;
use tracing::info;

static EPOCH: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

struct TenantRuntime {
    state: Arc<AndromedaState>,
    worker_handles: Vec<JoinHandle<()>>,
    transport: Arc<TransportDispatcher>,
    pool: sqlx::PgPool,
    last_used: AtomicU64,
}

impl TenantRuntime {
    fn touch(&self) {
        let millis = EPOCH.elapsed().as_millis() as u64;
        self.last_used.store(millis, Ordering::Relaxed);
    }

    fn last_used_instant(&self) -> Instant {
        let millis = self.last_used.load(Ordering::Relaxed);
        *EPOCH + Duration::from_millis(millis)
    }
}

/// Manages per-tenant database connections and worker runtimes.
pub struct TenantRuntimeManager {
    config: Arc<RuntimeConfig>,
    tenancy: TenancyConfig,
    database_url: DatabaseUrl,
    runtimes: DashMap<TenantRef, TenantRuntime>,
    in_flight: Arc<tokio::sync::Mutex<HashSet<TenantRef>>>,
    main_pool: sqlx::PgPool,
    default_db_name: String,
}

impl TenantRuntimeManager {
    /// Create a new tenant runtime manager, connecting to the main database and running migrations.
    pub async fn new(config: Arc<RuntimeConfig>) -> Result<Arc<TenantRuntimeManager>, AndromedaError> {
        let main_pool = configure_pool_options(config.database.max_connections, config.database.synchronous_commit)
            .connect(&config.database.url)
            .await?;

        let database_url = DatabaseUrl::parse(&config.database.url)?;
        let default_db_name = database_url.db_name()?;

        let manager = Arc::new(TenantRuntimeManager {
            tenancy: config.tenancy.clone(),
            database_url,
            default_db_name,
            config,
            runtimes: DashMap::new(),
            in_flight: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            main_pool,
        });

        migrate_main_db(&manager.main_pool).await?;
        manager.ensure_default_tenant_registry_row().await?;

        if manager.tenancy.enabled {
            let janitor = Arc::clone(&manager);
            tokio::spawn(async move {
                janitor.run_janitor().await;
            });
        }

        Ok(manager)
    }

    /// Returns a reference to the main PostgreSQL connection pool.
    pub const fn main_pool(&self) -> &sqlx::PgPool {
        &self.main_pool
    }

    /// Returns the MQTT connection pool from the default tenant runtime, if initialized.
    pub fn default_mqtt_pool(&self) -> Option<Arc<MqttTransportPool>> {
        self.runtimes.get(&TenantRef::UnnamedDefault).map(|r| Arc::clone(r.transport.mqtt_pool()))
    }

    /// Return all named tenants currently present in the tenant registry.
    pub async fn list_named_tenants(&self) -> Result<Vec<TenantName>, AndromedaError> {
        let raw_names: Vec<String> = sqlx::query_scalar("SELECT name FROM tenants WHERE is_unnamed_default = FALSE AND name IS NOT NULL ORDER BY name")
            .fetch_all(&self.main_pool)
            .await?;

        raw_names
            .into_iter()
            .map(|name| TenantName::try_from(name).map_err(|_| AndromedaError::internal("Invalid tenant name value found in tenant registry".to_string())))
            .collect()
    }

    /// Resolve tenant runtime state according to creation policy.
    pub async fn resolve_state(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<Arc<AndromedaState>, AndromedaError> {
        if !self.tenancy.enabled && matches!(tenant, TenantRef::Named(_)) {
            return Err(AndromedaError::multi_tenancy_not_supported());
        }

        if let Some(runtime) = self.runtimes.get(tenant) {
            runtime.touch();
            return Ok(Arc::clone(&runtime.state));
        }

        {
            let mut in_flight = self.in_flight.lock().await;
            if let Some(runtime) = self.runtimes.get(tenant) {
                runtime.touch();
                return Ok(Arc::clone(&runtime.state));
            }
            in_flight.insert(tenant.clone());
        }

        let result = self.resolve_state_cold(tenant, policy).await;
        self.in_flight.lock().await.remove(tenant);
        result
    }

    async fn resolve_state_cold(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<Arc<AndromedaState>, AndromedaError> {
        let db_name = match tenant {
            TenantRef::UnnamedDefault => self.default_db_name.clone(),
            TenantRef::Named(name) => match self.lookup_db_name(name).await? {
                Some(found) => found,
                None => match policy {
                    TenantCreationPolicy::MustExist => {
                        return Err(AndromedaError::nonexistent_tenant(format!("Tenant '{}' does not exist", name)));
                    }
                    TenantCreationPolicy::CreateOnDemand => self.ensure_named_tenant_created(name).await?,
                },
            },
        };

        let db_url = self.database_url.with_db_name(&db_name);
        let (state, handles, transport, pool) = build_tenant_state(&self.config, &db_url, tenant.clone()).await?;
        let state = Arc::new(state);

        match self.runtimes.entry(tenant.clone()) {
            dashmap::mapref::entry::Entry::Occupied(entry) => {
                entry.get().touch();
                for handle in handles {
                    handle.abort();
                }
                transport.shutdown().await;
                pool.close().await;
                Ok(Arc::clone(&entry.get().state))
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                vacant.insert(TenantRuntime {
                    state: Arc::clone(&state),
                    worker_handles: handles,
                    transport,
                    pool,
                    last_used: AtomicU64::new(EPOCH.elapsed().as_millis() as u64),
                });
                Ok(state)
            }
        }
    }

    async fn run_janitor(self: Arc<Self>) {
        let interval = Duration::from_secs(self.tenancy.janitor_interval_seconds.max(1));
        let ttl = Duration::from_secs(self.tenancy.runtime_idle_ttl_seconds.max(1));
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;

            let orphan_count = self.evict_orphan_runtimes().await;
            if orphan_count > 0 {
                info!(orphaned = orphan_count, "Janitor evicted tenant runtimes whose registry row was removed");
            }

            let excess_evictions = collect_excess_evictions(&self.runtimes, self.tenancy.runtime_max_cached);
            if !excess_evictions.is_empty() {
                let trimmed = excess_evictions.len();
                for key in excess_evictions {
                    if let Some((_, runtime)) = self.runtimes.remove(&key) {
                        shutdown_runtime(runtime).await;
                    }
                }
                info!(trimmed, max = self.tenancy.runtime_max_cached, "Janitor trimmed excess tenant runtimes");
            }

            let now = Instant::now();
            let keys_to_evict: Vec<TenantRef> = self
                .runtimes
                .iter()
                .filter(|entry| !matches!(entry.key(), TenantRef::UnnamedDefault) && now.duration_since(entry.value().last_used_instant()) > ttl)
                .map(|entry| entry.key().clone())
                .collect();

            let evict_count = keys_to_evict.len();
            for key in keys_to_evict {
                if let Some((_, runtime)) = self.runtimes.remove(&key) {
                    shutdown_runtime(runtime).await;
                }
            }
            if evict_count > 0 {
                info!(evicted = evict_count, "Janitor evicted idle tenant runtimes");
            }
        }
    }

    /// Synchronously evict every orphan tenant runtime and close its pool.
    ///
    /// Returns the number of runtimes that were evicted. Exposed as a public
    /// method so admin/test tooling can force a sweep without waiting for
    /// the next janitor tick (`janitor_interval_seconds`).
    pub async fn evict_orphan_runtimes(&self) -> usize {
        let keys = self.collect_orphan_runtimes().await;
        let count = keys.len();
        for key in keys {
            if let Some((_, runtime)) = self.runtimes.remove(&key) {
                shutdown_runtime(runtime).await;
            }
        }
        count
    }

    /// Collect keys of cached tenant runtimes whose row no longer exists in the
    /// `tenants` registry table. These are "orphans" — either a tenant was
    /// deleted directly in the database (e.g., by integration tests resetting
    /// state) or an administrative tool removed it. Evicting them frees the
    /// cached connection pool and lets PostgreSQL reclaim the connection slots.
    async fn collect_orphan_runtimes(&self) -> Vec<TenantRef> {
        let registered: Vec<String> = match sqlx::query_scalar("SELECT name FROM tenants WHERE is_unnamed_default = FALSE AND name IS NOT NULL")
            .fetch_all(&self.main_pool)
            .await
        {
            Ok(rows) => rows,
            Err(_) => return Vec::new(),
        };
        let registered: HashSet<String> = registered.into_iter().collect();
        self.runtimes
            .iter()
            .filter_map(|entry| match entry.key() {
                TenantRef::UnnamedDefault => None,
                TenantRef::Named(name) => {
                    if registered.contains(name.as_str()) {
                        None
                    } else {
                        Some(TenantRef::Named(name.clone()))
                    }
                }
            })
            .collect()
    }

    async fn ensure_default_tenant_registry_row(&self) -> Result<(), AndromedaError> {
        let status = TenantStatus::Active;
        sqlx::query(
            r#"
            INSERT INTO tenants (name, is_unnamed_default, db_name, status, created_at, modified_at)
            VALUES (NULL, TRUE, $1, $2, NOW(), NOW())
            ON CONFLICT (db_name) DO UPDATE SET modified_at = NOW()
            "#,
        )
        .bind(&self.default_db_name)
        .bind(status)
        .execute(&self.main_pool)
        .await?;

        Ok(())
    }

    async fn lookup_db_name(&self, tenant_name: &TenantName) -> Result<Option<String>, AndromedaError> {
        let row = sqlx::query("SELECT db_name FROM tenants WHERE name = $1 AND is_unnamed_default = FALSE")
            .bind(tenant_name.as_str())
            .fetch_optional(&self.main_pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>("db_name")))
    }

    async fn ensure_named_tenant_created(&self, tenant_name: &TenantName) -> Result<String, AndromedaError> {
        let mut tx = self.main_pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtext($1))")
            .bind(tenant_name.as_str())
            .execute(&mut *tx)
            .await?;

        if let Some(existing) = sqlx::query("SELECT db_name FROM tenants WHERE name = $1")
            .bind(tenant_name.as_str())
            .fetch_optional(&mut *tx)
            .await?
        {
            let db_name: String = existing.get("db_name");
            tx.commit().await?;
            return Ok(db_name);
        }

        let db_name = generate_db_name(&self.tenancy.tenant_db_prefix, tenant_name);
        create_database_if_missing(&self.database_url, &db_name).await?;

        let status = TenantStatus::Active;
        sqlx::query(
            r#"
            INSERT INTO tenants (name, is_unnamed_default, db_name, status, created_at, modified_at)
            VALUES ($1, FALSE, $2, $3, NOW(), NOW())
            "#,
        )
        .bind(tenant_name.as_str())
        .bind(&db_name)
        .bind(status)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let tenant_url = self.database_url.with_db_name(&db_name);
        migrate_single_tenant_db(&tenant_url, self.tenancy.tenant_max_connections, self.config.database.synchronous_commit).await?;

        Ok(db_name)
    }
}

/// Shut down a tenant runtime: stop transport, abort workers, and close the
/// per-tenant database pool so PostgreSQL connection slots are released
/// immediately rather than waiting for idle_timeout or eventual drop.
async fn shutdown_runtime(runtime: TenantRuntime) {
    runtime.transport.shutdown().await;
    for handle in runtime.worker_handles {
        handle.abort();
    }
    runtime.pool.close().await;
}

/// Collect tenant keys that should be evicted to bring the cache below `max_cached`.
///
/// Returns the keys of the oldest non-default tenants that exceed the limit.
fn collect_excess_evictions(runtimes: &DashMap<TenantRef, TenantRuntime>, max_cached: usize) -> Vec<TenantRef> {
    if runtimes.len() <= max_cached {
        return Vec::new();
    }

    let mut candidates: Vec<(TenantRef, Instant)> = runtimes
        .iter()
        .filter(|entry| !matches!(entry.key(), TenantRef::UnnamedDefault))
        .map(|entry| (entry.key().clone(), entry.value().last_used_instant()))
        .collect();
    candidates.sort_by_key(|(_, touched)| *touched);

    candidates
        .into_iter()
        .take(runtimes.len().saturating_sub(max_cached))
        .map(|(tenant, _)| tenant)
        .collect()
}
