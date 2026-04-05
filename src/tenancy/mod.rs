use crate::{
    api::state::AndromedaState,
    core::config::{Configuration, MultitenancyConfig},
    error::AndromedaError,
    transport::dispatch::TransportDispatcher,
};
use sqlx::{Connection, Executor, PgConnection, Row, postgres::PgPoolOptions};
use std::{
    collections::{HashMap, hash_map::Entry},
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{sync::RwLock, task::JoinHandle};
use tracing::info;
use url::Url;

static EPOCH: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum TenantRef {
    UnnamedDefault,
    Named(String),
}

impl TenantRef {
    pub fn from_header(header: Option<String>) -> Self {
        match header {
            Some(value) => Self::Named(value),
            None => Self::UnnamedDefault,
        }
    }

    pub const fn response_header_value(&self) -> Option<&str> {
        match self {
            Self::UnnamedDefault => None,
            Self::Named(value) => Some(value.as_str()),
        }
    }
}

struct TenantRuntime {
    state: Arc<AndromedaState>,
    worker_handles: Vec<JoinHandle<()>>,
    transport: Arc<TransportDispatcher>,
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

pub struct TenantRuntimeManager {
    config: Configuration,
    multitenancy: MultitenancyConfig,
    runtimes: Arc<RwLock<HashMap<TenantRef, TenantRuntime>>>,
    main_pool: sqlx::PgPool,
    default_db_name: String,
}

impl TenantRuntimeManager {
    pub async fn new(config: &Configuration) -> Result<Arc<Self>, AndromedaError> {
        let main_pool = PgPoolOptions::new()
            .max_connections(config.database.max_connections)
            .connect(&config.database.url)
            .await?;

        let manager = Arc::new(Self {
            config: config.clone(),
            multitenancy: config.multitenancy.clone(),
            runtimes: Arc::new(RwLock::new(HashMap::new())),
            main_pool,
            default_db_name: db_name_from_url(&config.database.url)?,
        });

        migrate_main_db(&manager.main_pool).await?;
        manager.ensure_default_tenant_registry_row().await?;

        if manager.multitenancy.enabled {
            let janitor = Arc::clone(&manager);
            tokio::spawn(async move {
                janitor.run_janitor().await;
            });
        }

        Ok(manager)
    }

    pub async fn resolve_state(&self, tenant: &TenantRef, allow_implicit_creation: bool) -> Result<Arc<AndromedaState>, AndromedaError> {
        if !self.multitenancy.enabled && matches!(tenant, TenantRef::Named(_)) {
            return Err(AndromedaError::no_multi_tenant_support(
                "NGSILD-Tenant header is not supported by this deployment",
            ));
        }

        {
            let guard = self.runtimes.read().await;
            if let Some(runtime) = guard.get(tenant) {
                runtime.touch();
                return Ok(Arc::clone(&runtime.state));
            }
        }

        let db_name = match tenant {
            TenantRef::UnnamedDefault => self.default_db_name.clone(),
            TenantRef::Named(name) => match self.lookup_db_name(name).await? {
                Some(db_name) => db_name,
                None => {
                    if !allow_implicit_creation {
                        return Err(AndromedaError::nonexistent_tenant(format!("Tenant '{}' does not exist", name)));
                    }
                    self.ensure_named_tenant_created(name).await?
                }
            },
        };

        let db_url = with_db_name(&self.config.database.url, &db_name)?;
        let (state, handles, transport) = crate::api::router::build_tenant_state(&self.config, &db_url, tenant.clone()).await?;
        let state = Arc::new(state);

        let mut guard = self.runtimes.write().await;
        match guard.entry(tenant.clone()) {
            Entry::Occupied(entry) => {
                entry.get().touch();
                for handle in handles {
                    handle.abort();
                }
                transport.shutdown().await;
                Ok(Arc::clone(&entry.get().state))
            }
            Entry::Vacant(vacant) => {
                vacant.insert(TenantRuntime {
                    state: Arc::clone(&state),
                    worker_handles: handles,
                    transport,
                    last_used: AtomicU64::new(EPOCH.elapsed().as_millis() as u64),
                });
                Ok(state)
            }
        }
    }

    async fn run_janitor(self: Arc<Self>) {
        let interval = Duration::from_secs(self.multitenancy.janitor_interval_seconds.max(1));
        let ttl = Duration::from_secs(self.multitenancy.runtime_idle_ttl_seconds.max(1));
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;
            let mut guard = self.runtimes.write().await;

            if guard.len() > self.multitenancy.runtime_max_cached {
                let trimmed = trim_oldest(&mut guard, self.multitenancy.runtime_max_cached);
                if trimmed > 0 {
                    info!(trimmed, max = self.multitenancy.runtime_max_cached, "Janitor trimmed excess tenant runtimes");
                }
            }

            let now = Instant::now();
            let keys_to_evict: Vec<TenantRef> = guard
                .iter()
                .filter(|(tenant, runtime)| !matches!(tenant, TenantRef::UnnamedDefault) && now.duration_since(runtime.last_used_instant()) > ttl)
                .map(|(tenant, _)| tenant.clone())
                .collect();

            let evict_count = keys_to_evict.len();
            for key in keys_to_evict {
                if let Some(runtime) = guard.remove(&key) {
                    runtime.transport.shutdown().await;
                    for handle in runtime.worker_handles {
                        handle.abort();
                    }
                }
            }
            if evict_count > 0 {
                info!(evicted = evict_count, "Janitor evicted idle tenant runtimes");
            }
        }
    }

    async fn ensure_default_tenant_registry_row(&self) -> Result<(), AndromedaError> {
        sqlx::query(
            r#"
            INSERT INTO tenants (name, is_unnamed_default, db_name, status, created_at, modified_at)
            VALUES (NULL, TRUE, $1, 'active', NOW(), NOW())
            ON CONFLICT (db_name) DO UPDATE SET modified_at = NOW()
            "#,
        )
        .bind(&self.default_db_name)
        .execute(&self.main_pool)
        .await?;

        Ok(())
    }

    async fn lookup_db_name(&self, tenant_name: &str) -> Result<Option<String>, AndromedaError> {
        let row = sqlx::query("SELECT db_name FROM tenants WHERE name = $1 AND is_unnamed_default = FALSE")
            .bind(tenant_name)
            .fetch_optional(&self.main_pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>("db_name")))
    }

    async fn ensure_named_tenant_created(&self, tenant_name: &str) -> Result<String, AndromedaError> {
        let lock_key = advisory_lock_key(tenant_name);
        let mut tx = self.main_pool.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(lock_key).execute(&mut *tx).await?;

        if let Some(existing) = sqlx::query("SELECT db_name FROM tenants WHERE name = $1")
            .bind(tenant_name)
            .fetch_optional(&mut *tx)
            .await?
        {
            let db_name: String = existing.get("db_name");
            tx.commit().await?;
            return Ok(db_name);
        }

        let db_name = generate_db_name(&self.multitenancy.tenant_db_prefix, tenant_name);
        create_database_if_missing(&self.config.database.url, &db_name).await?;

        sqlx::query(
            r#"
            INSERT INTO tenants (name, is_unnamed_default, db_name, status, created_at, modified_at)
            VALUES ($1, FALSE, $2, 'active', NOW(), NOW())
            "#,
        )
        .bind(tenant_name)
        .bind(&db_name)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;

        let tenant_url = with_db_name(&self.config.database.url, &db_name)?;
        let pool = PgPoolOptions::new()
            .max_connections(self.config.database.max_connections)
            .connect(&tenant_url)
            .await?;
        migrate_tenant_db(&pool).await?;
        pool.close().await;

        Ok(db_name)
    }
}

