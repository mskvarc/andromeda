//! Holding a bounded set of tenant runtimes open, and letting go of the rest.

pub mod lease;

mod cached_runtime;
mod eviction;
#[cfg(test)]
mod tests;

use crate::{
    administration::{OrphanTenantEvictor, TenantDirectory},
    config::TenancyConfig,
    creation_policy::TenantCreationPolicy,
    database::TenantDatabase,
    error::TenancyError,
    runtime::{
        cached_runtime::CachedRuntime,
        lease::{LeasedState, RuntimeLease},
    },
    state_factory::{TenantRuntime, TenantStateFactory},
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;
use dashmap::{DashMap, mapref::entry::Entry};
use std::sync::Arc;
use tokio::{sync::Mutex, task::JoinHandle};

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
    #[must_use]
    pub fn spawn_janitor(self: &Arc<TenantRuntimeManager<D, F>>) -> Option<JoinHandle<()>> {
        if !self.config.enabled {
            return None;
        }
        let manager = Arc::clone(self);
        Some(tokio::spawn(async move { manager.run_janitor().await }))
    }

    /// Every named tenant currently in the registry.
    ///
    /// # Errors
    ///
    /// Returns whatever the registry reports, which for a store that cannot be
    /// reached is [`TenancyError::RegistryUnavailable`].
    pub async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        self.database.registered_tenants().await
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

    /// Build a runtime for a tenant that has none open, returning it leased.
    async fn resolve_state_cold(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<LeasedState<F::State>, TenancyError> {
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
                // instant it is inserted -- observes the lease and spares it.
                let (cached, leased) = CachedRuntime::newly_leased(Arc::clone(&state), runtime);
                // Release the shard guard before awaiting: the trim iterates the
                // map and awaits shutdown of evicted runtimes, and holding a
                // DashMap guard across an await deadlocks (see build_lock_for).
                drop(vacant.insert(cached));
                // Enforce runtime_max_cached now, not only on the next janitor
                // tick, so a burst of cold tenant builds cannot pile per-tenant
                // pools past the configured bound between sweeps. Leased entries
                // -- every in-flight cold build's own runtime among them -- are
                // spared, so the cache overshoots transiently under a burst and
                // is trimmed back as the leases drop.
                self.trim_excess_runtimes(Some(tenant)).await;
                Ok(LeasedState {
                    state,
                    lease: RuntimeLease::hold(leased),
                })
            }
        }
    }
}

#[async_trait]
impl<D: TenantDatabase, F: TenantStateFactory> TenantDirectory for TenantRuntimeManager<D, F> {
    async fn registered_tenants(&self) -> Result<Vec<TenantName>, TenancyError> {
        TenantRuntimeManager::registered_tenants(self).await
    }
}

#[async_trait]
impl<D: TenantDatabase, F: TenantStateFactory> OrphanTenantEvictor for TenantRuntimeManager<D, F> {
    async fn evict_orphan_runtimes(&self) -> usize {
        TenantRuntimeManager::evict_orphan_runtimes(self).await
    }
}
