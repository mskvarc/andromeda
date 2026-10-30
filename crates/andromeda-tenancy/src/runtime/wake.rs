//! Waking a dormant tenant to run the background work its probe finds due, and
//! recording when each tenant is next to be woken.

use crate::{
    database::TenantDatabase,
    database_name::TenantDatabaseName,
    pending_work::{PendingWork, PendingWorkError},
    registered_tenant::RegisteredTenant,
    runtime::{
        TenantRuntimeManager,
        cached_runtime::{CachedRuntime, RuntimeExistence},
    },
    state_factory::{TenantRuntime, TenantStateFactory},
    status::TenantStatus,
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use andromeda_telemetry::tenancy::{WakeOutcome, record_wake};
use dashmap::mapref::entry::Entry;
use std::time::SystemTime;
use tracing::warn;

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
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

        // A provisional tenant is woken like an established one: a creating
        // operation may have committed work to its database before any request
        // established it, and that work falls due regardless.
        let (database, existence) = match self.database.registered_tenant(tenant).await {
            Ok(Some(RegisteredTenant::Established(record))) => match record.status {
                // A suspended tenant is never woken: clear its schedule so it is
                // not re-claimed until it is resumed and next accessed.
                TenantStatus::Suspended => {
                    self.settle_schedule(tenant, now, None).await;
                    return WakeOutcome::NothingDue;
                }
                TenantStatus::Active => (record.database, RuntimeExistence::Established),
            },
            Ok(Some(RegisteredTenant::Provisional { database })) => (database, RuntimeExistence::Provisional),
            Ok(None) => return WakeOutcome::Unregistered,
            // A registry read failure is transient; leave the leased claim to
            // re-fire and report nothing loaded.
            Err(_) => return WakeOutcome::NothingDue,
        };

        // A source due at once reports the database's clock at the probe's
        // statement, which is later than `now`, so the deadline is compared
        // with the instant the answer arrived.
        match self.probe.earliest_pending_work(&database).await {
            Ok(PendingWork::DueAt(due)) if due <= SystemTime::now() => self.load_woken(&tenant_ref, database, existence, tenant).await,
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
            Err(PendingWorkError::Unmigrated { .. }) => match existence {
                // Its provisioning is still migrating the database, or was
                // interrupted, which the janitor retires. A creating operation
                // writes only once the migration completes, and the runtime it
                // runs on records the tenant's next wake when evicted.
                RuntimeExistence::Provisional => {
                    self.settle_schedule(tenant, now, None).await;
                    WakeOutcome::NothingDue
                }
                // Building the runtime migrates the database, and the runtime
                // then runs whatever its obligations hold due.
                RuntimeExistence::Established => self.load_woken(&tenant_ref, database, existence, tenant).await,
            },
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
    /// evicts one unleased runtime (the least recently used, whose own next wake
    /// is rescheduled as it goes) so the connection budget stays flat while the
    /// due work is admitted. Only genuine backpressure, every cached runtime held
    /// by an in-flight request so no slot can be freed, defers the wake.
    async fn load_woken(&self, tenant_ref: &TenantRef, database: TenantDatabaseName, existence: RuntimeExistence, tenant: &TenantName) -> WakeOutcome {
        let now = SystemTime::now();
        if self.runtimes.len() >= self.config.residency.max_resident.get() && !self.evict_one_for_wake(tenant_ref).await {
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
                let cached = {
                    let origin = self.new_origin(&runtime);
                    CachedRuntime::woken(state, runtime, database, existence, origin, self.waker.wake_residency())
                };
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
}
