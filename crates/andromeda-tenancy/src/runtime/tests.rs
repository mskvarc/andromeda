//! Unit tests for the tenant runtime cache: single-flight cold builds, the
//! lease that spares an in-use runtime, and the orphan, excess and idle sweeps.

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
use andromeda_telemetry::tenancy::EvictionReason;
use async_trait::async_trait;
use std::{
    sync::{
        Arc,
        Mutex as StdMutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{
    sync::Notify,
    time::{sleep, timeout},
};

fn named(raw: &str) -> TenantName {
    TenantName::try_from(raw.to_string()).expect("a valid tenant name")
}

fn config(enabled: bool, max_cached: usize) -> TenancyConfig {
    TenancyConfig {
        enabled,
        tenant_db_prefix: DbNamePrefix::try_from("andromeda_tenant").expect("valid default db name prefix"),
        tenant_max_connections: 16,
        runtime_idle_ttl_secs: 900,
        runtime_max_cached: max_cached,
        janitor_interval_secs: 30,
        max_concurrent_builds: 8,
        eviction_concurrency: 8,
    }
}

/// A config with an explicit build-admission and eviction bound, for the tests
/// that assert those bounds are honoured.
fn bounded_config(max_cached: usize, max_concurrent_builds: usize, eviction_concurrency: usize) -> TenancyConfig {
    TenancyConfig {
        max_concurrent_builds,
        eviction_concurrency,
        ..config(true, max_cached)
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
    let leased = manager
        .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
        .await
        .expect("the default tenant resolves");
    pretty_assertions::assert_eq!(leased.state.as_str(), "andromeda");
}

#[tokio::test]
async fn a_second_request_for_one_tenant_reuses_the_runtime_the_first_built() {
    let Harness { manager, factory, .. } = harness(&["acme"], true, 64);
    let tenant = TenantRef::Named(named("acme"));

    let first = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");
    let second = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");

    assert!(Arc::ptr_eq(&first.state, &second.state));
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
    sleep(Duration::from_millis(200)).await;
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), 1, "single-flight must build exactly once");

    gate.notify_waiters();
    let mut states = Vec::new();
    for handle in handles {
        states.push(handle.await.expect("task joins"));
    }
    for leased in &states {
        assert!(Arc::ptr_eq(&leased.state, &states[0].state), "every request resolves to the one built runtime");
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
    let leased = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves");

    // A snapshot that no longer matches the runtime's last_used stands in for
    // a request that touched it after the sweep chose it: it must not evict.
    let evicted = manager.evict_all(vec![(tenant.clone(), Instant::now())], EvictionReason::Idle).await;
    pretty_assertions::assert_eq!(evicted, 0, "a runtime used since the snapshot must be spared");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
    assert!(manager.runtimes.contains_key(&tenant), "the runtime is still held open");

    // Drop the lease: with nothing holding the runtime and the matching
    // snapshot (nothing touched it since), the idle sweep evicts it.
    drop(leased);
    let candidates = manager.idle_runtimes(Duration::ZERO);
    pretty_assertions::assert_eq!(
        manager.evict_all(candidates, EvictionReason::Idle).await,
        1,
        "an untouched, unleased idle runtime is evicted"
    );
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_leased_runtime_survives_idle_and_excess_eviction() {
    // A request holding a lease pins its runtime against every sweep. With a
    // cap of one beside the unnamed default, acme is both idle (zero TTL) and
    // over the cap, yet neither sweep may take it while its lease is held.
    let Harness { manager, shutdowns, .. } = harness(&[], true, 1);
    let _default = manager
        .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
        .await
        .expect("the default resolves");
    let acme = TenantRef::Named(named("acme"));
    let leased = manager.resolve_state(&acme, TenantCreationPolicy::CreateOnDemand).await.expect("resolves");

    // Idle sweep at zero TTL: acme is idle but leased, so it is spared.
    let idle = manager.idle_runtimes(Duration::ZERO);
    pretty_assertions::assert_eq!(manager.evict_all(idle, EvictionReason::Idle).await, 0, "a leased idle runtime is spared");
    // Excess trim at cap one: acme is over the cap but leased, so it is spared.
    pretty_assertions::assert_eq!(manager.trim_excess_runtimes(None).await, 0, "a leased excess runtime is spared");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
    assert!(manager.runtimes.contains_key(&acme), "the leased runtime is still held open");

    // Once the lease drops, the idle sweep may take it.
    drop(leased);
    let idle = manager.idle_runtimes(Duration::ZERO);
    pretty_assertions::assert_eq!(manager.evict_all(idle, EvictionReason::Idle).await, 1, "an unleased idle runtime is evicted");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn excess_trim_skips_a_leased_lru_and_takes_the_next() {
    // The excess trim drops the least-recently-used named runtimes. When the
    // LRU is leased it must be skipped and the next-oldest unleased runtime
    // taken instead, so the cap is honoured without tearing down an in-use
    // runtime.
    let Harness { manager, shutdowns, .. } = harness(&["a", "b", "c"], true, 2);

    // `a` is resolved first, so it is the LRU, and its lease is held. The
    // sleeps space the resolves apart so their last-used stamps differ.
    let leased_a = manager
        .resolve_state(&TenantRef::Named(named("a")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");
    sleep(Duration::from_millis(3)).await;
    // `b` next: the cache now holds {a, b} at cap 2, so nothing is trimmed.
    manager
        .resolve_state(&TenantRef::Named(named("b")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");
    sleep(Duration::from_millis(3)).await;
    // `c` pushes the cache to three at cap 2. The insert-trim must skip the
    // leased LRU `a` and take the next-oldest unleased runtime `b`.
    manager
        .resolve_state(&TenantRef::Named(named("c")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");

    // The insert-trim detaches the victim's teardown, so wait for it to settle
    // before counting shutdowns; the removal itself is synchronous.
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1, "exactly one runtime is trimmed");
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("a"))), "the leased LRU is spared");
    assert!(
        !manager.runtimes.contains_key(&TenantRef::Named(named("b"))),
        "the next-oldest unleased runtime is taken"
    );
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("c"))), "the just-built runtime is kept");
    drop(leased_a);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_concurrent_cold_build_is_never_evicted_by_another_builds_insert_trim() {
    // Twelve distinct tenants cold-build at once against a cap of four. Each
    // build's insert-trim sees the others' just-inserted runtimes, but every
    // one is leased by its still-in-flight request, so none is torn down: the
    // cache overshoots the cap transiently and no request is handed a closed
    // pool. Once the leases drop, a trim brings the cache back to the cap.
    const TENANTS: usize = 12;
    const CAP: usize = 4;

    let names: Vec<String> = (0..TENANTS).map(|index| format!("t{index}")).collect();
    let registry = Arc::new(FakeRegistry::new(&names.iter().map(String::as_str).collect::<Vec<_>>()));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Notify::new());
    let factory = Arc::new(GatedFactory {
        built: AtomicUsize::new(0),
        shutdowns: Arc::clone(&shutdowns),
        gate: Arc::clone(&gate),
    });
    // A build-admission bound above the tenant count so this test exercises the
    // lease, not admission: all twelve must reach the gate at once.
    let manager = TenantRuntimeManager::new(bounded_config(CAP, TENANTS, 8), Arc::clone(&registry), Arc::clone(&factory));

    let mut handles = Vec::new();
    for index in 0..TENANTS {
        let manager = Arc::clone(&manager);
        let tenant = TenantRef::Named(named(&format!("t{index}")));
        handles.push(tokio::spawn(async move {
            manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves")
        }));
    }

    // Let every build reach the gate, so all twelve are cold-building at once.
    sleep(Duration::from_millis(300)).await;
    pretty_assertions::assert_eq!(factory.built.load(Ordering::Relaxed), TENANTS, "all twelve build concurrently");

    gate.notify_waiters();
    let mut leases = Vec::new();
    for handle in handles {
        leases.push(handle.await.expect("task joins"));
    }

    // While every lease is held, no runtime was evicted despite the cap, and
    // every request holds the live cached runtime for its tenant.
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0, "no leased runtime is torn down under the insert-trim");
    pretty_assertions::assert_eq!(manager.runtimes.len(), TENANTS, "the cache overshoots the cap while leases are held");
    for (index, leased) in leases.iter().enumerate() {
        let tenant = TenantRef::Named(named(&format!("t{index}")));
        let cached = manager.runtimes.get(&tenant).expect("the runtime is cached");
        assert!(Arc::ptr_eq(&leased.state, &cached.state), "request holds the live cached runtime");
    }

    // Dropping the leases lets the next trim bring the cache back to the cap.
    drop(leases);
    let trimmed = manager.trim_excess_runtimes(None).await;
    pretty_assertions::assert_eq!(trimmed, TENANTS - CAP, "the trim releases the overshoot once leases drop");
    pretty_assertions::assert_eq!(manager.runtimes.len(), CAP, "the cache settles at the cap");
}

#[tokio::test]
async fn an_unknown_tenant_is_rejected_under_must_exist_and_created_on_demand() {
    let Harness { manager, registry, .. } = harness(&[], true, 64);
    let tenant = TenantRef::Named(named("acme"));

    let refused = manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await;
    assert!(matches!(refused, Err(TenancyError::UnknownTenant { .. })));
    pretty_assertions::assert_eq!(registry.provisioned.load(Ordering::Relaxed), 0);

    let created = manager.resolve_state(&tenant, TenantCreationPolicy::CreateOnDemand).await.expect("resolves");
    pretty_assertions::assert_eq!(created.state.as_str(), "db_acme");
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
async fn resolving_past_the_limit_trims_the_lru_named_tenant_on_insert() {
    // No janitor is spawned in these tests, so the synchronous insert-trim is
    // the only trimming in effect: resolving a third tenant past a cap of two
    // must release the least-recently-used one during that resolve, not wait
    // for a sweep that never runs. Each resolve drops its lease at the end of
    // the statement, so the earlier tenants are unleased when the trim runs.
    let Harness { manager, shutdowns, .. } = harness(&["a", "b", "c"], true, 2);
    for name in ["a", "b", "c"] {
        manager
            .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
        // Space the resolves apart so their millisecond last-used stamps differ
        // and `a` is unambiguously the least-recently-used entry to trim.
        sleep(Duration::from_millis(3)).await;
    }

    pretty_assertions::assert_eq!(manager.runtimes.len(), 2);
    // The removal is synchronous; the teardown is detached, so drain it before
    // counting shutdowns.
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
    assert!(
        !manager.runtimes.contains_key(&TenantRef::Named(named("a"))),
        "the least-recently-used tenant is evicted"
    );
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("b"))));
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("c"))));
}

