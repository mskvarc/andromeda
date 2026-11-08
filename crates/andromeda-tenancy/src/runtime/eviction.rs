//! Choosing which tenant runtimes to let go of: the orphan, excess, idle and
//! retired sweeps and the janitor loop that drives them.

use crate::{
    database::TenantDatabase,
    pending_work::PendingWork,
    runtime::{TenantRuntimeManager, eviction_rank::EvictionRank, last_used::LastUsed, teardown::next_wake_at},
    state_factory::{TenantRuntime, TenantStateFactory},
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_telemetry::tenancy::{EvictionReason, record_eviction};
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};
use tokio::time::interval as tokio_interval;
use tracing::info;

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// Sweep for orphaned, excess and idle runtimes and settle provisional
    /// tenants until the process ends, and release a suspended tenant's retired
    /// runtime the moment its last lease drops.
    pub(super) async fn run_janitor(self: Arc<TenantRuntimeManager<D, F>>) {
        let interval = self.config.residency.sweep_interval.duration().max(Duration::from_secs(1));
        let ttl = self.config.residency.idle_eviction_after.duration().max(Duration::from_secs(1));
        let mut ticker = tokio_interval(interval);
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                () = self.release_signal.notified() => {
                    self.evict_retired_runtimes().await;
                    continue;
                }
            }

            // A retired runtime whose release signal raced a sweep is picked up
            // on the tick instead.
            self.evict_retired_runtimes().await;

            // Reap teardowns and wake probes the request path detached, so the
            // eviction set does not accumulate finished tasks between shutdowns.
            self.reap_evictions().await;

            let orphaned = self.evict_orphan_runtimes().await;
            if orphaned > 0 {
                info!(orphaned, "Janitor evicted tenant runtimes whose registry row was removed");
            }

            let settlement = self.settle_provisional().await;
            if settlement.established > 0 {
                info!(
                    established = settlement.established,
                    "Janitor established provisional tenants whose databases hold information"
                );
            }
            if settlement.retired > 0 {
                info!(retired = settlement.retired, "Janitor retired provisional tenants that stayed empty");
            }

            let trimmed = self.trim_excess_runtimes(None).await;
            if trimmed > 0 {
                info!(
                    trimmed,
                    max = self.config.residency.max_resident.get(),
                    "Janitor trimmed excess tenant runtimes"
                );
            }

            let evicted = self.sweep_idle_dormant(ttl).await;
            if evicted > 0 {
                info!(evicted, "Janitor evicted idle tenant runtimes with no imminent work");
            }
        }
    }

    /// Sweep idle runtimes: retain any with work due within the TTL, evict the
    /// rest, and record when each evicted tenant is next due to be woken.
    ///
    /// A tenant with an obligation falling due within the idle TTL is kept
    /// resident and its last use bumped: the background work counts as use, so
    /// a tenant busy draining its outbox is not evicted as idle. A tenant with
    /// nothing imminent is evicted and its next wake recorded, so the wake loop
    /// reloads it exactly when its work next falls due.
    pub(super) async fn sweep_idle_dormant(&self, ttl: Duration) -> usize {
        let now = SystemTime::now();
        let mut evicted = 0;
        for (tenant, chosen_at) in self.idle_runtimes(ttl) {
            // Take the database to probe while the shard guard is briefly held,
            // then drop the guard before awaiting; a leased runtime is skipped.
            let (database, existence) = match self.runtimes.get(&tenant) {
                Some(cached) if !cached.is_leased() && cached.runtime.undelivered_changes() == 0 => {
                    (cached.database().clone(), cached.existence()) // clone: the probe outlives the guard
                }
                Some(cached) => {
                    // A backlog is work in progress: the runtime is in use until it is acted on.
                    if !cached.is_leased() {
                        cached.touch();
                    }
                    continue;
                }
                None => continue,
            };

            let probe = self.probe.earliest_pending_work(&database, existence).await;
            if matches!(&probe, Ok(PendingWork::DueAt(due)) if *due <= now + ttl) {
                if let Some(cached) = self.runtimes.get(&tenant) {
                    cached.touch();
                }
                continue;
            }

            let permit = self.teardown_slot().await;
            if let Some(cached) = self.remove_unleased(&tenant, chosen_at) {
                record_eviction(EvictionReason::Dormant);
                self.detach_shutdown(permit, cached.runtime).await;
                if let TenantRef::Named(name) = &tenant {
                    let wake_at = next_wake_at(&self.waker, &probe, now);
                    self.record_schedule(name, now, wake_at).await;
                }
                evicted += 1;
            }
        }
        if evicted > 0 {
            self.record_cache_size();
        }
        evicted
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
        let registered: HashSet<&TenantName> = registered.iter().collect();

        let orphans: Vec<(TenantRef, LastUsed)> = self
            .runtimes
            .iter()
            .filter_map(|entry| match entry.key() {
                TenantRef::UnnamedDefault => None,
                TenantRef::Named(name) if registered.contains(name) => None,
                // clone: the eviction list outlives the iteration. The last-used
                // stamp is captured so a request that touches this runtime after
                // it is judged an orphan spares it from removal.
                orphan @ TenantRef::Named(_) => Some((orphan.clone(), entry.value().last_used())),
            })
            .collect();

        self.evict_all(orphans, EvictionReason::Orphaned).await
    }

    /// Evict every retired runtime (one whose tenant was suspended while a
    /// request held it) that no request holds any longer, returning how many
    /// were evicted.
    ///
    /// Like the suspend itself, never schedules a wake: a suspended tenant must
    /// not be woken.
    pub(super) async fn evict_retired_runtimes(&self) -> usize {
        let retired: Vec<(TenantRef, LastUsed)> = self
            .runtimes
            .iter()
            .filter(|entry| entry.value().is_retired() && !entry.value().is_leased())
            .map(|entry| (entry.key().clone(), entry.value().last_used()))
            .collect();
        if retired.is_empty() {
            return 0;
        }
        self.evict_all(retired, EvictionReason::Suspended).await
    }

    /// Tenants to drop so the cache fits inside `runtime_max_cached`, each with
    /// the last-used instant it was chosen at.
    ///
    /// Leased runtimes (those a request currently holds) are never
    /// candidates, nor is a runtime woken for due work within its wake
    /// residency: its last use is backdated, so it would otherwise be the first
    /// victim of every cold build and be unloaded before its work ran, then
    /// woken again for the same work. The cache may therefore transiently
    /// exceed `runtime_max_cached` while every other runtime is held; the next
    /// sweep or insert-trim brings it back down as leases drop and residencies
    /// end. The candidates are taken in [`EvictionRank`] order, so a runtime
    /// with a periodic notification due within `min_wake_interval` goes only
    /// once every runtime without one has.
    pub(super) fn excess_runtimes(&self) -> Vec<(TenantRef, LastUsed)> {
        let over = self.runtimes.len().saturating_sub(self.config.residency.max_resident.get());
        if over == 0 {
            return Vec::new();
        }

        let now = Instant::now();
        let horizon = self.eviction_horizon();
        let mut candidates: Vec<(TenantRef, EvictionRank)> = self
            .runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && entry.value().is_unloadable(now))
            .map(|entry| (entry.key().clone(), entry.value().eviction_rank(horizon)))
            .collect();
        candidates.sort_by_key(|(_, rank)| *rank);
        candidates.into_iter().take(over).map(|(tenant, rank)| (tenant, rank.last_used)).collect()
    }

    /// Release the least-recently-used named runtimes until the cache fits
    /// inside `runtime_max_cached`, returning how many were released.
    ///
    /// Runs both on the janitor tick and synchronously after a cold build
    /// inserts a runtime, so the live runtime count, and with it the open
    /// PostgreSQL connection count, is bounded rather than left to climb until
    /// the next sweep. `protect` is the tenant a cold build just inserted: it is
    /// never evicted here, so a request is never handed a runtime whose pool
    /// this trim just closed. That freshly built runtime is also leased by the
    /// resolving request, so [`excess_runtimes`](Self::excess_runtimes) already
    /// spares it; `protect` guards the case where its lease is not yet observed.
    pub(super) async fn trim_excess_runtimes(&self, protect: Option<&TenantRef>) -> usize {
        let excess = match protect {
            Some(keep) => self.excess_runtimes().into_iter().filter(|(tenant, _)| tenant != keep).collect(),
            None => self.excess_runtimes(),
        };
        self.evict_all(excess, EvictionReason::Excess).await
    }

    /// The lowest-ranked named runtime other than `exclude` that no request
    /// currently holds and that is past any wake residency, with the last-used
    /// instant it was chosen at, or `None` when every such runtime is leased or
    /// within its wake residency (or the cache holds only the never-evicted
    /// unnamed default).
    ///
    /// Used to free one slot for a tenant about to be loaded (a woken tenant
    /// whose work is due now, or a cold build) so the coldest unleased runtime
    /// yields to it. `exclude` is that tenant, which is never its own victim.
    /// Candidates are ranked by [`EvictionRank`]: a runtime with a periodic
    /// notification due within `min_wake_interval` is chosen only when every
    /// candidate has one, and then the least recently used of them, so
    /// `max_resident` stays a hard bound.
    pub(super) fn lru_unleased_named(&self, exclude: &TenantRef) -> Option<(TenantRef, LastUsed)> {
        let now = Instant::now();
        let horizon = self.eviction_horizon();
        self.runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && entry.key() != exclude && entry.value().is_unloadable(now))
            .map(|entry| (entry.key().clone(), entry.value().eviction_rank(horizon)))
            .min_by_key(|(_, rank)| *rank)
            .map(|(tenant, rank)| (tenant, rank.last_used))
    }

    /// The instant before which a tenant unloaded now would not be woken
    /// again: `min_wake_interval` from now.
    fn eviction_horizon(&self) -> SystemTime {
        SystemTime::now() + self.config.residency.min_wake_interval.duration()
    }

    /// Evict one unleased runtime to make room for a woken tenant, returning
    /// whether a slot was freed.
    ///
    /// The victim is the least-recently-used unleased runtime; it is shut down and
    /// its own next wake rescheduled through the same excess-eviction path a cache
    /// trim uses, so nothing it still owes is lost. Returns `false` when there is
    /// no unleased victim to take (every runtime is held by an in-flight request),
    /// so the caller defers rather than exceeding the connection budget.
    ///
    /// A victim used between its choice and its removal is spared, as is an
    /// idle runtime the janitor finds with work falling due soon; the next
    /// victim is then tried, at most once per place, so a wake is deferred only
    /// when no runtime may be unloaded.
    pub(super) async fn evict_one_for_wake(&self, woken: &TenantRef) -> bool {
        for _ in 0..self.config.residency.max_resident.get() {
            let Some(victim) = self.lru_unleased_named(woken) else {
                return false;
            };
            if self.evict_all(vec![victim], EvictionReason::Excess).await > 0 {
                return true;
            }
        }
        false
    }

    /// Evict the named tenant's runtime at once, returning whether one was
    /// released.
    ///
    /// Used on the suspend path: a suspend must close the tenant's pool
    /// deterministically rather than wait for the next janitor tick. It funnels
    /// through the lease-aware [`evict_all`](Self::evict_all), so a runtime a
    /// request still holds is spared; the suspend retires it first, so admission
    /// refuses it and its last lease to drop wakes the janitor to release it. No
    /// wake is scheduled, because a suspended tenant must never be woken, which
    /// the non-`Excess` reason guarantees.
    pub(super) async fn evict_named(&self, tenant: &TenantRef) -> bool {
        // Snapshot the last-used stamp under the shard guard, then drop it before
        // awaiting; a request that touches the runtime after this snapshot bumps
        // the stamp and the conditional removal spares it, so an in-flight request
        // is never handed a torn-down runtime.
        let Some(chosen_at) = self.runtimes.get(tenant).map(|cached| cached.last_used()) else {
            return false;
        };
        self.evict_all(vec![(tenant.clone(), chosen_at)], EvictionReason::Suspended).await > 0
    }

    /// Tenants whose runtime has not been used within `ttl`, each with the
    /// last-used instant it was chosen at.
    pub(super) fn idle_runtimes(&self, ttl: Duration) -> Vec<(TenantRef, LastUsed)> {
        self.runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && entry.value().last_used().has_been_idle_for(ttl))
            .map(|entry| (entry.key().clone(), entry.value().last_used()))
            .collect()
    }
}
