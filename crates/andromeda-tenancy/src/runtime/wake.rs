//! Waking a dormant tenant to run the background work its probe finds due, and
//! recording when each tenant is next to be woken.

use crate::{
    database::TenantDatabase,
    pending_work::{PendingWork, PendingWorkError},
    registered_tenant::RegisteredTenant,
    runtime::{TenantRuntimeManager, cached_runtime::CachedRuntime},
    state_factory::{TenantRuntime, TenantStateFactory},
    status::TenantStatus,
    tenant_existence::TenantExistence,
    tenant_ref::TenantRef,
};
use andromeda_ngsi_ld_lexicon::{tenant::TenantName, tenant_database_name::TenantDatabaseName};
use andromeda_telemetry::tenancy::{WakeOutcome, record_wake};
use dashmap::mapref::entry::Entry;
use std::time::SystemTime;
use tracing::{debug, warn};

/// Whether a woken runtime entered the cache.
enum WokenInsertion<R> {
    /// It is cached.
    Inserted,
    /// A request cached the tenant first; this runtime is to be shut down.
    Lost(R),
}

/// What a dormant tenant owes, once every answer that leaves nothing to load
/// has settled its schedule.
enum WakeReading {
    /// Its work falls due at `due`, which may be past or ahead.
    Due {
        /// The database its data lives in.
        database: TenantDatabaseName,
        /// Its standing in the registry.
        existence: TenantExistence,
        /// The earliest instant its probe found work due.
        due: SystemTime,
    },
    /// An established tenant whose schema is behind this broker's: building
    /// its runtime migrates it.
    Unmigrated {
        /// The database its data lives in.
        database: TenantDatabaseName,
        /// Its standing in the registry.
        existence: TenantExistence,
    },
    /// Nothing to load: the schedule is settled and the wake resolved so.
    Settled(WakeOutcome),
}

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
        match outcome {
            WakeOutcome::AlreadyCached | WakeOutcome::Loaded | WakeOutcome::NothingDue | WakeOutcome::Unregistered => self.backlog.served(tenant),
            WakeOutcome::Deferred | WakeOutcome::BuildFailed => {}
        }
        outcome
    }

    /// The wake decision, without the metric the public [`wake`](Self::wake)
    /// records around it.
    async fn wake_inner(&self, tenant: &TenantName) -> WakeOutcome {
        let tenant_ref = TenantRef::Named(tenant.clone()); // clone: the ref outlives this call
        let now = SystemTime::now();
        // A source due at once reports the database's clock at the probe's
        // statement, which is later than `now`, so the deadline is compared
        // with the instant the answer arrived.
        match self.read_wake(tenant, &tenant_ref, now).await {
            WakeReading::Due { database, existence, due } if due <= SystemTime::now() => self.load_woken(&tenant_ref, database, existence, tenant, due).await,
            // Not due yet: woken again at its due instant. The hold-off after
            // an eviction was applied when the eviction recorded the wake; a
            // re-probe must not push a wake already recorded past the instant
            // its work falls due.
            WakeReading::Due { due, .. } => {
                self.settle_schedule(tenant, now, Some(due)).await;
                WakeOutcome::NothingDue
            }
            // Building the runtime migrates the database, and the runtime then
            // runs whatever its obligations hold due.
            WakeReading::Unmigrated { database, existence } => self.load_woken(&tenant_ref, database, existence, tenant, now).await,
            WakeReading::Settled(outcome) => outcome,
        }
    }

    /// Re-probe a dormant tenant and record the instant its work falls due,
    /// without loading it.
    ///
    /// The reconciliation sweep's backstop: a wake a crashed replica recorded
    /// but never fired, or one an eviction-time probe misread, is put back on
    /// the schedule at its own instant. The wake pass then loads it in turn,
    /// the longest-overdue first, as the cache has room; the alarm the record
    /// rings wakes the waker at once when it sleeps past that instant. Loading
    /// it here instead would probe it and defer it whenever the cache is full.
    pub async fn reconcile(&self, tenant: &TenantName) {
        let tenant_ref = TenantRef::Named(tenant.clone()); // clone: the ref outlives this call
        let now = SystemTime::now();
        match self.read_wake(tenant, &tenant_ref, now).await {
            WakeReading::Due { due, .. } => self.settle_schedule(tenant, now, Some(due)).await,
            WakeReading::Unmigrated { .. } => self.settle_schedule(tenant, now, Some(now)).await,
            WakeReading::Settled(_) => {}
        }
        // The schedule now holds whatever this process had deferred.
        self.backlog.served(tenant);
    }

    /// Read what a dormant tenant owes: its registry row, then its probe,
    /// settling the schedule for every answer that leaves nothing to load.
    async fn read_wake(&self, tenant: &TenantName, tenant_ref: &TenantRef, now: SystemTime) -> WakeReading {
        // A cached tenant runs its own drains; clear its schedule without
        // touching its last use, so a wake never keeps it artificially resident.
        if self.runtimes.contains_key(tenant_ref) {
            self.settle_schedule(tenant, now, None).await;
            return WakeReading::Settled(WakeOutcome::AlreadyCached);
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
                    return WakeReading::Settled(WakeOutcome::NothingDue);
                }
                TenantStatus::Active => (record.database, TenantExistence::Established),
            },
            Ok(Some(RegisteredTenant::Provisional { database })) => (database, TenantExistence::Provisional),
            Ok(None) => return WakeReading::Settled(WakeOutcome::Unregistered),
            // A registry read failure is transient; leave the leased claim to
            // re-fire and report nothing loaded.
            Err(_) => return WakeReading::Settled(WakeOutcome::NothingDue),
        };

        match self.probe.earliest_pending_work(&database, existence).await {
            Ok(PendingWork::DueAt(due)) => WakeReading::Due { database, existence, due },
            // Nothing is due, or the tenant's database is gone (deprovisioned
            // under a stale schedule row). Both clear the schedule: a gone tenant
            // must not be re-armed, or it would churn the wake pass every
            // dormancy interval and starve the tenants that really have work.
            Ok(PendingWork::None) | Err(PendingWorkError::DatabaseGone { .. }) => {
                self.settle_schedule(tenant, now, None).await;
                WakeReading::Settled(WakeOutcome::NothingDue)
            }
            Err(PendingWorkError::Unmigrated { .. }) => match existence {
                // Its provisioning is still migrating the database, or was
                // interrupted, which the janitor retires. A creating operation
                // writes only once the migration completes, and the runtime it
                // runs on records the tenant's next wake when evicted.
                TenantExistence::Provisional => {
                    self.settle_schedule(tenant, now, None).await;
                    WakeReading::Settled(WakeOutcome::NothingDue)
                }
                TenantExistence::Established => WakeReading::Unmigrated { database, existence },
            },
            // Transient: the probe could not read this pass. Re-arm a retry
            // rather than treating the tenant as having nothing due.
            Err(PendingWorkError::Saturated { .. } | PendingWorkError::ProbeFailed { .. }) => {
                self.settle_schedule(tenant, now, Some(self.waker.retry_wake(now))).await;
                WakeReading::Settled(WakeOutcome::NothingDue)
            }
        }
    }

    /// Build and cache a woken tenant's runtime, displacing the
    /// least-recently-used runtime when the cache is full.
    ///
    /// A tenant reaches here only with work already due (see
    /// [`read_wake`](Self::read_wake)), so it must run rather than defer behind
    /// runtimes that may have no work at all. When the cache is at capacity it
    /// evicts one unleased runtime (the least recently used, whose own next wake
    /// is rescheduled as it goes) so the connection budget stays flat while the
    /// due work is admitted. Only genuine backpressure, every cached runtime held
    /// by an in-flight request or within its woken lifetime with its work still
    /// outstanding so no slot can be freed, defers the wake, until the first of
    /// those places may free. A deferred wake is overdue since `due`, the
    /// instant its work fell due. A wake pass claims only as many tenants as it
    /// counted room for, so a wake is deferred only when requests took that room
    /// first.
    async fn load_woken(
        &self,
        tenant_ref: &TenantRef,
        database: TenantDatabaseName,
        existence: TenantExistence,
        tenant: &TenantName,
        due: SystemTime,
    ) -> WakeOutcome {
        let now = SystemTime::now();
        if self.runtimes.len() >= self.config.residency.max_resident.get() && !self.make_place_for_woken(tenant_ref).await {
            let retry = self.waker.deferred_wake(now, self.room_for_wake_in());
            debug!(
                tenant = tenant.as_str(),
                cached = self.runtimes.len(),
                room = ?self.wake_room(),
                "tenant wake deferred: no loaded tenant could be unloaded to make room"
            );
            self.settle_schedule(tenant, now, Some(retry)).await;
            self.backlog.deferred(tenant, due, retry);
            return WakeOutcome::Deferred;
        }
        self.build_woken(tenant_ref, database, existence, tenant).await
    }

    /// Unload one runtime to make a place for the woken `tenant_ref`, returning
    /// whether a place was made.
    ///
    /// A request may have taken the place the wake pass counted on; a woken
    /// runtime whose work has run then gives up its own place before the wake
    /// is deferred.
    async fn make_place_for_woken(&self, tenant_ref: &TenantRef) -> bool {
        self.evict_one_for_wake(tenant_ref).await || (self.release_drained_woken(1).await > 0 && self.evict_one_for_wake(tenant_ref).await)
    }

    /// Build a woken tenant's runtime and cache it as the first idle-eviction
    /// victim, settling its wake schedule with the result.
    ///
    /// A build that fails re-arms a retry one `min_dormancy` out, so the
    /// tenant is retried by its wake schedule rather than forgotten.
    pub(super) async fn build_woken(
        &self,
        tenant_ref: &TenantRef,
        database: TenantDatabaseName,
        existence: TenantExistence,
        tenant: &TenantName,
    ) -> WakeOutcome {
        let now = SystemTime::now();
        // Load under the same admission bound as a request cold build.
        let permit = self.build_permits.acquire().await.ok();
        let built = self.factory.build(tenant_ref, &database).await;
        drop(permit);

        let Ok((state, runtime)) = built else {
            self.settle_schedule(tenant, now, Some(self.waker.retry_wake(now))).await;
            return WakeOutcome::BuildFailed;
        };

        // The entry is decided and its shard guard dropped before anything is
        // awaited: a guard held across an await blocks every thread that
        // reaches the shard, and once all of them do, every worker of the
        // runtime.
        let insertion = match self.runtimes.entry(tenant_ref.clone()) {
            Entry::Occupied(_) => WokenInsertion::Lost(runtime),
            Entry::Vacant(vacant) => {
                let cached = {
                    let origin = self.new_origin(&runtime);
                    let due_work = runtime.due_work();
                    CachedRuntime::woken(state, runtime, database, existence, origin, self.waker.wake_residency(), due_work)
                };
                cached.backdate(self.waker.woken_backdate());
                drop(vacant.insert(cached));
                WokenInsertion::Inserted
            }
        };
        match insertion {
            // A request cold-loaded the tenant while we built; discard ours.
            WokenInsertion::Lost(runtime) => {
                runtime.shutdown().await;
                self.settle_schedule(tenant, now, None).await;
                WakeOutcome::AlreadyCached
            }
            WokenInsertion::Inserted => {
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
        match self.schedule.record(tenant, probed_at, wake_at).await {
            Ok(()) => self.raise_alarm(wake_at),
            Err(error) => warn!(tenant = tenant.as_str(), error = %error, "failed to record the tenant wake schedule"),
        }
    }

    /// Settle a wake attempt's next schedule, logging a write failure rather
    /// than propagating it.
    async fn settle_schedule(&self, tenant: &TenantName, probed_at: SystemTime, wake_at: Option<SystemTime>) {
        match self.schedule.settle(tenant, probed_at, wake_at).await {
            Ok(()) => self.raise_alarm(wake_at),
            Err(error) => warn!(tenant = tenant.as_str(), error = %error, "failed to settle the tenant wake schedule"),
        }
    }

    /// Tell the waker of a wake this process just recorded.
    fn raise_alarm(&self, wake_at: Option<SystemTime>) {
        if let Some(wake_at) = wake_at {
            self.alarm.recorded(wake_at);
        }
    }
}
