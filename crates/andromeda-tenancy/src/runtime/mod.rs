//! Holding a bounded set of tenant runtimes open, and letting go of the rest.

pub mod lease;

mod admission;
mod cached_runtime;
mod eviction;
mod eviction_rank;
mod overdue_report;
mod provisional_sweep;
mod resident_view;
mod shed_report;
mod teardown;
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests;
mod wake;
pub mod wake_pass;

use crate::{
    administration::TenantAdministration,
    database::TenantDatabase,
    error::TenancyError,
    lifecycle::TenantLifecycle,
    pending_work::PendingWorkProbe,
    provisional::Establishment,
    registered_tenant::RegisteredTenant,
    residency::TenantResidency,
    runtime::{cached_runtime::CachedRuntime, lease::LeaseOrigin, overdue_report::OverdueReport, shed_report::report_shed},
    state_factory::{TenantRuntime, TenantStateFactory},
    status::TenantStatus,
    tenant_listing::TenantListing,
    tenant_page_start::TenantPageStart,
    tenant_record::TenantRecord,
    tenant_ref::TenantRef,
    tenant_view::TenantView,
    wake_alarm::WakeAlarm,
    wake_backlog::WakeBacklog,
    wake_schedule::TenantWakeSchedule,
    waker::WakerPolicy,
};
use andromeda_configuration::schema::tenancy::TenancyConfig;
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
    /// build a whole runtime and N-1 would be thrown away after opening their
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
    /// once (cached plus being torn down) stay bounded however slowly their
    /// pools close, instead of piling up behind a teardown queue.
    eviction_permits: Arc<Semaphore>,
    /// Bounds how many evicted tenants' databases are probed for due work at
    /// once, so a thrashing cache cannot flood the pooler with probes.
    ///
    /// Separate from [`eviction_permits`](Self::eviction_permits) because the
    /// probe runs after the runtime has been released: holding a teardown slot
    /// across it would slow teardown, and with it every cold build waiting for
    /// room, to the pace of the probe.
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
    /// Rung when this process records a wake due before the waker's sleep
    /// ends.
    alarm: Arc<WakeAlarm>,
    /// The wakes this process deferred and has not since served.
    backlog: WakeBacklog,
    /// The gauges and the warning the overdue wakes of each pass are reported
    /// by.
    overdue_report: OverdueReport,
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
        let build_permits = Semaphore::new(config.build.max_concurrent.get());
        let eviction_permits = Arc::new(Semaphore::new(config.residency.max_concurrent_evictions.get()));
        let wake_probe_permits = Arc::new(Semaphore::new(config.residency.max_concurrent_evictions.get()));
        let waker = WakerPolicy::new(
            config.residency.min_wake_interval.duration(),
            config.residency.woken_lifetime.duration(),
            config.residency.idle_eviction_after.duration(),
            config.residency.max_wakes_per_pass.get(),
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
            alarm: Arc::new(WakeAlarm::new()),
            backlog: WakeBacklog::default(),
            overdue_report: OverdueReport::default(),
        })
    }

    /// The alarm the waker waits on beside its sleep.
    #[must_use]
    pub fn wake_alarm(&self) -> &WakeAlarm {
        &self.alarm
    }

    /// Start the background janitor, if this deployment serves named tenants.
    ///
    /// A single-tenant broker holds exactly one runtime that is never evicted,
    /// so the sweep would have nothing to do.
    #[must_use]
    pub fn spawn_janitor(self: &Arc<TenantRuntimeManager<D, F>>) -> Option<JoinHandle<()>> {
        if !self.config.is_enabled() {
            return None;
        }
        let manager = Arc::clone(self);
        Some(tokio::spawn(async move { manager.run_janitor().await }))
    }

    /// A fresh eviction origin for `runtime` about to enter the cache, wired to
    /// this manager's release signal and to the runtime's activity.
    fn new_origin(&self, runtime: &F::Runtime) -> Arc<LeaseOrigin> {
        LeaseOrigin::new(Arc::clone(&self.release_signal), runtime.activity())
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
    /// The residency half of a tenant's computed `TenantState`: a cheap
    /// in-memory read that never touches a tenant database.
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

    /// Settle a provisional tenant against its database, recording on its
    /// cached runtime, if one is open, that a tenant found to exist does.
    async fn establish_tenant(&self, name: &TenantName) -> Result<Establishment, TenancyError> {
        let establishment = self.database.establish(name).await?;
        let tenant = TenantRef::Named(name.clone()); // clone: the cache is keyed by an owned tenant reference
        if establishment == Establishment::Established
            && let Some(cached) = self.runtimes.get(&tenant)
        {
            cached.establish();
        }
        Ok(establishment)
    }

    /// The registry row of an established tenant, settling a provisional one
    /// first, or `None` when no tenant of that name exists.
    async fn established_record(&self, name: &TenantName) -> Result<Option<TenantRecord>, TenancyError> {
        match self.database.registered_tenant(name).await? {
            Some(RegisteredTenant::Established(record)) => Ok(Some(record)),
            None => Ok(None),
            Some(RegisteredTenant::Provisional { .. }) => match self.establish_tenant(name).await? {
                Establishment::Empty => Ok(None),
                Establishment::Established => match self.database.registered_tenant(name).await? {
                    Some(RegisteredTenant::Established(record)) => Ok(Some(record)),
                    Some(RegisteredTenant::Provisional { .. }) | None => Ok(None),
                },
            },
        }
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
        report_shed(self.tenant_view(name)).await
    }

    async fn list_tenants(&self, start: &TenantPageStart, limit: u32) -> Result<TenantListing, TenancyError> {
        report_shed(self.tenant_listing(start, limit)).await
    }

    async fn set_admin_status(&self, name: &TenantName, status: TenantStatus) -> Result<Option<TenantView>, TenancyError> {
        report_shed(self.apply_admin_status(name, status)).await
    }
}

