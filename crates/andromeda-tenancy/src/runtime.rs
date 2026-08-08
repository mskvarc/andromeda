//! Holding a bounded set of tenant runtimes open, and letting go of the rest.

use crate::{
    config::TenancyConfig,
    creation_policy::TenantCreationPolicy,
    database::TenantDatabase,
    error::TenancyError,
    state_factory::{TenantRuntime, TenantStateFactory},
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use dashmap::DashMap;
use std::{
    collections::HashSet,
    sync::{
        Arc,
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tracing::info;

/// The instant every last-used stamp is measured from.
///
/// `Instant` is not `Copy`-into-an-atomic, and a runtime's last use is written
/// on every request. Storing an offset in milliseconds from a process-wide
/// epoch makes the write a relaxed atomic store rather than a lock.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Milliseconds elapsed since the process epoch.
fn now_millis() -> u64 {
    EPOCH.elapsed().as_millis() as u64
}

/// One tenant's live runtime, and when it was last reached for.
struct CachedRuntime<S, R> {
    /// The state handlers read.
    state: Arc<S>,
    /// The resources to release on eviction.
    runtime: R,
    /// Milliseconds since [`EPOCH`] at the last use.
    last_used: AtomicU64,
}

impl<S, R> CachedRuntime<S, R> {
    /// Record that this runtime was just used.
    fn touch(&self) {
        self.last_used.store(now_millis(), Ordering::Relaxed);
    }

    /// When this runtime was last used.
    fn last_used_instant(&self) -> Instant {
        *EPOCH + Duration::from_millis(self.last_used.load(Ordering::Relaxed))
    }
}

/// Manages per-tenant runtimes: builds them on demand, hands them out, and
/// evicts the ones nobody is using.
///
/// Generic over both halves of "where does this tenant live" and "what does it
/// need" so that neither the store nor the composition root has to be named
/// here. The unnamed default tenant is never evicted, by any of the three
/// paths below.
pub struct TenantRuntimeManager<D: TenantDatabase, F: TenantStateFactory> {
    /// Limits and intervals.
    config: TenancyConfig,
    /// Where each tenant's data lives.
    database: Arc<D>,
    /// How a tenant's runtime is built.
    factory: Arc<F>,
    /// The runtimes currently held open.
    runtimes: DashMap<TenantRef, CachedRuntime<F::State, F::Runtime>>,
    /// Per-tenant build locks that single-flight a cold start.
    ///
    /// Without this, N concurrent first requests for one tenant would each
    /// build a whole runtime and N-1 would be thrown away — after opening their
    /// connection pools. Concurrent requests share one lock; the first builds,
    /// the rest re-check `runtimes` under the lock and reuse what it inserted.
    build_locks: DashMap<TenantRef, Arc<Mutex<()>>>,
}

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// A manager over the given registry and factory.
    pub fn new(config: TenancyConfig, database: Arc<D>, factory: Arc<F>) -> Arc<TenantRuntimeManager<D, F>> {
        Arc::new(TenantRuntimeManager {
            config,
            database,
            factory,
            runtimes: DashMap::new(),
            build_locks: DashMap::new(),
        })
    }

    /// Start the background janitor, if this deployment serves named tenants.
    ///
    /// A single-tenant broker holds exactly one runtime that is never evicted,
    /// so the sweep would have nothing to do.
    pub fn spawn_janitor(self: &Arc<TenantRuntimeManager<D, F>>) -> Option<tokio::task::JoinHandle<()>> {
        if !self.config.enabled {
            return None;
        }
        let manager = Arc::clone(self);
        Some(tokio::spawn(async move { manager.run_janitor().await }))
    }

    /// Every named tenant currently in the registry.
    pub async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        self.database.registered_tenants().await
    }

    /// Remove and orderly shut down every cached tenant runtime.
    ///
    /// Called after the HTTP server stops accepting traffic, so no new runtime
    /// can be resolved while the cache is drained.
    pub async fn shutdown_all(&self) {
        let tenants = self.runtimes.iter().map(|entry| entry.key().clone()).collect::<Vec<_>>();
        for tenant in tenants {
            if let Some((_tenant, cached)) = self.runtimes.remove(&tenant) {
                cached.runtime.shutdown().await;
            }
        }
    }

    /// The state for one tenant, building its runtime if none is open.
    pub async fn resolve_state(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<Arc<F::State>, TenancyError> {
        if !self.config.enabled && !tenant.is_unnamed_default() {
            return Err(TenancyError::MultiTenancyDisabled);
        }

        if let Some(runtime) = self.runtimes.get(tenant) {
            runtime.touch();
            return Ok(Arc::clone(&runtime.state));
        }

        // Single-flight the cold build: concurrent first requests for one tenant
        // share its build lock, so exactly one opens a runtime. The rest wait,
        // then re-check `runtimes` under the lock and reuse the one it inserted
        // instead of each opening a pool that would be thrown away.
        let build_lock = self.build_lock_for(tenant);
        let _guard = build_lock.lock().await;

        if let Some(runtime) = self.runtimes.get(tenant) {
            runtime.touch();
            return Ok(Arc::clone(&runtime.state));
        }

        let result = self.resolve_state_cold(tenant, policy).await;

        // Drop our build lock now the build is over. The pointer check removes
        // only this lock, never one a later request created after we released
        // it, so the map stays bounded without detaching a lock two requests
        // still share.
        self.build_locks.remove_if(tenant, |_, existing| Arc::ptr_eq(existing, &build_lock));

        result
    }

    /// The build lock for a tenant, creating it on the first request.
    fn build_lock_for(&self, tenant: &TenantRef) -> Arc<Mutex<()>> {
        // clone: hand back an owned Arc so the DashMap shard guard is released
        // before the caller awaits on the lock -- holding a shard guard across
        // an await deadlocks.
        Arc::clone(self.build_locks.entry(tenant.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).value())
    }

    /// Build a runtime for a tenant that has none open.
    async fn resolve_state_cold(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<Arc<F::State>, TenancyError> {
        let database = match tenant {
            TenantRef::UnnamedDefault => self.database.default_database(),
            TenantRef::Named(name) => match self.database.lookup(name).await? {
                Some(found) => found,
                None => match policy {
                    TenantCreationPolicy::MustExist => return Err(TenancyError::UnknownTenant { name: name.clone() }), // clone: the error outlives this borrow
                    TenantCreationPolicy::CreateOnDemand => self.database.provision(name).await?,
                },
            },
        };

        let (state, runtime) = self.factory.build(tenant, &database).await?;

        // A concurrent caller may have won the race between the in-flight
        // release and this insert. The loser releases what it built rather
        // than replacing a runtime other requests are already holding.
        match self.runtimes.entry(tenant.clone()) {
            dashmap::mapref::entry::Entry::Occupied(entry) => {
                entry.get().touch();
                let winner = Arc::clone(&entry.get().state);
                drop(entry);
                runtime.shutdown().await;
                Ok(winner)
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                vacant.insert(CachedRuntime {
                    state: Arc::clone(&state),
                    runtime,
                    last_used: AtomicU64::new(now_millis()),
                });
                Ok(state)
            }
        }
    }

    /// Sweep for orphaned, excess and idle runtimes until the process ends.
    async fn run_janitor(self: Arc<TenantRuntimeManager<D, F>>) {
        let interval = Duration::from_secs(self.config.janitor_interval_seconds.max(1));
        let ttl = Duration::from_secs(self.config.runtime_idle_ttl_seconds.max(1));
        let mut ticker = tokio::time::interval(interval);
        loop {
            ticker.tick().await;

            let orphaned = self.evict_orphan_runtimes().await;
            if orphaned > 0 {
                info!(orphaned, "Janitor evicted tenant runtimes whose registry row was removed");
            }

            let trimmed = self.evict_all(self.excess_runtimes()).await;
            if trimmed > 0 {
                info!(trimmed, max = self.config.runtime_max_cached, "Janitor trimmed excess tenant runtimes");
            }

            let evicted = self.evict_all(self.idle_runtimes(ttl)).await;
            if evicted > 0 {
                info!(evicted, "Janitor evicted idle tenant runtimes");
            }
        }
    }

    /// Evict every cached runtime whose tenant is no longer registered.
    ///
    /// Returns how many were evicted. Public so operational tooling can force
    /// a sweep rather than waiting for the next janitor tick.
    pub async fn evict_orphan_runtimes(&self) -> usize {
        let Ok(registered) = self.database.registered_tenants().await else {
            // A registry that cannot be read is not evidence that anything was
            // removed from it, and evicting on that reading would drop every
            // live runtime the moment the store hiccuped.
            return 0;
        };
        let registered: HashSet<&str> = registered.iter().map(TenantName::as_str).collect();

        let orphans: Vec<(TenantRef, Instant)> = self
            .runtimes
            .iter()
            .filter_map(|entry| match entry.key() {
                TenantRef::UnnamedDefault => None,
                TenantRef::Named(name) if registered.contains(name.as_str()) => None,
                // clone: the eviction list outlives the iteration. The last-used
                // stamp is captured so a request that touches this runtime after
                // it is judged an orphan spares it from removal.
                named => Some((named.clone(), entry.value().last_used_instant())),
            })
            .collect();

        self.evict_all(orphans).await
    }

    /// Tenants to drop so the cache fits inside `runtime_max_cached`, each with
    /// the last-used instant it was chosen at.
    fn excess_runtimes(&self) -> Vec<(TenantRef, Instant)> {
        if self.runtimes.len() <= self.config.runtime_max_cached {
            return Vec::new();
        }

        let mut candidates: Vec<(TenantRef, Instant)> = self
            .runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default())
            .map(|entry| (entry.key().clone(), entry.value().last_used_instant())) // clone: the eviction list outlives the iteration
            .collect();
        candidates.sort_by_key(|(_, touched)| *touched);

        candidates
            .into_iter()
            .take(self.runtimes.len().saturating_sub(self.config.runtime_max_cached))
            .collect()
    }

    /// Tenants whose runtime has not been used within `ttl`, each with the
    /// last-used instant it was chosen at.
    fn idle_runtimes(&self, ttl: Duration) -> Vec<(TenantRef, Instant)> {
        let now = Instant::now();
        self.runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && now.duration_since(entry.value().last_used_instant()) > ttl)
            .map(|entry| (entry.key().clone(), entry.value().last_used_instant())) // clone: the eviction list outlives the iteration
            .collect()
    }

    /// Remove and release each named runtime that has not been used since it was
    /// chosen for eviction, returning how many were released.
    ///
    /// Each candidate carries the `last_used` instant it was picked at. Removal
    /// is conditional on that instant still matching, so a runtime a request
    /// touched between the sweep's snapshot and this removal keeps its
    /// `last_used` bumped and is spared -- a request holding its `Arc` is never
    /// handed a torn-down runtime.
    async fn evict_all(&self, candidates: Vec<(TenantRef, Instant)>) -> usize {
        let mut evicted = 0;
        for (tenant, chosen_at) in candidates {
            let removed = self.runtimes.remove_if(&tenant, |_, cached| cached.last_used_instant() == chosen_at);
            if let Some((_, cached)) = removed {
                cached.runtime.shutdown().await;
                evicted += 1;
            }
        }
        evicted
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        config::TenancyConfig,
        creation_policy::TenantCreationPolicy,
        database::TenantDatabase,
        database_name::TenantDatabaseName,
        db_name_prefix::DbNamePrefix,
        error::TenancyError,
        runtime::TenantRuntimeManager,
        state_factory::{TenantRuntime, TenantStateFactory},
        tenant_ref::TenantRef,
    };
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use async_trait::async_trait;
    use std::{
        sync::{
            Arc,
            Mutex as StdMutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    use tokio::sync::Notify;

    fn named(raw: &str) -> TenantName {
        TenantName::try_from(raw.to_string()).expect("a valid tenant name")
    }

    fn config(enabled: bool, max_cached: usize) -> TenancyConfig {
        TenancyConfig {
            enabled,
            tenant_db_prefix: DbNamePrefix::try_from("andromeda_tenant").expect("valid default db name prefix"),
            tenant_max_connections: 16,
            runtime_idle_ttl_seconds: 900,
            runtime_max_cached: max_cached,
            janitor_interval_seconds: 30,
        }
    }

    /// A registry holding a mutable set of tenants, which records provisioning.
    struct FakeRegistry {
        /// Tenants that are already registered.
        registered: StdMutex<Vec<String>>,
        /// How many times `provision` was called.
        provisioned: AtomicUsize,
    }

    impl FakeRegistry {
        /// A registry holding the given tenants.
        fn new(names: &[&str]) -> FakeRegistry {
            FakeRegistry {
                registered: StdMutex::new(names.iter().map(|name| (*name).to_string()).collect()),
                provisioned: AtomicUsize::new(0),
            }
        }

        /// Every registered name.
        fn all(&self) -> Vec<String> {
            self.registered.lock().map(|names| names.clone()).unwrap_or_default()
        }

        /// Forget a tenant, as an operator dropping its row would.
        fn forget(&self, name: &str) {
            if let Ok(mut names) = self.registered.lock() {
                names.retain(|held| held != name);
            }
        }
    }

    #[async_trait]
    impl TenantDatabase for FakeRegistry {
        fn default_database(&self) -> TenantDatabaseName {
            TenantDatabaseName::from("andromeda".to_string())
        }

        async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
            Ok(self.all().into_iter().filter_map(|name| TenantName::try_from(name).ok()).collect())
        }

        async fn lookup(&self, name: &TenantName) -> Result<Option<TenantDatabaseName>, TenancyError> {
            Ok(self
                .all()
                .iter()
                .any(|held| held == name.as_str())
                .then(|| TenantDatabaseName::from(format!("db_{}", name.as_str()))))
        }

        async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
            self.provisioned.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut names) = self.registered.lock() {
                names.push(name.as_str().to_string());
            }
            Ok(TenantDatabaseName::from(format!("db_{}", name.as_str())))
        }
    }

    /// A runtime that records the fact it was released.
    struct FakeRuntime {
        /// Incremented on shutdown.
        shutdowns: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl TenantRuntime for FakeRuntime {
        async fn shutdown(self) {
            self.shutdowns.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// A factory recording which database each runtime was built against.
    struct FakeFactory {
        /// Shared shutdown counter handed to every runtime it builds.
        shutdowns: Arc<AtomicUsize>,
        /// How many runtimes it has built.
        built: AtomicUsize,
    }

    #[async_trait]
    impl TenantStateFactory for FakeFactory {
        type State = String;
        type Runtime = FakeRuntime;

        async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, FakeRuntime), TenancyError> {
            self.built.fetch_add(1, Ordering::Relaxed);
            Ok((
                Arc::new(database.as_str().to_owned()),
                FakeRuntime {
                    shutdowns: Arc::clone(&self.shutdowns),
                },
            ))
        }
    }

    /// A manager, its registry, its factory and the shutdown counter every
    /// runtime it builds increments.
    struct Harness {
        /// The manager under test.
        manager: Arc<TenantRuntimeManager<FakeRegistry, FakeFactory>>,
        /// The registry it resolves against.
        registry: Arc<FakeRegistry>,
        /// The factory it builds through.
        factory: Arc<FakeFactory>,
        /// How many runtimes have been released.
        shutdowns: Arc<AtomicUsize>,
    }

    fn harness(registered: &[&str], enabled: bool, max_cached: usize) -> Harness {
        let registry = Arc::new(FakeRegistry::new(registered));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let factory = Arc::new(FakeFactory {
            shutdowns: Arc::clone(&shutdowns),
            built: AtomicUsize::new(0),
        });
        let manager = TenantRuntimeManager::new(config(enabled, max_cached), Arc::clone(&registry), Arc::clone(&factory));
        Harness {
            manager,
            registry,
            factory,
            shutdowns,
        }
    }

    #[tokio::test]
    async fn the_unnamed_default_resolves_against_the_database_the_registry_names() {
        let Harness { manager, .. } = harness(&[], true, 64);
        let state = manager
            .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
            .await
            .expect("the default tenant resolves");
        pretty_assertions::assert_eq!(state.as_str(), "andromeda");
    }

    #[tokio::test]
    async fn a_second_request_for_one_tenant_reuses_the_runtime_the_first_built() {
        let Harness { manager, factory, .. } = harness(&["acme"], true, 64);
        let tenant = TenantRef::Named(named("acme"));

        let first = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");
        let second = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");

        assert!(Arc::ptr_eq(&first, &second));
        pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1);
    }

    /// A factory whose every build blocks on a gate until the test opens it, so
    /// concurrent builds overlap and single-flight is observable.
    struct GatedFactory {
        /// How many builds were entered.
        built: AtomicUsize,
        /// Shared shutdown counter handed to every runtime it builds.
        shutdowns: Arc<AtomicUsize>,
        /// The gate every build waits on before completing.
        gate: Arc<Notify>,
    }

    #[async_trait]
    impl TenantStateFactory for GatedFactory {
        type State = String;
        type Runtime = FakeRuntime;

        async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, FakeRuntime), TenancyError> {
            self.built.fetch_add(1, Ordering::Relaxed);
            self.gate.notified().await;
            Ok((
                Arc::new(database.as_str().to_owned()),
                FakeRuntime {
                    shutdowns: Arc::clone(&self.shutdowns),
                },
            ))
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_first_requests_for_one_tenant_trigger_a_single_build() {
        // Without single-flight, N concurrent first requests each open a whole
        // runtime and N-1 are thrown away. A gated factory holds every build
        // open so the requests overlap; single-flight lets exactly one through.
        let registry = Arc::new(FakeRegistry::new(&["acme"]));
        let shutdowns = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new(Notify::new());
        let factory = Arc::new(GatedFactory {
            built: AtomicUsize::new(0),
            shutdowns: Arc::clone(&shutdowns),
            gate: Arc::clone(&gate),
        });
        let manager = TenantRuntimeManager::new(config(true, 64), Arc::clone(&registry), Arc::clone(&factory));
        let tenant = TenantRef::Named(named("acme"));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let manager = Arc::clone(&manager);
            let tenant = tenant.clone(); // clone: each task owns its reference
            handles.push(tokio::spawn(async move {
                manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves")
            }));
        }

        // Let every task reach the build or the single-flight lock.
        tokio::time::sleep(Duration::from_millis(200)).await;
        pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1, "single-flight must build exactly once");

        gate.notify_waiters();
        let mut states = Vec::new();
        for handle in handles {
            states.push(handle.await.expect("task joins"));
        }
        for state in &states {
            assert!(Arc::ptr_eq(state, &states[0]), "every request resolves to the one built runtime");
        }
        pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1);
        // No runtime was built only to be discarded.
        pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn eviction_spares_a_runtime_used_since_it_was_chosen() {
        // Eviction snapshots last_used, then removes. A request that touches the
        // runtime in that window bumps last_used, and the conditional removal
        // must spare it -- otherwise a request holding its Arc is handed a
        // torn-down runtime.
        let Harness { manager, shutdowns, .. } = harness(&["acme"], true, 64);
        let tenant = TenantRef::Named(named("acme"));
        let _state = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");

        // A snapshot that no longer matches the runtime's last_used stands in for
        // a request that touched it after the sweep chose it: it must not evict.
        let evicted = manager.evict_all(vec![(tenant.clone(), Instant::now())]).await;
        pretty_assertions::assert_eq!(evicted, 0, "a runtime used since the snapshot must be spared");
        pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
        assert!(manager.runtimes.contains_key(&tenant), "the runtime is still held open");

        // With the matching snapshot (nothing touched it since), it is evicted.
        let candidates = manager.idle_runtimes(Duration::ZERO);
        pretty_assertions::assert_eq!(manager.evict_all(candidates).await, 1, "an untouched idle runtime is evicted");
        pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn an_unknown_tenant_is_rejected_under_must_exist_and_created_on_demand() {
        let Harness { manager, registry, .. } = harness(&[], true, 64);
        let tenant = TenantRef::Named(named("acme"));

        let refused = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await;
        assert!(matches!(refused, Err(TenancyError::UnknownTenant { .. })));
        pretty_assertions::assert_eq!(registry.provisioned.load(Ordering::Relaxed), 0);

        let created = manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("resolves");
        pretty_assertions::assert_eq!(created.as_str(), "db_acme");
        pretty_assertions::assert_eq!(registry.provisioned.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_named_tenant_is_refused_when_multi_tenancy_is_disabled() {
        let Harness { manager, .. } = harness(&["acme"], false, 64);

        let refused = manager.resolve_state(&TenantRef::Named(named("acme")), TenantCreationPolicy::MustExist).await;
        assert!(matches!(refused, Err(TenancyError::MultiTenancyDisabled)));

        assert!(manager.resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist).await.is_ok());
    }

    #[tokio::test]
    async fn a_tenant_removed_from_the_registry_has_its_runtime_released() {
        let Harness {
            manager, registry, shutdowns, ..
        } = harness(&["acme", "globex"], true, 64);
        for name in ["acme", "globex"] {
            manager
                .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
                .await
                .expect("resolves");
        }

        registry.forget("acme");
        pretty_assertions::assert_eq!(manager.evict_orphan_runtimes().await, 1);
        pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn the_unnamed_default_is_never_evicted_as_an_orphan() {
        let Harness { manager, shutdowns, .. } = harness(&[], true, 64);
        manager
            .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");

        pretty_assertions::assert_eq!(manager.evict_orphan_runtimes().await, 0);
        pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn a_registry_that_cannot_be_read_evicts_nothing_rather_than_everything() {
        /// A registry whose reads always fail.
        struct UnreadableRegistry;

        #[async_trait]
        impl TenantDatabase for UnreadableRegistry {
            fn default_database(&self) -> TenantDatabaseName {
                TenantDatabaseName::from("andromeda".to_string())
            }

            async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
                Err(TenancyError::RegistryUnavailable {
                    detail: "connection refused".to_string(),
                })
            }

            async fn lookup(&self, name: &TenantName) -> Result<Option<TenantDatabaseName>, TenancyError> {
                Ok(Some(TenantDatabaseName::from(format!("db_{}", name.as_str()))))
            }

            async fn provision(&self, name: &TenantName) -> Result<TenantDatabaseName, TenancyError> {
                Ok(TenantDatabaseName::from(format!("db_{}", name.as_str())))
            }
        }

        let shutdowns = Arc::new(AtomicUsize::new(0));
        let factory = Arc::new(FakeFactory {
            shutdowns: Arc::clone(&shutdowns),
            built: AtomicUsize::new(0),
        });
        let manager = TenantRuntimeManager::new(config(true, 64), Arc::new(UnreadableRegistry), factory);
        manager
            .resolve_state(&TenantRef::Named(named("acme")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");

        pretty_assertions::assert_eq!(manager.evict_orphan_runtimes().await, 0);
        pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn a_cache_over_its_limit_drops_the_least_recently_used_named_tenant() {
        let Harness { manager, shutdowns, .. } = harness(&["a", "b", "c"], true, 2);
        for name in ["a", "b", "c"] {
            manager
                .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
                .await
                .expect("resolves");
        }

        let excess = manager.excess_runtimes();
        pretty_assertions::assert_eq!(excess.len(), 1);
        pretty_assertions::assert_eq!(manager.evict_all(excess).await, 1);
        pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
        pretty_assertions::assert_eq!(manager.runtimes.len(), 2);
    }

    #[tokio::test]
    async fn a_cache_inside_its_limit_drops_nothing() {
        let Harness { manager, .. } = harness(&["a", "b"], true, 2);
        for name in ["a", "b"] {
            manager
                .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
                .await
                .expect("resolves");
        }
        assert!(manager.excess_runtimes().is_empty());
    }

    #[tokio::test]
    async fn a_runtime_older_than_the_ttl_is_idle_and_the_default_never_is() {
        let Harness { manager, shutdowns, .. } = harness(&["acme"], true, 64);
        manager
            .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
        manager
            .resolve_state(&TenantRef::Named(named("acme")), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");

        // A zero TTL makes every runtime idle the instant it is inspected.
        let idle = manager.idle_runtimes(std::time::Duration::ZERO);
        let idle_tenants: Vec<TenantRef> = idle.iter().map(|(tenant, _)| tenant.clone()).collect();
        pretty_assertions::assert_eq!(idle_tenants, vec![TenantRef::Named(named("acme"))]);
        pretty_assertions::assert_eq!(manager.evict_all(idle).await, 1);
        pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_single_tenant_broker_starts_no_janitor() {
        let Harness { manager, .. } = harness(&[], false, 64);
        assert!(manager.spawn_janitor().is_none());
    }
}
