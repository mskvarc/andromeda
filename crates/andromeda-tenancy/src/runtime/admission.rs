//! Admitting a request to a tenant's runtime: the cache hit, the single-flight
//! cold build, and the deadline every wait ahead of that build answers to.

use crate::{
    creation_policy::TenantCreationPolicy,
    database::TenantDatabase,
    database_name::TenantDatabaseName,
    error::TenancyError,
    provisional::Establishment,
    registered_tenant::RegisteredTenant,
    runtime::{
        TenantRuntimeManager,
        cached_runtime::{CachedRuntime, RuntimeExistence},
        lease::{LeasedState, RuntimeLease},
    },
    state_factory::{TenantRuntime, TenantStateFactory},
    status::TenantStatus,
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_telemetry::tenancy::{ColdBuildOutcome, record_build_admission_wait, record_cold_build};
use dashmap::mapref::entry::Entry;
use std::{sync::Arc, time::Instant as StdInstant};
use tokio::{
    sync::Mutex,
    time::{Instant, timeout_at},
};

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
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
    /// `policy` is [`TenantCreationPolicy::MustExist`], when it is provisional
    /// under that policy and its database holds no information, or when it is
    /// suspended (including a cached runtime a suspend retired while a request
    /// held it).
    /// Returns [`TenancyError::Saturated`] when the request cannot be admitted
    /// to a cold build within `build_admission_timeout`. Reading the registry,
    /// provisioning the tenant and building its runtime each surface the
    /// failure the store or the factory reported.
    pub async fn resolve_state(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<LeasedState<F::State>, TenancyError> {
        if !self.config.is_enabled() && !tenant.is_unnamed_default() {
            return Err(TenancyError::MultiTenancyDisabled);
        }

        if let Some(cached) = self.cached_lease(tenant)? {
            return self.admit_cached(tenant, policy, cached).await;
        }

        // Every wait ahead of the build (the single-flight lock, the build
        // permit and a teardown slot to make room) answers to one deadline, so
        // an overloaded broker sheds the request as retryable backpressure before
        // the HTTP request deadline instead of stalling into it.
        let deadline = Instant::now() + self.config.build.admission_timeout.duration();

        // Single-flight the cold build: concurrent first requests for one tenant
        // share its build lock, so exactly one opens a runtime. The rest wait,
        // then re-check `runtimes` under the lock and reuse the one it inserted
        // instead of each opening a pool that would be thrown away. A waiter that
        // gives up at the deadline leaves the lock in place for the request
        // still building, so a later request cannot start a second build.
        let build_lock = self.build_lock_for(tenant);
        let Ok(guard) = timeout_at(deadline, build_lock.lock()).await else {
            return Err(saturated(tenant));
        };
        let result = self.resolve_state_locked(tenant, policy, deadline).await;

        // Drop our build lock now the build is over. The pointer check removes
        // only this lock, never one a later request created after we released
        // it, so the map stays bounded without detaching a lock two requests
        // still share.
        self.build_locks.remove_if(tenant, |_, existing| Arc::ptr_eq(existing, &build_lock));
        drop(guard);

        result
    }

    /// The cached runtime's state and a fresh lease on it, with whether its
    /// tenant exists yet, if one is cached.
    ///
    /// A runtime retired by a suspend while a request held it is refused exactly
    /// as the cold path refuses a suspended tenant (clause 5.5.10): the data path
    /// answers as if the tenant did not exist.
    fn cached_lease(&self, tenant: &TenantRef) -> Result<Option<CachedLease<F::State>>, TenancyError> {
        let Some(runtime) = self.runtimes.get(tenant) else {
            return Ok(None);
        };
        if runtime.is_retired()
            && let TenantRef::Named(name) = tenant
        {
            return Err(unknown(name));
        }
        runtime.touch();
        // The lease is cloned under the shard read guard that also reads the
        // state, so the strong count a concurrent eviction reads already counts
        // this hold.
        Ok(Some(CachedLease {
            leased: LeasedState {
                state: Arc::clone(&runtime.state),
                lease: RuntimeLease::clone_from(&runtime.origin),
            },
            existence: runtime.existence(),
        }))
    }

    /// Hand a request the cached runtime it found, once its tenant is known to
    /// exist when `policy` requires that.
    ///
    /// A provisional tenant is served to a request that may create it. A
    /// request that requires the tenant to exist is served only once the
    /// tenant's database holds information (clause 5.5.10); the lease is held
    /// while that is read, so the runtime cannot be released underneath it.
    async fn admit_cached(
        &self,
        tenant: &TenantRef,
        policy: TenantCreationPolicy,
        CachedLease { leased, existence }: CachedLease<F::State>,
    ) -> Result<LeasedState<F::State>, TenancyError> {
        if let (RuntimeExistence::Provisional, TenantCreationPolicy::MustExist, TenantRef::Named(name)) = (existence, policy, tenant) {
            self.require_established(name).await?;
        }
        Ok(leased)
    }

    /// Refuse a provisional tenant whose database holds no information.
    async fn require_established(&self, name: &TenantName) -> Result<(), TenancyError> {
        match self.establish_tenant(name).await? {
            Establishment::Established => Ok(()),
            Establishment::Empty => Err(unknown(name)),
        }
    }

    /// The cold path under the tenant's single-flight lock: re-check the cache,
    /// take a build permit and build.
    async fn resolve_state_locked(&self, tenant: &TenantRef, policy: TenantCreationPolicy, deadline: Instant) -> Result<LeasedState<F::State>, TenancyError> {
        if let Some(cached) = self.cached_lease(tenant)? {
            return self.admit_cached(tenant, policy, cached).await;
        }

        // Admission: only a genuine cold build past the second cache check waits
        // here, so the number of runtimes being built at once, and with it the
        // direct migrator connections and pools they open, is bounded.
        let admission_started = StdInstant::now();
        let permit = timeout_at(deadline, self.build_permits.acquire()).await;
        record_build_admission_wait(admission_started.elapsed().as_secs_f64());
        let permit = permit.map_err(|_| saturated(tenant))?.ok();

        let build_started = StdInstant::now();
        let result = self.resolve_state_cold(tenant, policy, deadline).await;
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

        // Bring the cache back inside runtime_max_cached if concurrent builds
        // overshot it. The freshly built runtime is leased and protected, so it
        // is never itself the victim.
        if result.is_ok() {
            self.detach_trim_excess_runtimes(tenant).await;
        }
        result
    }

    /// The build lock for a tenant, creating it on the first request.
    pub(super) fn build_lock_for(&self, tenant: &TenantRef) -> Arc<Mutex<()>> {
        // clone: hand back an owned Arc so the DashMap shard guard is released
        // before the caller awaits on the lock: holding a shard guard across
        // an await deadlocks.
        Arc::clone(self.build_locks.entry(tenant.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).value())
    }

    /// Build a runtime for a tenant that has none open, returning it leased.
    async fn resolve_state_cold(&self, tenant: &TenantRef, policy: TenantCreationPolicy, deadline: Instant) -> Result<LeasedState<F::State>, TenancyError> {
        let (database, existence) = self.admitted_database(tenant, policy).await?;

        self.make_room(tenant, deadline).await?;
        let (state, runtime) = self.factory.build(tenant, &database).await?;

        // A concurrent caller may have won the race between the in-flight
        // release and this insert. The loser releases what it built rather
        // than replacing a runtime other requests are already holding.
        match self.runtimes.entry(tenant.clone()) {
            Entry::Occupied(entry) => {
                entry.get().touch();
                if existence == RuntimeExistence::Established {
                    entry.get().establish();
                }
                let state = Arc::clone(&entry.get().state);
                let lease = RuntimeLease::clone_from(&entry.get().origin);
                drop(entry);
                runtime.shutdown().await;
                Ok(LeasedState { state, lease })
            }
            Entry::Vacant(vacant) => {
                // The runtime is leased before it enters the cache, so the trim
                // any concurrent cold build runs, which sees this entry the
                // instant it is inserted, observes the lease and spares it.
                let (cached, leased) = {
                    let origin = self.new_origin(&runtime);
                    CachedRuntime::newly_leased(Arc::clone(&state), runtime, database, existence, origin)
                };
                drop(vacant.insert(cached));
                self.record_cache_size();
                Ok(LeasedState {
                    state,
                    lease: RuntimeLease::hold(leased),
                })
            }
        }
    }

    /// Where a tenant's data lives and whether it exists yet, for a request
    /// admitted under `policy`.
    ///
    /// A suspended tenant is refused on the data path exactly as an absent one
    /// is (clause 5.5.10): it is never cold-loaded, so a request bearing its
    /// `NGSILD-Tenant` header sees a 404 `NonexistentTenant`, and the suspension
    /// cannot be probed by whether a request succeeds. A provisional tenant is
    /// refused the same way under [`TenantCreationPolicy::MustExist`] until its
    /// database holds information. A request that may create the tenant
    /// provisions it, which also prepares the database of a provisional tenant
    /// whose provisioning was interrupted.
    async fn admitted_database(&self, tenant: &TenantRef, policy: TenantCreationPolicy) -> Result<(TenantDatabaseName, RuntimeExistence), TenancyError> {
        let TenantRef::Named(name) = tenant else {
            return Ok((self.database.default_database(), RuntimeExistence::Established));
        };
        match (self.database.registered_tenant(name).await?, policy) {
            (Some(RegisteredTenant::Established(record)), TenantCreationPolicy::MustExist | TenantCreationPolicy::CreateOnDemand) => match record.status {
                TenantStatus::Suspended => Err(unknown(name)),
                TenantStatus::Active => Ok((record.database, RuntimeExistence::Established)),
            },
            (Some(RegisteredTenant::Provisional { database }), TenantCreationPolicy::MustExist) => {
                self.require_established(name).await?;
                Ok((database, RuntimeExistence::Established))
            }
            (Some(RegisteredTenant::Provisional { .. }) | None, TenantCreationPolicy::CreateOnDemand) => {
                Ok((self.database.provision(name).await?, RuntimeExistence::Provisional))
            }
            (None, TenantCreationPolicy::MustExist) => Err(unknown(name)),
        }
    }

    /// Release least-recently-used runtimes until a new one fits inside
    /// `runtime_max_cached`, before it is built.
    ///
    /// Each release takes a teardown slot first, so when every slot is busy
    /// with a slow teardown the cold build waits for one, up to `deadline`
    /// and then is refused as saturated, rather than adding a runtime while the
    /// evicted ones pile up alive behind their teardowns. Only unleased named
    /// runtimes are released; when every cached runtime is leased the build
    /// proceeds and the cache overshoots by at most the requests in flight, as
    /// `runtime_max_cached` documents.
    async fn make_room(&self, tenant: &TenantRef, deadline: Instant) -> Result<(), TenancyError> {
        while self.runtimes.len() >= self.config.residency.max_resident.get() {
            let Some((victim, chosen_at)) = self.lru_unleased_named(tenant) else {
                return Ok(());
            };
            let permit = timeout_at(deadline, Arc::clone(&self.eviction_permits).acquire_owned())
                .await
                .map_err(|_| saturated(tenant))?
                .ok();
            self.evict_holding(permit, victim, chosen_at).await;
        }
        Ok(())
    }
}

/// A lease on a cached runtime, with whether its tenant exists yet.
struct CachedLease<S> {
    /// The runtime's state and the hold against its eviction.
    leased: LeasedState<S>,
    /// Whether the tenant the runtime serves was seen to exist.
    existence: RuntimeExistence,
}

/// The refusal for a named tenant the data path must answer as nonexistent:
/// absent or provisional and empty under [`TenantCreationPolicy::MustExist`],
/// suspended, or retired.
fn unknown(name: &TenantName) -> TenancyError {
    TenancyError::UnknownTenant { name: name.clone() } // clone: the error outlives this borrow
}

/// The backpressure refusal for a request that could not be admitted in time.
fn saturated(tenant: &TenantRef) -> TenancyError {
    TenancyError::Saturated { tenant: tenant.clone() } // clone: the error outlives this borrow
}