impl<D: TenantDatabase + TenantLifecycle, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// One tenant's view, or `None` when no such tenant is registered.
    async fn tenant_view(&self, name: &TenantName) -> Result<Option<TenantView>, TenancyError> {
        let Some(record) = self.established_record(name).await? else {
            return Ok(None);
        };
        let residency = self.residency(&TenantRef::Named(name.clone())); // clone: the residency probe keys the cache
        Ok(Some(TenantView::new(record, residency)))
    }

    /// The page of the tenant collection `start` and `limit` select.
    async fn tenant_listing(&self, start: &TenantPageStart, limit: u32) -> Result<TenantListing, TenancyError> {
        // Every provisional tenant is settled before the page is read, so a
        // tenant whose creating operation has answered is listed and counted,
        // and one whose creating operation was refused is not.
        for name in self.database.provisional_tenants().await? {
            self.establish_tenant(&name).await?;
        }
        let (records, total) = self.database.list_tenant_records(start, limit).await?;
        let views = records
            .into_iter()
            .map(|record| {
                let residency = self.residency(&TenantRef::Named(record.tenant_name.clone())); // clone: the residency probe keys the cache
                TenantView::new(record, residency)
            })
            .collect();
        Ok(TenantListing::new(views, total))
    }

    /// Apply an administrative state to a tenant and return its resulting view.
    async fn apply_admin_status(&self, name: &TenantName, status: TenantStatus) -> Result<Option<TenantView>, TenancyError> {
        // A status change on a tenant that does not exist is a 404 for the
        // caller, so nothing is written for an unregistered name or for a
        // provisional tenant that holds no information.
        if self.established_record(name).await?.is_none() {
            return Ok(None);
        }
        self.database.set_status(name, status).await?;
        let tenant = TenantRef::Named(name.clone()); // clone: the cache key outlives this borrow
        // Suspending releases the runtime at once so its pool closes
        // deterministically; a runtime a request still holds is retired and
        // released as soon as that hold drops, and admission refuses the tenant
        // in the meantime.
        match status {
            TenantStatus::Suspended => self.release_suspended(&tenant).await,
            TenantStatus::Active => self.reinstate(&tenant),
        }
        self.tenant_view(name).await
    }
}
