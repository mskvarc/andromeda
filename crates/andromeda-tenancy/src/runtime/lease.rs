//! A hold that keeps a cached tenant runtime resident for the life of a
//! request.

use crate::state_factory::RuntimeActivity;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;

/// What every lease on one cached runtime shares: the strong count the eviction
/// paths read, and whether the runtime has been retired while leased.
///
/// A runtime is retired when its tenant is suspended while a request still holds
/// it. Eviction spares a leased runtime, so the suspend cannot release it at
/// once; retiring it instead makes admission refuse it and makes the last lease
/// to drop wake the janitor, which then releases it. Without that, a suspended
/// tenant's runtime would stay cached -- and keep running its background work --
/// until it next idled out, contradicting the promise in
/// `docs/tenant-management.md` that the runtime drops when the request's hold
/// releases.
pub(super) struct LeaseOrigin {
    /// Set once the runtime's tenant is suspended while the runtime is leased.
    retired: AtomicBool,
    /// Woken when a lease on a retired runtime is released, so the janitor
    /// evicts it without waiting for its next tick.
    release_signal: Arc<Notify>,
    /// Where the runtime hears of use: every lease reports when it is taken and
    /// when it is released.
    activity: Arc<dyn RuntimeActivity>,
}

impl LeaseOrigin {
    /// A fresh, serving origin whose retired releases wake `release_signal` and
    /// whose leases report to `activity`.
    pub(super) fn new(release_signal: Arc<Notify>, activity: Arc<dyn RuntimeActivity>) -> Arc<LeaseOrigin> {
        Arc::new(LeaseOrigin {
            retired: AtomicBool::new(false),
            release_signal,
            activity,
        })
    }

    /// Report a use of the runtime: a request reached for it or let it go.
    pub(super) fn record_activity(&self) {
        self.activity.record_activity();
    }

    /// Whether the runtime has been retired and must no longer be handed out.
    pub(super) fn is_retired(&self) -> bool {
        self.retired.load(Ordering::Acquire)
    }

    /// Retire the runtime: admission refuses it from now on and the last lease
    /// to drop wakes the janitor to release it.
    pub(super) fn retire(&self) {
        self.retired.store(true, Ordering::Release);
    }

    /// Return a retired runtime to service, for a tenant resumed before the
    /// runtime's last lease dropped.
    pub(super) fn reinstate(&self) {
        self.retired.store(false, Ordering::Release);
    }
}

/// A hold on a cached tenant runtime that spares it from eviction.
///
/// While at least one lease on a runtime is alive, that runtime's eviction
/// origin has a strong count above one, and every eviction path -- the idle
/// sweep, the excess trim on the janitor tick, and the synchronous trim a cold
/// build runs on insert -- skips it. A lease is taken under the cache shard
/// guard at resolve time and rides inside the per-request capability set to the
/// end of the request, so a request holding a runtime's state is never handed a
/// connection pool that eviction closed underneath it.
///
/// Cloning a lease adds another hold; the runtime becomes evictable only once
/// every clone has dropped and the origin's strong count falls back to one.
/// Dropping a lease on a retired runtime wakes the janitor, so the runtime of a
/// tenant suspended mid-request is released as soon as its last hold goes.
///
/// Dropping any lease reports a use to the runtime: the request holding it has
/// committed whatever it wrote by the time it lets the runtime go.
#[derive(Clone)]
pub struct RuntimeLease {
    /// The cloned eviction origin whose strong count keeps the runtime resident.
    ///
    /// Always `Some` while the lease is alive; taken only by `Drop`, so the hold
    /// is released before the janitor is woken and the janitor never observes a
    /// strong count this lease still inflates.
    hold: Option<Arc<LeaseOrigin>>,
}

impl RuntimeLease {
    /// Take ownership of an already-cloned eviction origin as a lease.
    ///
    /// Used on the cold-build path, where the origin was cloned as the runtime
    /// entered the cache so the runtime is leased before any concurrent trim
    /// can observe it.
    pub(super) const fn hold(origin: Arc<LeaseOrigin>) -> RuntimeLease {
        RuntimeLease { hold: Some(origin) }
    }

    /// Clone a lease from a cached runtime's eviction origin.
    ///
    /// Used on the cache-hit path, under the shard guard that also reads the
    /// state, so the strong count eviction observes already reflects this hold.
    pub(super) fn clone_from(origin: &Arc<LeaseOrigin>) -> RuntimeLease {
        // clone: a lease is by definition an additional owner of the origin; its
        // whole purpose is to raise the strong count the eviction paths read.
        RuntimeLease {
            hold: Some(Arc::clone(origin)),
        }
    }
}

impl Drop for RuntimeLease {
    fn drop(&mut self) {
        let Some(origin) = self.hold.take() else {
            return;
        };
        origin.record_activity();
        if origin.is_retired() {
            // clone: the signal must outlive the hold, which is released first so
            // the woken janitor reads the lowered strong count.
            let signal = Arc::clone(&origin.release_signal);
            drop(origin);
            signal.notify_one();
        }
    }
}

/// One tenant's state together with the lease that keeps its runtime resident.
///
/// [`TenantRuntimeManager::resolve_state`](super::TenantRuntimeManager::resolve_state)
/// hands this back so the caller holds the lease for exactly as long as it
/// holds the state. Dropping it releases the hold, and the runtime becomes
/// eligible for eviction on the next sweep or insert-trim.
pub struct LeasedState<S> {
    /// The per-tenant state a request handler reads.
    pub state: Arc<S>,
    /// The eviction hold that keeps `state`'s runtime resident.
    pub lease: RuntimeLease,
}
