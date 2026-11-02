//! Carrying out a runtime eviction: the conditional removal, the bounded
//! teardown slot every eviction path takes before it removes a runtime, and the
//! wake each excess-evicted tenant is rescheduled for.

use crate::{
    database::TenantDatabase,
    pending_work::{PendingWork, PendingWorkError},
    runtime::{TenantRuntimeManager, cached_runtime::CachedRuntime},
    state_factory::{TenantRuntime, TenantStateFactory},
    tenant_ref::TenantRef,
    waker::WakerPolicy,
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_telemetry::tenancy::{EvictionReason, record_eviction};
use std::{
    sync::Arc,
    time::{Instant, SystemTime},
};
use tokio::{sync::OwnedSemaphorePermit, task::JoinSet};
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
    /// detached under its teardown slot and then the whole eviction set is
    /// awaited, so this also joins any teardown a request-path trim left
    /// running.
    pub async fn shutdown_all(&self) {
        let tenants = self.runtimes.iter().map(|entry| entry.key().clone()).collect::<Vec<_>>();
        for tenant in tenants {
            let permit = self.teardown_slot().await;
            if let Some((_tenant, cached)) = self.runtimes.remove(&tenant) {
                self.detach_shutdown(permit, cached.runtime).await;
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
    /// sweep's snapshot and this removal is spared, so a request holding its
    /// `Arc` is never handed a torn-down runtime. `reason` is what the eviction
    /// counter records for each release.
    ///
    /// Each removal first takes a teardown slot, so the removed runtimes are torn
    /// down concurrently up to the eviction bound and never pile up alive behind
    /// it. This call awaits them all: the janitor sweeps, the orphan eviction and
    /// a suspend have no request waiting on them (a suspend in fact promises
    /// its pool is closed once it answers), so they settle the teardown here.
    /// An excess eviction of a named tenant instead detaches its teardown and the
    /// wake probe that follows it.
    pub(super) async fn evict_all(&self, candidates: Vec<(TenantRef, Instant)>, reason: EvictionReason) -> usize {
        let mut teardowns = JoinSet::new();
        let mut evicted = 0;
        for (tenant, chosen_at) in candidates {
            let permit = self.teardown_slot().await;
            let Some(cached) = self.remove_unleased(&tenant, chosen_at) else {
                continue;
            };
            record_eviction(reason);
            evicted += 1;
            // An excess eviction leaves a still-registered tenant dormant, so it
            // is shut down and its due work scheduled on the eviction set: the
            // probe reads the database only after shutdown has committed any
            // in-flight delivery. An orphan, a suspended tenant (or the unnamed
            // default) is never woken, so it is torn down on this call's awaited
            // set with no wake scheduled.
            if matches!(reason, EvictionReason::Excess)
                && let TenantRef::Named(name) = &tenant
            {
                self.detach_shutdown_then_schedule(permit, cached, name).await;
            } else {
                teardowns.spawn(Self::shutdown_holding(permit, cached.runtime));
            }
        }
        while teardowns.join_next().await.is_some() {}
        if evicted > 0 {
            self.record_cache_size();
        }
        evicted
    }

    /// Release one excess runtime under a teardown slot the caller already
    /// holds, returning whether it was released.
    ///
    /// The removal is conditional exactly as in [`evict_all`](Self::evict_all);
    /// the teardown, and for a named tenant the wake probe after it, is
    /// detached so the caller waits on neither.
    pub(super) async fn evict_holding(&self, permit: Option<OwnedSemaphorePermit>, tenant: TenantRef, chosen_at: Instant) -> bool {
        let Some(cached) = self.remove_unleased(&tenant, chosen_at) else {
            return false;
        };
        record_eviction(EvictionReason::Excess);
        match &tenant {
            TenantRef::Named(name) => self.detach_shutdown_then_schedule(permit, cached, name).await,
            TenantRef::UnnamedDefault => self.detach_shutdown(permit, cached.runtime).await,
        }
        self.record_cache_size();
        true
    }

    /// Trim the cache back to `runtime_max_cached` after a cold insert, detaching
    /// each victim's teardown so the resolving request does not wait on it.
    ///
    /// Concurrent cold builds can each find room before any of them inserts, so
    /// the cache may briefly exceed its cap by up to `max_concurrent_builds`;
    /// this brings it back. `protect` is the tenant this cold build just
    /// inserted: it is filtered out of the candidates, and it is leased by the
    /// resolving request in any case, so a request is never handed a runtime
    /// whose pool this trim closed. A victim is released only while a teardown
    /// slot is free: when every slot is busy the trim stops rather than queue
    /// runtimes alive behind the teardowns, and the next cold build's room-making
    /// or the janitor's trim picks the overshoot up.
    pub(super) async fn detach_trim_excess_runtimes(&self, protect: &TenantRef) {
        let excess: Vec<(TenantRef, Instant)> = self.excess_runtimes().into_iter().filter(|(tenant, _)| tenant != protect).collect();
        for (tenant, chosen_at) in excess {
            let Ok(permit) = Arc::clone(&self.eviction_permits).try_acquire_owned() else {
                break;
            };
            self.evict_holding(Some(permit), tenant, chosen_at).await;
        }
        self.record_cache_size();
    }

    /// Remove a cached runtime if it is still unleased and untouched since it
    /// was chosen at `chosen_at`.
    pub(super) fn remove_unleased(&self, tenant: &TenantRef, chosen_at: Instant) -> Option<CachedRuntime<F::State, F::Runtime>> {
        self.runtimes
            .remove_if(tenant, |_, cached| cached.last_used_instant() == chosen_at && !cached.is_leased())
            .map(|(_, cached)| cached)
    }

    /// Wait for a teardown slot.
    ///
    /// A closed semaphore never arises here (nothing closes it), so a slot
    /// that cannot be acquired degrades to an unbounded teardown rather than a
    /// panic or a leaked runtime.
    pub(super) async fn teardown_slot(&self) -> Option<OwnedSemaphorePermit> {
        Arc::clone(&self.eviction_permits).acquire_owned().await.ok()
    }

    /// Spawn a runtime's teardown onto the manager's eviction set, holding the
    /// teardown slot the caller took for it until the teardown completes.
    pub(super) async fn detach_shutdown(&self, permit: Option<OwnedSemaphorePermit>, runtime: F::Runtime) {
        self.eviction_set.lock().await.spawn(Self::shutdown_holding(permit, runtime));
    }

    /// Shut a runtime down, then probe its database for when its due work should
    /// wake it and record that, all on one detached task on the eviction set.
    ///
    /// The shutdown precedes the probe deliberately. A delivery the runtime was
    /// mid-flight commits and releases its pool during shutdown, so a probe that
    /// ran first could snapshot the database before the obligation landed and
    /// record a null wake, stranding the evicted tenant until the slow
    /// reconciliation sweep found it. Probing after shutdown reads the settled
    /// state instead. The teardown slot is released as soon as the runtime is
    /// gone, so a slow probe never holds back the next eviction; the probe waits
    /// on its own bound instead, so a thrashing cache cannot flood the pooler
    /// with probes. A tenant whose probe then finds nothing due records a null
    /// wake and is not claimed, so an obligation-free workload adds no wake load.
    async fn detach_shutdown_then_schedule(&self, permit: Option<OwnedSemaphorePermit>, cached: CachedRuntime<F::State, F::Runtime>, tenant: &TenantName) {
        let tenant = tenant.clone(); // clone: the detached task outlives this call
        let database: TenantDatabaseName = cached.database().clone(); // clone: the detached task outlives the cache entry
        let existence = cached.existence();
        let runtime = cached.runtime;
        let probe = Arc::clone(&self.probe);
        let schedule = Arc::clone(&self.schedule);
        let probe_permits = Arc::clone(&self.wake_probe_permits);
        let waker = self.waker;
        let alarm = Arc::clone(&self.alarm);
        self.eviction_set.lock().await.spawn(async move {
            Self::shutdown_holding(permit, runtime).await;
            let probe_permit = probe_permits.acquire_owned().await.ok();
            let now = SystemTime::now();
            let wake_at = next_wake_at(&waker, &probe.earliest_pending_work(&database, existence).await, now);
            match schedule.record(&tenant, now, wake_at).await {
                Ok(()) => {
                    if let Some(wake_at) = wake_at {
                        alarm.recorded(wake_at);
                    }
                }
                Err(error) => debug!(%error, %tenant, "recording an excess-evicted tenant's wake failed"),
            }
            drop(probe_permit);
        });
    }

    /// Reap finished teardowns and wake probes from the eviction set without
    /// blocking, so detached request-path work does not accumulate between the
    /// full drains [`shutdown_all`](Self::shutdown_all) performs.
    pub(super) async fn reap_evictions(&self) {
        let mut set = self.eviction_set.lock().await;
        while set.try_join_next().is_some() {}
    }

    /// Shut one runtime down while holding its teardown slot.
    async fn shutdown_holding(permit: Option<OwnedSemaphorePermit>, runtime: F::Runtime) {
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
