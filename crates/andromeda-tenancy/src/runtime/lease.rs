//! A hold that keeps a cached tenant runtime resident for the life of a
//! request.

use std::sync::Arc;

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
#[derive(Clone)]
pub struct RuntimeLease {
    /// The cloned eviction origin whose strong count keeps the runtime resident.
    ///
    /// Held purely for its ownership: nothing reads it, and it is the
    /// [`CachedRuntime`](super::cached_runtime::CachedRuntime)'s own clone of the
    /// same origin whose strong count the eviction paths inspect. Dropping this
    /// lets that count fall and the runtime become evictable.
    _hold: Arc<()>,
}

impl RuntimeLease {
    /// Take ownership of an already-cloned eviction origin as a lease.
    ///
    /// Used on the cold-build path, where the origin was cloned as the runtime
    /// entered the cache so the runtime is leased before any concurrent trim
    /// can observe it.
    pub(super) const fn hold(origin: Arc<()>) -> RuntimeLease {
        RuntimeLease { _hold: origin }
    }

    /// Clone a lease from a cached runtime's eviction origin.
    ///
    /// Used on the cache-hit path, under the shard guard that also reads the
    /// state, so the strong count eviction observes already reflects this hold.
    pub(super) fn clone_from(origin: &Arc<()>) -> RuntimeLease {
        // clone: a lease is by definition an additional owner of the origin; its
        // whole purpose is to raise the strong count the eviction paths read.
        RuntimeLease { _hold: Arc::clone(origin) }
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
