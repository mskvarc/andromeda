//! Letting go of tenant runtimes: the orphan, excess and idle sweeps, and the
//! conditional, lease-aware removal they all funnel through.

use crate::{
    database::TenantDatabase,
    runtime::TenantRuntimeManager,
    state_factory::{TenantRuntime, TenantStateFactory},
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::time::interval as tokio_interval;
use tracing::info;

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
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

    /// Sweep for orphaned, excess and idle runtimes until the process ends.
    pub(super) async fn run_janitor(self: Arc<TenantRuntimeManager<D, F>>) {
        let interval = Duration::from_secs(self.config.janitor_interval_secs.max(1));
        let ttl = Duration::from_secs(self.config.runtime_idle_ttl_secs.max(1));
        let mut ticker = tokio_interval(interval);
        loop {
            ticker.tick().await;

            let orphaned = self.evict_orphan_runtimes().await;
            if orphaned > 0 {
                info!(orphaned, "Janitor evicted tenant runtimes whose registry row was removed");
            }

            let trimmed = self.trim_excess_runtimes(None).await;
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
        let registered: HashSet<&TenantName> = registered.iter().collect();

        let orphans: Vec<(TenantRef, Instant)> = self
            .runtimes
            .iter()
            .filter_map(|entry| match entry.key() {
                TenantRef::UnnamedDefault => None,
                TenantRef::Named(name) if registered.contains(name) => None,
                // clone: the eviction list outlives the iteration. The last-used
                // stamp is captured so a request that touches this runtime after
                // it is judged an orphan spares it from removal.
                orphan @ TenantRef::Named(_) => Some((orphan.clone(), entry.value().last_used_instant())),
            })
            .collect();

        self.evict_all(orphans).await
    }

    /// Tenants to drop so the cache fits inside `runtime_max_cached`, each with
    /// the last-used instant it was chosen at.
    ///
    /// Leased runtimes -- those a request currently holds -- are never
    /// candidates, so the cache may transiently exceed `runtime_max_cached`
    /// while more than that many requests are in flight at once; the next sweep
    /// or insert-trim brings it back down as their leases drop.
    pub(super) fn excess_runtimes(&self) -> Vec<(TenantRef, Instant)> {
        let over = self.runtimes.len().saturating_sub(self.config.runtime_max_cached);
        if over == 0 {
            return Vec::new();
        }

        let mut candidates: Vec<(TenantRef, Instant)> = self
            .runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && !entry.value().is_leased())
            .map(|entry| (entry.key().clone(), entry.value().last_used_instant())) // clone: the eviction list outlives the iteration
            .collect();
        candidates.sort_by_key(|(_, touched)| *touched);
        candidates.truncate(over);
        candidates
    }

    /// Release the least-recently-used named runtimes until the cache fits
    /// inside `runtime_max_cached`, returning how many were released.
    ///
    /// Runs both on the janitor tick and synchronously after a cold build
    /// inserts a runtime, so the live runtime count -- and with it the open
    /// PostgreSQL connection count -- is bounded rather than left to climb until
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
        self.evict_all(excess).await
    }

    /// Tenants whose runtime has not been used within `ttl`, each with the
    /// last-used instant it was chosen at.
    pub(super) fn idle_runtimes(&self, ttl: Duration) -> Vec<(TenantRef, Instant)> {
        let now = Instant::now();
        self.runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && now.duration_since(entry.value().last_used_instant()) > ttl)
            .map(|entry| (entry.key().clone(), entry.value().last_used_instant())) // clone: the eviction list outlives the iteration
            .collect()
    }

    /// Remove and release each named runtime that is still idle and unleased
    /// since it was chosen for eviction, returning how many were released.
    ///
    /// Each candidate carries the `last_used` instant it was picked at. Removal
    /// is conditional on that instant still matching and on the runtime still
    /// being unleased, so a runtime a request touched or leased between the
    /// sweep's snapshot and this removal is spared -- a request holding its
    /// `Arc` is never handed a torn-down runtime.
    pub(super) async fn evict_all(&self, candidates: Vec<(TenantRef, Instant)>) -> usize {
        let mut evicted = 0;
        for (tenant, chosen_at) in candidates {
            let removed = self
                .runtimes
                .remove_if(&tenant, |_, cached| cached.last_used_instant() == chosen_at && !cached.is_leased());
            if let Some((_, cached)) = removed {
                cached.runtime.shutdown().await;
                evicted += 1;
            }
        }
        evicted
    }
}
