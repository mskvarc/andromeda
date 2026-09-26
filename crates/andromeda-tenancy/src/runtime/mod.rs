//! Holding a bounded set of tenant runtimes open, and letting go of the rest.

pub mod lease;

mod admission;
mod cached_runtime;
mod eviction;
mod teardown;
#[cfg(test)]
mod tests;
mod wake;

use crate::{
    administration::TenantAdministration,
    config::TenancyConfig,
    database::TenantDatabase,
    error::TenancyError,
    lifecycle::TenantLifecycle,
    pending_work::PendingWorkProbe,
    residency::TenantResidency,
    runtime::{cached_runtime::CachedRuntime, lease::LeaseOrigin},
    state_factory::TenantStateFactory,
    status::TenantStatus,
    tenant_keyset::TenantKeyset,
    tenant_listing::TenantListing,
    tenant_ref::TenantRef,
    tenant_view::TenantView,
    wake_schedule::TenantWakeSchedule,
    waker::WakerPolicy,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_telemetry::tenancy::set_runtimes_cached;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::{
    sync::{Mutex, Notify, Semaphore},
    task::{JoinHandle, JoinSet},
};

/// Manages per-tenant runtimes: builds them on demand, hands them out, and
/// evicts the ones nobody is using.
///
/// Generic over both halves of "where does this tenant live" and "what does it
/// need" so that neither the store nor the composition root has to be named
/// here. The unnamed default tenant is never evicted, by any of the eviction
/// paths in [`eviction`].
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
    /// Bounds how many tenant runtimes are cold-built at once.
    ///
    /// A permit is acquired only past the second cache check, so it is taken
    /// for a genuine cold build and never for a cache hit. Held across the
    /// build and released before the cache trim, so a burst of first requests
    /// for distinct tenants cannot open more than this many connection pools
    /// and migrator connections at once.
    build_permits: Semaphore,
    /// Bounds how many evicted tenant runtimes are alive at once.
    ///
    /// Every eviction path takes a permit before it removes a runtime from the
    /// cache and the permit travels with the runtime into its teardown, so an
    /// evicted runtime is always inside one of these slots: the runtimes alive at
    /// once -- cached plus being torn down -- stay bounded however slowly their
    /// pools close, instead of piling up behind a teardown queue.
    eviction_permits: Arc<Semaphore>,
    /// Bounds how many evicted tenants' databases are probed for due work at
    /// once, so a thrashing cache cannot flood the pooler with probes.
    ///
    /// Separate from [`eviction_permits`](Self::eviction_permits) because the
    /// probe runs after the runtime has been released: holding a teardown slot
    /// across it would slow teardown -- and with it every cold build waiting for
    /// room -- to the pace of the probe.
    wake_probe_permits: Arc<Semaphore>,
    /// Teardowns and wake probes the eviction paths detached, so no request
    /// waits on another tenant's shutdown. Drained by
    /// [`shutdown_all`](Self::shutdown_all) after the server stops.
    eviction_set: Mutex<JoinSet<()>>,
    /// Woken when the last lease on a retired runtime drops, so the janitor
    /// releases a suspended tenant's runtime as soon as nothing holds it.
    release_signal: Arc<Notify>,
    /// Reads a dormant tenant's database for the earliest instant it has work
    /// due, so the janitor retains a tenant with imminent work and the waker
    /// loads only a tenant that has some.
    probe: Arc<dyn PendingWorkProbe>,
    /// The persistent, replica-shared record of when each tenant is next woken.
    schedule: Arc<dyn TenantWakeSchedule>,
    /// The timing policy the janitor and the waker follow.
    waker: WakerPolicy,
}

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// A manager over the given registry, factory, pending-work probe and wake
    /// schedule.
    pub fn new(
        config: TenancyConfig,
        database: Arc<D>,
        factory: Arc<F>,
        probe: Arc<dyn PendingWorkProbe>,
        schedule: Arc<dyn TenantWakeSchedule>,
    ) -> Arc<TenantRuntimeManager<D, F>> {
        let build_permits = Semaphore::new(config.max_concurrent_builds);
        let eviction_permits = Arc::new(Semaphore::new(config.eviction_concurrency));
        let wake_probe_permits = Arc::new(Semaphore::new(config.eviction_concurrency));
        let waker = WakerPolicy::new(
            config.min_dormancy.duration(),
            config.wake_residency.duration(),
            config.runtime_idle_ttl.duration(),
            config.waker_max_wakes_per_pass,
        );
        Arc::new(TenantRuntimeManager {
            config,
            database,
            factory,
            runtimes: DashMap::new(),
            build_locks: DashMap::new(),
            build_permits,
            eviction_permits,
            wake_probe_permits,
            eviction_set: Mutex::new(JoinSet::new()),
            release_signal: Arc::new(Notify::new()),
            probe,
            schedule,
            waker,
        })
    }

    /// Start the background janitor, if this deployment serves named tenants.
    ///
    /// A single-tenant broker holds exactly one runtime that is never evicted,
    /// so the sweep would have nothing to do.
    #[must_use]
    pub fn spawn_janitor(self: &Arc<TenantRuntimeManager<D, F>>) -> Option<JoinHandle<()>> {
        if !self.config.enabled {
            return None;
        }
        let manager = Arc::clone(self);
        Some(tokio::spawn(async move { manager.run_janitor().await }))
    }

    /// A fresh eviction origin for a runtime about to enter the cache, wired to
    /// this manager's release signal.
    fn new_origin(&self) -> Arc<LeaseOrigin> {
        LeaseOrigin::new(Arc::clone(&self.release_signal))
    }

    /// Publish the current number of cached runtimes to the tenancy gauge.
    ///
    /// Called after every insert and eviction so `andromeda_tenant_runtimes_cached`
    /// tracks the live count without a per-tenant label.
    fn record_cache_size(&self) {
        set_runtimes_cached(self.runtimes.len());
    }

    /// Whether a runtime for `tenant` is currently held open in the cache.
    ///
    /// The residency half of a tenant's computed
    /// [`TenantState`](crate::tenant_state::TenantState): a cheap in-memory read
    /// that never touches a tenant database.
    fn residency(&self, tenant: &TenantRef) -> TenantResidency {
        if self.runtimes.contains_key(tenant) {
            TenantResidency::Resident
        } else {
            TenantResidency::NotResident
        }
    }

    /// Release a suspended tenant's runtime, or retire it when a request still
    /// holds it.
    ///
    /// The runtime is retired before the eviction is attempted, so a lease that
    /// drops between the two still wakes the janitor to release it; admission
    /// refuses a retired runtime in the meantime.
    async fn release_suspended(&self, tenant: &TenantRef) {
        if let Some(cached) = self.runtimes.get(tenant) {
            cached.origin.retire();
        }
        self.evict_named(tenant).await;
    }

    /// Return a resumed tenant's runtime to service if a suspend retired it while
    /// a request held it and it is still cached.
    fn reinstate(&self, tenant: &TenantRef) {
        if let Some(cached) = self.runtimes.get(tenant) {
            cached.origin.reinstate();
        }
    }
}

