//! Holding a bounded set of tenant runtimes open, and letting go of the rest.

pub mod lease;

mod cached_runtime;
mod eviction;
mod teardown;
#[cfg(test)]
mod tests;

use crate::{
    administration::TenantAdministration,
    config::TenancyConfig,
    creation_policy::TenantCreationPolicy,
    database::TenantDatabase,
    database_name::TenantDatabaseName,
    error::TenancyError,
    lifecycle::TenantLifecycle,
    pending_work::{PendingWork, PendingWorkError, PendingWorkProbe},
    residency::TenantResidency,
    runtime::{
        cached_runtime::CachedRuntime,
        lease::{LeasedState, RuntimeLease},
    },
    state_factory::{TenantRuntime, TenantStateFactory},
    status::TenantStatus,
    tenant_keyset::TenantKeyset,
    tenant_listing::TenantListing,
    tenant_ref::TenantRef,
    tenant_view::TenantView,
    wake_schedule::TenantWakeSchedule,
    waker::WakerPolicy,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_telemetry::tenancy::{ColdBuildOutcome, WakeOutcome, record_build_admission_wait, record_cold_build, record_wake, set_runtimes_cached};
use async_trait::async_trait;
use dashmap::{DashMap, mapref::entry::Entry};
use std::{
    sync::Arc,
    time::{Instant, SystemTime},
};
use tokio::{
    sync::{Mutex, Semaphore},
    task::{JoinHandle, JoinSet},
};
use tracing::warn;

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
    /// Bounds how many tenant runtimes are torn down at once, shared by the
    /// request-path insert-trim and the janitor sweeps so their teardowns share
    /// one global budget rather than each opening its own.
    eviction_permits: Arc<Semaphore>,
    /// Teardowns a request-path insert-trim detached, so the cold request that
    /// triggered the trim does not wait on another tenant's shutdown. Drained by
    /// [`shutdown_all`](Self::shutdown_all) after the server stops.
    eviction_set: Mutex<JoinSet<()>>,
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
        let waker = WakerPolicy::new(
            config.min_dormancy_secs,
            config.wake_residency_secs,
            config.runtime_idle_ttl_secs,
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
            eviction_set: Mutex::new(JoinSet::new()),
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

    /// The state for one tenant, building its runtime if none is open, together
    /// with the lease that keeps it resident for the life of the caller's hold.
    ///
    /// The returned [`LeasedState`] carries a [`RuntimeLease`]: while the caller
    /// holds it, no eviction path releases the runtime, so a request is never
    /// handed a pool eviction closes underneath it. Dropping the returned value
    /// releases the hold.
    ///
    /// # Errors
    ///
    /// Returns [`TenancyError::MultiTenancyDisabled`] when a named tenant is
    /// addressed on a deployment that serves only the unnamed default, and
    /// [`TenancyError::UnknownTenant`] when the tenant is not registered and
    /// `policy` is [`TenantCreationPolicy::MustExist`]. Reading the registry,
    /// provisioning the tenant and building its runtime each surface the
    /// failure the store or the factory reported.
    pub async fn resolve_state(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<LeasedState<F::State>, TenancyError> {
        if !self.config.enabled && !tenant.is_unnamed_default() {
            return Err(TenancyError::MultiTenancyDisabled);
        }

        if let Some(runtime) = self.runtimes.get(tenant) {
            runtime.touch();
            // The lease is cloned under the shard read guard that also reads the
            // state, so the strong count a concurrent eviction reads already
            // counts this hold.
            return Ok(LeasedState {
                state: Arc::clone(&runtime.state),
                lease: RuntimeLease::clone_from(&runtime.origin),
            });
        }

        // Single-flight the cold build: concurrent first requests for one tenant
        // share its build lock, so exactly one opens a runtime. The rest wait,
        // then re-check `runtimes` under the lock and reuse the one it inserted
        // instead of each opening a pool that would be thrown away.
        let build_lock = self.build_lock_for(tenant);
        let _guard = build_lock.lock().await;

        if let Some(runtime) = self.runtimes.get(tenant) {
            runtime.touch();
            return Ok(LeasedState {
                state: Arc::clone(&runtime.state),
                lease: RuntimeLease::clone_from(&runtime.origin),
            });
        }

        // Admission: only a genuine cold build past the second cache check waits
        // here, so the number of runtimes being built at once -- and with it the
        // direct migrator connections and pools they open -- is bounded. Requests
        // past the bound wait for a permit rather than being refused.
        let admission_started = Instant::now();
        let permit = self.build_permits.acquire().await.ok();
        record_build_admission_wait(admission_started.elapsed().as_secs_f64());

        let build_started = Instant::now();
        let result = self.resolve_state_cold(tenant, policy).await;
        let outcome = if result.is_ok() {
            ColdBuildOutcome::Succeeded
        } else {
            ColdBuildOutcome::Failed
        };
        record_cold_build(outcome, build_started.elapsed().as_secs_f64());

        // Release the admission permit before the trim: the trim bounds only the
        // cache size, and holding a build permit across it would charge another
        // tenant's teardown against the concurrent-build budget.
        drop(permit);

        // Enforce runtime_max_cached now the build is cached, detaching each
        // victim's shutdown so this cold request never waits on another tenant's
        // teardown. The freshly built runtime is leased and `protect`ed, so it is
        // never itself the victim.
        if result.is_ok() {
            self.detach_trim_excess_runtimes(tenant).await;
        }

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

    /// Wake a dormant tenant to run background work its probe finds due,
    /// reporting how the attempt resolved.
    ///
    /// Never touches an already-cached tenant: a wake is not a request, so it
    /// must not extend a runtime's residency. Probes before loading, so a
    /// tenant a request or another replica already serviced is not cold-loaded
    /// again. A woken runtime is loaded through the same admission bound as a
    /// request cold build and inserted as the first idle-eviction victim, so it
    /// never displaces a request-touched tenant.
    pub async fn wake(&self, tenant: &TenantName) -> WakeOutcome {
        let outcome = self.wake_inner(tenant).await;
        record_wake(outcome);
        outcome
    }

    /// The wake decision, without the metric the public [`wake`](Self::wake)
    /// records around it.
    async fn wake_inner(&self, tenant: &TenantName) -> WakeOutcome {
        let tenant_ref = TenantRef::Named(tenant.clone()); // clone: the ref outlives this call
        let now = SystemTime::now();

        // A cached tenant runs its own drains; clear its schedule without
        // touching its last use, so a wake never keeps it artificially resident.
        if self.runtimes.contains_key(&tenant_ref) {
            self.settle_schedule(tenant, now, None).await;
            return WakeOutcome::AlreadyCached;
        }

        let record = match self.database.tenant_record(tenant).await {
            Ok(Some(record)) => record,
            Ok(None) => return WakeOutcome::Unregistered,
            // A registry read failure is transient; leave the leased claim to
            // re-fire and report nothing loaded.
            Err(_) => return WakeOutcome::NothingDue,
        };
        // A suspended tenant is never woken: clear its schedule so it is not
        // re-claimed until it is resumed and next accessed.
        if matches!(record.status, TenantStatus::Suspended) {
            self.settle_schedule(tenant, now, None).await;
            return WakeOutcome::NothingDue;
        }
        let database = record.database;

        match self.probe.earliest_pending_work(&database).await {
            Ok(PendingWork::DueAt(due)) if due <= now => self.load_woken(&tenant_ref, database, tenant).await,
            Ok(PendingWork::DueAt(due)) => {
                self.settle_schedule(tenant, now, self.waker.next_wake(PendingWork::DueAt(due), now)).await;
                WakeOutcome::NothingDue
            }
            // Nothing is due, or the tenant's database is gone (deprovisioned
            // under a stale schedule row). Both clear the schedule: a gone tenant
            // must not be re-armed, or it would churn the wake pass every
            // dormancy interval and starve the tenants that really have work.
            Ok(PendingWork::None) | Err(PendingWorkError::DatabaseGone { .. }) => {
                self.settle_schedule(tenant, now, None).await;
                WakeOutcome::NothingDue
            }
            // Transient: the probe could not read this pass. Re-arm a retry
            // rather than treating the tenant as having nothing due.
            Err(PendingWorkError::Saturated { .. } | PendingWorkError::ProbeFailed { .. }) => {
                self.settle_schedule(tenant, now, Some(self.waker.retry_wake(now))).await;
                WakeOutcome::NothingDue
            }
        }
    }

    /// Build and cache a woken tenant's runtime, displacing the
    /// least-recently-used runtime when the cache is full.
    ///
    /// A tenant reaches here only with work already due (see
    /// [`wake_inner`](Self::wake_inner)), so it must run rather than defer behind
    /// runtimes that may have no work at all. When the cache is at capacity it
    /// evicts one unleased runtime -- the least recently used, whose own next wake
    /// is rescheduled as it goes -- so the connection budget stays flat while the
    /// due work is admitted. Only genuine backpressure, every cached runtime held
    /// by an in-flight request so no slot can be freed, defers the wake.
    async fn load_woken(&self, tenant_ref: &TenantRef, database: TenantDatabaseName, tenant: &TenantName) -> WakeOutcome {
        let now = SystemTime::now();
        if self.runtimes.len() >= self.config.runtime_max_cached && !self.evict_one_for_wake().await {
            self.settle_schedule(tenant, now, Some(self.waker.retry_wake(now))).await;
            return WakeOutcome::Deferred;
        }

        // Load under the same admission bound as a request cold build.
        let permit = self.build_permits.acquire().await.ok();
        let built = self.factory.build(tenant_ref, &database).await;
        drop(permit);

        let Ok((state, runtime)) = built else {
            self.settle_schedule(tenant, now, Some(self.waker.retry_wake(now))).await;
            return WakeOutcome::Deferred;
        };

        match self.runtimes.entry(tenant_ref.clone()) {
            Entry::Occupied(_) => {
                // A request cold-loaded the tenant while we built; discard ours.
                runtime.shutdown().await;
                self.settle_schedule(tenant, now, None).await;
                WakeOutcome::AlreadyCached
            }
            Entry::Vacant(vacant) => {
                let cached = CachedRuntime::unleased(state, runtime, database);
                cached.backdate(self.waker.woken_backdate());
                drop(vacant.insert(cached));
                self.record_cache_size();
                // Resident now and running its drains; clear the wake so it is
                // not re-claimed until its next eviction re-probes it.
                self.settle_schedule(tenant, now, None).await;
                WakeOutcome::Loaded
            }
        }
    }

    /// Record a fresh eviction's next wake, logging a schedule write failure
    /// rather than failing the sweep: the leased claim re-fires regardless.
    pub(super) async fn record_schedule(&self, tenant: &TenantName, probed_at: SystemTime, wake_at: Option<SystemTime>) {
        if let Err(error) = self.schedule.record(tenant, probed_at, wake_at).await {
            warn!(tenant = tenant.as_str(), error = %error, "failed to record the tenant wake schedule");
        }
    }

    /// Settle a wake attempt's next schedule, logging a write failure rather
    /// than propagating it.
    async fn settle_schedule(&self, tenant: &TenantName, probed_at: SystemTime, wake_at: Option<SystemTime>) {
        if let Err(error) = self.schedule.settle(tenant, probed_at, wake_at).await {
            warn!(tenant = tenant.as_str(), error = %error, "failed to settle the tenant wake schedule");
        }
    }

    /// Build a runtime for a tenant that has none open, returning it leased.
    async fn resolve_state_cold(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<LeasedState<F::State>, TenancyError> {
        let database = match tenant {
            TenantRef::UnnamedDefault => self.database.default_database(),
            TenantRef::Named(name) => match self.database.tenant_record(name).await? {
                // A suspended tenant is refused on the data path exactly as an
                // absent one is (clause 5.5.10): it is never cold-loaded, so a
                // request bearing its `NGSILD-Tenant` header sees a 404
                // `NonexistentTenant`, and the suspension cannot be probed by
                // whether a request succeeds.
                Some(record) if matches!(record.status, TenantStatus::Suspended) => {
                    return Err(TenancyError::UnknownTenant { name: name.clone() }); // clone: the error outlives this borrow
                }
                Some(record) => record.database,
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
            Entry::Occupied(entry) => {
                entry.get().touch();
                let state = Arc::clone(&entry.get().state);
                let lease = RuntimeLease::clone_from(&entry.get().origin);
                drop(entry);
                runtime.shutdown().await;
                Ok(LeasedState { state, lease })
            }
            Entry::Vacant(vacant) => {
                // The runtime is leased before it enters the cache, so the trim
                // any concurrent cold build runs -- which sees this entry the
                // instant it is inserted -- observes the lease and spares it. The
                // caller enforces runtime_max_cached once the build permit is
                // released, so the trim runs off the admission budget.
                let (cached, leased) = CachedRuntime::newly_leased(Arc::clone(&state), runtime, database);
                drop(vacant.insert(cached));
                self.record_cache_size();
                Ok(LeasedState {
                    state,
                    lease: RuntimeLease::hold(leased),
                })
            }
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
        // Suspending releases the runtime at once so its pool closes
        // deterministically. A leased runtime drops when its lease releases, and
        // admission now refuses the suspended tenant, so nothing re-admits it in
        // the meantime.
        if matches!(status, TenantStatus::Suspended) {
            self.evict_named(&TenantRef::Named(name.clone())).await; // clone: the eviction key outlives this borrow
        }
        self.tenant(name).await
    }
}
