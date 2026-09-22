//! Carrying out a runtime eviction: the conditional removal, the bounded
//! teardown set every eviction path funnels through, and the wake each
//! excess-evicted tenant is rescheduled for.

use crate::{
    database::TenantDatabase,
    database_name::TenantDatabaseName,
    pending_work::{PendingWork, PendingWorkError},
    runtime::TenantRuntimeManager,
    state_factory::{TenantRuntime, TenantStateFactory},
    tenant_ref::TenantRef,
    waker::WakerPolicy,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_telemetry::tenancy::{EvictionReason, record_eviction};
use std::{
    sync::Arc,
    time::{Instant, SystemTime},
};
use tokio::{sync::Semaphore, task::JoinSet};
use tracing::debug;

/// When a tenant should next be woken given what its probe found: the policy's
/// due instant when it read cleanly, or a retry instant when the probe failed
/// (a failed probe is retried, never left unscheduled).
pub(super) fn next_wake_at(waker: &WakerPolicy, work: &Result<PendingWork, PendingWorkError>, now: SystemTime) -> Option<SystemTime> {
    match work {
        Ok(work) => waker.next_wake(*work, now),
        Err(_) => Some(waker.retry_wake(now)),
    }
}

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// Remove and orderly shut down every cached tenant runtime.
    ///
    /// Called after the HTTP server stops accepting traffic, so no new runtime
    /// can be resolved while the cache is drained. Each runtime's teardown is
    /// detached and then the whole eviction set is awaited, so this also joins
    /// any teardown a request-path trim left running.
    pub async fn shutdown_all(&self) {
        let tenants = self.runtimes.iter().map(|entry| entry.key().clone()).collect::<Vec<_>>();
        for tenant in tenants {
            if let Some((_tenant, cached)) = self.runtimes.remove(&tenant) {
                self.detach_shutdown(cached.runtime).await;
            }
        }
        self.await_evictions().await;
    }

    /// Remove and release each named runtime that is still idle and unleased
    /// since it was chosen for eviction, returning how many were released.
    ///
    /// Each candidate carries the `last_used` instant it was picked at. Removal
    /// is conditional on that instant still matching and on the runtime still
    /// being unleased, so a runtime a request touched or leased between the
    /// sweep's snapshot and this removal is spared -- a request holding its
    /// `Arc` is never handed a torn-down runtime. `reason` is what the eviction
    /// counter records for each release.
    ///
    /// The removed runtimes are torn down concurrently, up to the eviction
    /// bound, and this call awaits them all: the janitor sweeps and the orphan
    /// eviction have no request waiting on them, so they settle the teardown
    /// here rather than detaching it. The request-path insert-trim detaches
    /// instead -- see [`detach_trim_excess_runtimes`](Self::detach_trim_excess_runtimes).
    pub(super) async fn evict_all(&self, candidates: Vec<(TenantRef, Instant)>, reason: EvictionReason) -> usize {
        let mut teardowns = JoinSet::new();
        let mut evicted = 0;
        for (tenant, chosen_at) in candidates {
            let removed = self
                .runtimes
                .remove_if(&tenant, |_, cached| cached.last_used_instant() == chosen_at && !cached.is_leased());
            if let Some((_, cached)) = removed {
                record_eviction(reason);
                // An excess eviction leaves a still-registered tenant dormant, so
                // it is shut down and its due work scheduled on the eviction set --
                // the probe reads the database only after shutdown has committed
                // any in-flight delivery. An orphan (or the unnamed default) is
                // deregistered and its schedule row cascades away, so it is torn
                // down on this call's awaited set with no wake scheduled.
                if matches!(reason, EvictionReason::Excess)
                    && let TenantRef::Named(name) = &tenant
                {
                    let name = name.clone(); // clone: the detached task outlives this loop
                    let database = cached.database().clone(); // clone: the detached task outlives this loop
                    self.detach_shutdown_then_schedule(cached.runtime, name, database).await;
                } else {
                    teardowns.spawn(Self::bounded_shutdown(Arc::clone(&self.eviction_permits), cached.runtime));
                }
                evicted += 1;
            }
        }
        while teardowns.join_next().await.is_some() {}
        if evicted > 0 {
            self.record_cache_size();
        }
        evicted
    }

    /// Trim the cache back to `runtime_max_cached` after a cold insert, detaching
    /// each victim's teardown so the resolving request does not wait on it.
    ///
    /// `protect` is the tenant this cold build just inserted: it is filtered out
    /// of the candidates, and it is leased by the resolving request in any case,
    /// so a request is never handed a runtime whose pool this trim closed. The
    /// removals are synchronous, so the cache is back inside its bound the moment
    /// this returns; only the teardown is deferred, onto the manager's eviction
    /// set that [`shutdown_all`](Self::shutdown_all) later drains.
    pub(super) async fn detach_trim_excess_runtimes(&self, protect: &TenantRef) {
        let excess: Vec<(TenantRef, Instant)> = self.excess_runtimes().into_iter().filter(|(tenant, _)| tenant != protect).collect();
        for (tenant, chosen_at) in excess {
            let removed = self
                .runtimes
                .remove_if(&tenant, |_, cached| cached.last_used_instant() == chosen_at && !cached.is_leased());
            if let Some((_, cached)) = removed {
                record_eviction(EvictionReason::Excess);
                // The evicted tenant is still registered, so it is shut down and
                // its due work scheduled -- detached so the cold request waits on
                // neither, and with the probe reading only after shutdown has
                // committed any in-flight delivery.
                if let TenantRef::Named(name) = &tenant {
                    let name = name.clone(); // clone: the detached task outlives this loop
                    let database = cached.database().clone(); // clone: the detached task outlives this loop
                    self.detach_shutdown_then_schedule(cached.runtime, name, database).await;
                } else {
                    self.detach_shutdown(cached.runtime).await;
                }
            }
        }
        self.record_cache_size();
    }

    /// Spawn a runtime's teardown onto the manager's eviction set.
    ///
    /// The task is bounded by the eviction permit, so no more than
    /// `eviction_concurrency` teardowns run at once across every eviction path.
    pub(super) async fn detach_shutdown(&self, runtime: F::Runtime) {
        self.eviction_set
            .lock()
            .await
            .spawn(Self::bounded_shutdown(Arc::clone(&self.eviction_permits), runtime));
    }

    /// Shut a runtime down, then probe its database for when its due work should
    /// wake it and record that, all on one detached task on the eviction set.
    ///
    /// The shutdown precedes the probe deliberately. A delivery the runtime was
    /// mid-flight commits and releases its pool during shutdown, so a probe that
    /// ran first could snapshot the database before the obligation landed and
    /// record a null wake -- stranding the evicted tenant until the slow
    /// reconciliation sweep found it. Probing after shutdown reads the settled
    /// state instead. Bounded by the same eviction permit as a plain teardown, so
    /// a thrashing cache cannot flood the pooler with probes; a tenant whose probe
    /// then finds nothing due records a null wake and is not claimed, so an
    /// obligation-free workload adds no wake load.
    async fn detach_shutdown_then_schedule(&self, runtime: F::Runtime, tenant: TenantName, database: TenantDatabaseName) {
        let probe = Arc::clone(&self.probe);
        let schedule = Arc::clone(&self.schedule);
        let permits = Arc::clone(&self.eviction_permits);
        let waker = self.waker;
        self.eviction_set.lock().await.spawn(async move {
            let permit = permits.acquire_owned().await.ok();
            runtime.shutdown().await;
            let now = SystemTime::now();
            let wake_at = next_wake_at(&waker, &probe.earliest_pending_work(&database).await, now);
            if let Err(error) = schedule.record(&tenant, now, wake_at).await {
                debug!(%error, %tenant, "recording an excess-evicted tenant's wake failed");
            }
            drop(permit);
        });
    }

    /// Reap finished teardowns and wake probes from the eviction set without
    /// blocking, so detached request-path work does not accumulate between the
    /// full drains [`shutdown_all`](Self::shutdown_all) performs.
    pub(super) async fn reap_evictions(&self) {
        let mut set = self.eviction_set.lock().await;
        while set.try_join_next().is_some() {}
    }

    /// Shut one runtime down while holding an eviction permit.
    ///
    /// A closed semaphore never arises here -- nothing closes it -- so a permit
    /// that cannot be acquired degrades to an unbounded teardown rather than a
    /// panic or a leaked runtime.
    async fn bounded_shutdown(permits: Arc<Semaphore>, runtime: F::Runtime) {
        let permit = permits.acquire_owned().await.ok();
        runtime.shutdown().await;
        drop(permit);
    }

    /// Await every detached teardown the manager still holds.
    ///
    /// Used by [`shutdown_all`](Self::shutdown_all), and by tests that need the
    /// request-path teardown to have settled before they inspect the shutdown
    /// count.
    pub(super) async fn await_evictions(&self) {
        let mut set = self.eviction_set.lock().await;
        while set.join_next().await.is_some() {}
    }
}