#[async_trait]
impl<D: TenantDatabase + TenantLifecycle, F: TenantStateFactory> TenantAdministration for TenantRuntimeManager<D, F> {
    async fn tenant(&self, name: &TenantName) -> Result<Option<TenantView>, TenancyError> {
        let Some(record) = self.database.tenant_record(name).await? else {
            return Ok(None);
        };
        let residency = self.residency(&TenantRef::Named(name.clone())); // clone: the residency probe keys the cache
        Ok(Some(TenantView::new(record, residency)))
    }

    async fn list_tenants(&self, keyset: &TenantKeyset, limit: i64) -> Result<TenantListing, TenancyError> {
        let (records, total) = self.database.list_tenant_records(keyset, limit).await?;
        let views = records
            .into_iter()
            .map(|record| {
                let residency = self.residency(&TenantRef::Named(record.tenant_name.clone())); // clone: the residency probe keys the cache
                TenantView::new(record, residency)
            })
            .collect();
        Ok(TenantListing::new(views, total))
    }

    async fn set_admin_status(&self, name: &TenantName, status: TenantStatus) -> Result<Option<TenantView>, TenancyError> {
        // A status change on a tenant that does not exist is a 404 for the
        // caller, so nothing is written for an unregistered name.
        if self.database.tenant_record(name).await?.is_none() {
            return Ok(None);
        }
        self.database.set_status(name, status).await?;
        let tenant = TenantRef::Named(name.clone()); // clone: the cache key outlives this borrow
        // Suspending releases the runtime at once so its pool closes
        // deterministically; a runtime a request still holds is retired and
        // released as soon as that hold drops, and admission refuses the tenant
        // in the meantime (docs/tenant-management.md, "Suspend and resume").
        match status {
            TenantStatus::Suspended => self.release_suspended(&tenant).await,
            TenantStatus::Active => self.reinstate(&tenant),
        }
        self.tenant(name).await
    }
}