pub(crate) async fn migrate_main_db(pool: &sqlx::PgPool) -> Result<(), AndromedaError> {
    let mut tenant_migrator = sqlx::migrate!("./migrations/tenant");
    tenant_migrator.set_ignore_missing(true);
    tenant_migrator.run(pool).await?;

    let mut platform_migrator = sqlx::migrate!("./migrations/platform");
    platform_migrator.set_ignore_missing(true);
    platform_migrator.run(pool).await?;
    Ok(())
}

pub(crate) async fn migrate_tenant_db(pool: &sqlx::PgPool) -> Result<(), AndromedaError> {
    let mut tenant_migrator = sqlx::migrate!("./migrations/tenant");
    tenant_migrator.set_ignore_missing(true);
    tenant_migrator.run(pool).await?;
    Ok(())
}

fn trim_oldest(runtimes: &mut HashMap<TenantRef, TenantRuntime>, max_cached: usize) -> usize {
    if runtimes.len() <= max_cached {
        return 0;
    }

    let mut candidates: Vec<(TenantRef, Instant)> = runtimes
        .iter()
        .filter(|(tenant, _)| !matches!(tenant, TenantRef::UnnamedDefault))
        .map(|(tenant, runtime)| (tenant.clone(), runtime.last_used_instant()))
        .collect();
    candidates.sort_by_key(|(_, touched)| *touched);

    let mut trimmed = 0;
    for (tenant, _) in candidates.into_iter().take(runtimes.len().saturating_sub(max_cached)) {
        if let Some(runtime) = runtimes.remove(&tenant) {
            for handle in runtime.worker_handles {
                handle.abort();
            }
            trimmed += 1;
        }
    }
    trimmed
}

fn advisory_lock_key(tenant_name: &str) -> i64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tenant_name.hash(&mut hasher);
    hasher.finish() as i64
}

fn db_name_from_url(url: &str) -> Result<String, AndromedaError> {
    let parsed = Url::parse(url).map_err(|e| AndromedaError::internal(format!("Invalid database URL: {e}")))?;
    let path = parsed.path().trim_start_matches('/');
    if path.is_empty() {
        return Err(AndromedaError::internal("Database URL must include database name"));
    }
    Ok(path.to_string())
}

fn with_db_name(url: &str, db_name: &str) -> Result<String, AndromedaError> {
    let mut parsed = Url::parse(url).map_err(|e| AndromedaError::internal(format!("Invalid database URL: {e}")))?;
    parsed.set_path(&format!("/{db_name}"));
    Ok(parsed.to_string())
}

fn sanitize_identifier(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    let collapsed = out.split('_').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("_");
    if collapsed.is_empty() { "tenant".to_string() } else { collapsed }
}

fn generate_db_name(prefix: &str, tenant_name: &str) -> String {
    let sanitized = sanitize_identifier(tenant_name);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    tenant_name.hash(&mut hasher);
    let hash = hasher.finish();
    let short_hash = format!("{:08x}", (hash & 0xffff_ffff) as u32);
    format!("{prefix}_{sanitized}_{short_hash}")
}

async fn create_database_if_missing(base_url: &str, db_name: &str) -> Result<(), AndromedaError> {
    let admin_url = with_db_name(base_url, "postgres")?;
    let mut conn = PgConnection::connect(&admin_url).await?;
    let exists = sqlx::query("SELECT 1 FROM pg_database WHERE datname = $1")
        .bind(db_name)
        .fetch_optional(&mut conn)
        .await?
        .is_some();
    if !exists {
        let sql = format!("CREATE DATABASE \"{}\"", db_name.replace('"', ""));
        conn.execute(sql.as_str()).await?;
    }
    conn.close().await?;
    Ok(())
}