#[tokio::test]
async fn insert_trim_keeps_the_just_built_runtime_at_capacity_one() {
    // With a cap of one beside the unnamed default, the freshly built named
    // runtime is itself over the cap. Its own lease -- and `protect` -- must
    // spare it, so the resolve hands back a live runtime rather than one the
    // trim just tore down.
    let Harness { manager, shutdowns, .. } = harness(&[], true, 1);
    manager
        .resolve_state(&TenantRef::UnnamedDefault, TenantCreationPolicy::MustExist)
        .await
        .expect("the default resolves");
    let acme = TenantRef::Named(named("acme"));
    let built = manager.resolve_state(&acme, TenantCreationPolicy::CreateOnDemand).await.expect("resolves");

    assert!(
        manager.runtimes.contains_key(&TenantRef::UnnamedDefault),
        "the unnamed default is never evicted"
    );
    assert!(manager.runtimes.contains_key(&acme), "the just-built runtime is protected from its own trim");
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);

    // The returned state is the live cached one: a follow-up hit resolves to
    // the same Arc, proving the trim did not close the pool behind it.
    let hit = manager.resolve_state(&acme, TenantCreationPolicy::MustExist).await.expect("resolves");
    assert!(Arc::ptr_eq(&built.state, &hit.state));
}

#[tokio::test]
async fn insert_trim_is_a_noop_within_the_limit() {
    // Resolving up to but not past the cap trims nothing: the boundary partner
    // to the over-limit case, guarding against over-eager eviction.
    let Harness { manager, shutdowns, .. } = harness(&["a", "b"], true, 2);
    for name in ["a", "b"] {
        manager
            .resolve_state(&TenantRef::Named(named(name)), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
    }

    pretty_assertions::assert_eq!(manager.runtimes.len(), 2);
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 0);
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
    let excess = manager.excess_runtimes();
    assert!(excess.is_empty(), "{excess:?}");
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
    let idle = manager.idle_runtimes(Duration::ZERO);
    let idle_tenants: Vec<TenantRef> = idle.iter().map(|(tenant, _)| tenant.clone()).collect();
    pretty_assertions::assert_eq!(idle_tenants, vec![TenantRef::Named(named("acme"))]);
    pretty_assertions::assert_eq!(manager.evict_all(idle, EvictionReason::Idle).await, 1);
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn a_single_tenant_broker_starts_no_janitor() {
    let Harness { manager, .. } = harness(&[], false, 64);
    assert!(manager.spawn_janitor().is_none());
}

/// A factory that records how many builds run at once, so the admission bound
/// is observable.
struct BuildConcurrencyProbe {
    /// Builds currently in flight.
    active: AtomicUsize,
    /// The highest concurrent build count observed.
    peak: AtomicUsize,
    /// How long each build is held so concurrent builds overlap.
    hold: Duration,
    /// Shared shutdown counter handed to every runtime it builds.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantStateFactory for BuildConcurrencyProbe {
    type State = String;
    type Runtime = FakeRuntime;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, FakeRuntime), TenancyError> {
        let concurrent = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(concurrent, Ordering::SeqCst);
        sleep(self.hold).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok((
            Arc::new(database.as_str().to_owned()),
            FakeRuntime {
                shutdowns: Arc::clone(&self.shutdowns),
            },
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cold_builds_across_tenants_never_exceed_the_configured_bound() {
    // Twelve distinct tenants cold-build at once against a build-admission bound
    // of three. Each build holds long enough to overlap the others, so without
    // the semaphore all twelve would run at once; the bound must hold the peak
    // at three. The cache is large enough that no trim interferes.
    const TENANTS: usize = 12;
    const BOUND: usize = 3;

    let names: Vec<String> = (0..TENANTS).map(|index| format!("t{index}")).collect();
    let registry = Arc::new(FakeRegistry::new(&names.iter().map(String::as_str).collect::<Vec<_>>()));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(BuildConcurrencyProbe {
        active: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
        hold: Duration::from_millis(50),
        shutdowns: Arc::clone(&shutdowns),
    });
    let manager = TenantRuntimeManager::new(bounded_config(TENANTS, BOUND, 8), Arc::clone(&registry), Arc::clone(&factory));

    let mut handles = Vec::new();
    for index in 0..TENANTS {
        let manager = Arc::clone(&manager);
        let tenant = TenantRef::Named(named(&format!("t{index}")));
        handles.push(tokio::spawn(async move {
            drop(manager.resolve_state(&tenant, TenantCreationPolicy::MustExist).await.expect("resolves"));
        }));
    }
    for handle in handles {
        handle.await.expect("task joins");
    }

    let peak = factory.peak.load(Ordering::SeqCst);
    assert!(peak <= BOUND, "no more than {BOUND} cold builds may run at once, but {peak} did");
    assert!(peak > 1, "the builds must overlap for the bound to mean anything, but the peak was {peak}");
}

/// A runtime whose shutdown records how many teardowns run at once, so the
/// eviction bound is observable.
struct ShutdownConcurrencyProbe {
    /// Teardowns currently in flight.
    active: Arc<AtomicUsize>,
    /// The highest concurrent teardown count observed.
    peak: Arc<AtomicUsize>,
    /// How long each teardown is held so concurrent teardowns overlap.
    hold: Duration,
}

#[async_trait]
impl TenantRuntime for ShutdownConcurrencyProbe {
    async fn shutdown(self) {
        let concurrent = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(concurrent, Ordering::SeqCst);
        sleep(self.hold).await;
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A factory that builds [`ShutdownConcurrencyProbe`] runtimes sharing one pair
/// of concurrency counters.
struct ShutdownProbeFactory {
    /// Teardowns currently in flight, shared by every runtime it builds.
    active: Arc<AtomicUsize>,
    /// The highest concurrent teardown count observed.
    peak: Arc<AtomicUsize>,
    /// How long each teardown is held.
    hold: Duration,
}

#[async_trait]
impl TenantStateFactory for ShutdownProbeFactory {
    type State = String;
    type Runtime = ShutdownConcurrencyProbe;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, ShutdownConcurrencyProbe), TenancyError> {
        Ok((
            Arc::new(database.as_str().to_owned()),
            ShutdownConcurrencyProbe {
                active: Arc::clone(&self.active),
                peak: Arc::clone(&self.peak),
                hold: self.hold,
            },
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn evictions_overlap_up_to_the_bound() {
    // Six idle, unleased runtimes are evicted at once against an eviction bound
    // of two. Their teardowns run concurrently, so the peak overlap must reach
    // the bound but never exceed it.
    const TENANTS: usize = 6;
    const BOUND: usize = 2;

    let names: Vec<String> = (0..TENANTS).map(|index| format!("t{index}")).collect();
    let registry = Arc::new(FakeRegistry::new(&names.iter().map(String::as_str).collect::<Vec<_>>()));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let factory = Arc::new(ShutdownProbeFactory {
        active: Arc::clone(&active),
        peak: Arc::clone(&peak),
        hold: Duration::from_millis(50),
    });
    let manager = TenantRuntimeManager::new(bounded_config(TENANTS, 8, BOUND), Arc::clone(&registry), factory);

    for index in 0..TENANTS {
        manager
            .resolve_state(&TenantRef::Named(named(&format!("t{index}"))), TenantCreationPolicy::MustExist)
            .await
            .expect("resolves");
    }

    let idle = manager.idle_runtimes(Duration::ZERO);
    pretty_assertions::assert_eq!(idle.len(), TENANTS, "every runtime is idle at zero TTL");
    pretty_assertions::assert_eq!(manager.evict_all(idle, EvictionReason::Idle).await, TENANTS, "all six are evicted");

    let observed = peak.load(Ordering::SeqCst);
    assert!(observed <= BOUND, "no more than {BOUND} teardowns may overlap, but {observed} did");
    assert!(observed > 1, "the teardowns must overlap for the bound to mean anything, but the peak was {observed}");
}

/// A runtime whose shutdown blocks on a gate until the test opens it, so a
/// detached teardown can be observed to run off the request path.
struct GatedShutdownRuntime {
    /// The gate the teardown waits on before it completes.
    gate: Arc<Notify>,
    /// Incremented once the teardown passes the gate.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantRuntime for GatedShutdownRuntime {
    async fn shutdown(self) {
        self.gate.notified().await;
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
    }
}

/// A factory building [`GatedShutdownRuntime`]s that share one gate.
struct GatedShutdownFactory {
    /// The gate every teardown waits on.
    gate: Arc<Notify>,
    /// Shared shutdown counter.
    shutdowns: Arc<AtomicUsize>,
}

#[async_trait]
impl TenantStateFactory for GatedShutdownFactory {
    type State = String;
    type Runtime = GatedShutdownRuntime;

    async fn build(&self, _tenant: &TenantRef, database: &TenantDatabaseName) -> Result<(Arc<String>, GatedShutdownRuntime), TenancyError> {
        Ok((
            Arc::new(database.as_str().to_owned()),
            GatedShutdownRuntime {
                gate: Arc::clone(&self.gate),
                shutdowns: Arc::clone(&self.shutdowns),
            },
        ))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn insert_trim_does_not_await_victim_shutdown() {
    // With a cap of one, resolving `b` past `a` trims `a`. Its teardown blocks
    // on the gate, yet the resolve must return: the removal is synchronous, the
    // teardown detached. Opening the gate then lets the detached teardown run.
    let registry = Arc::new(FakeRegistry::new(&["a", "b"]));
    let shutdowns = Arc::new(AtomicUsize::new(0));
    let gate = Arc::new(Notify::new());
    let factory = Arc::new(GatedShutdownFactory {
        gate: Arc::clone(&gate),
        shutdowns: Arc::clone(&shutdowns),
    });
    let manager = TenantRuntimeManager::new(config(true, 1), Arc::clone(&registry), factory);

    // Resolve `a`; its lease drops at the statement's end, so `a` is unleased.
    manager
        .resolve_state(&TenantRef::Named(named("a")), TenantCreationPolicy::MustExist)
        .await
        .expect("resolves");
    // Space the resolves so `a` is unambiguously the least-recently-used entry.
    sleep(Duration::from_millis(3)).await;

    // Resolving `b` triggers the insert-trim of `a`. If the trim awaited `a`'s
    // gated teardown this would hang, so a timeout turns that into a failure.
    let resolved_b = timeout(
        Duration::from_secs(2),
        manager.resolve_state(&TenantRef::Named(named("b")), TenantCreationPolicy::MustExist),
    )
    .await
    .expect("the cold insert returns without awaiting the blocked teardown")
    .expect("resolves");
    drop(resolved_b);

    assert!(
        !manager.runtimes.contains_key(&TenantRef::Named(named("a"))),
        "the victim is removed synchronously"
    );
    assert!(manager.runtimes.contains_key(&TenantRef::Named(named("b"))), "the just-built runtime is kept");
    pretty_assertions::assert_eq!(
        shutdowns.load(Ordering::SeqCst),
        0,
        "the teardown is detached, so it has not run while the gate is shut"
    );

    // Open the gate and drain the eviction set: the detached teardown completes.
    gate.notify_one();
    manager.await_evictions().await;
    pretty_assertions::assert_eq!(shutdowns.load(Ordering::SeqCst), 1, "the detached teardown eventually runs");
}
