//! One tenant's live runtime, when it was last reached for, and the hold a
//! request takes against its eviction.

use crate::{database_name::TenantDatabaseName, runtime::lease::LeaseOrigin, state_factory::TenantRuntime};
use std::{
    sync::{
        Arc,
        LazyLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// The instant every last-used stamp is measured from.
///
/// `Instant` is not `Copy`-into-an-atomic, and a runtime's last use is written
/// on every request. Storing an offset in milliseconds from a process-wide
/// epoch makes the write a relaxed atomic store rather than a lock.
pub(super) static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Milliseconds elapsed since the process epoch.
pub(super) fn now_millis() -> u64 {
    // `Duration::as_millis` answers in `u128`. The whole-second and sub-second
    // halves are each `u64`-representable on their own, so the stamp is
    // assembled from them rather than narrowed from a wider integer.
    let elapsed = EPOCH.elapsed();
    elapsed.as_secs() * 1_000 + u64::from(elapsed.subsec_millis())
}

/// One tenant's live runtime, and when it was last reached for.
pub(super) struct CachedRuntime<S, R> {
    /// The state handlers read.
    pub(super) state: Arc<S>,
    /// The resources to release on eviction.
    pub(super) runtime: R,
    /// The database this tenant's data lives in, kept so the janitor can probe
    /// it for due work after evicting the runtime.
    database: TenantDatabaseName,
    /// Milliseconds since [`EPOCH`] at the last use.
    pub(super) last_used: AtomicU64,
    /// The eviction hold's origin.
    ///
    /// A request cloning this into a [`RuntimeLease`](super::lease::RuntimeLease)
    /// raises the strong count above one; every eviction path treats a count
    /// above one as "in use" and spares the runtime, so a request holding this
    /// runtime's state is never handed a pool eviction closed underneath it.
    pub(super) origin: Arc<LeaseOrigin>,
}

impl<S, R: TenantRuntime> CachedRuntime<S, R> {
    /// Record that this runtime was just used, and tell the runtime so.
    pub(super) fn touch(&self) {
        self.last_used.store(now_millis(), Ordering::Relaxed);
        self.runtime.record_activity();
    }
}

impl<S, R> CachedRuntime<S, R> {
    /// Build a freshly leased runtime and the lease held against it.
    ///
    /// `origin` is fresh, so the caller's lease and this runtime's field are its
    /// first two owners: the runtime is leased from the instant it enters the
    /// cache, before any concurrent trim can observe it.
    pub(super) fn newly_leased(state: Arc<S>, runtime: R, database: TenantDatabaseName, origin: Arc<LeaseOrigin>) -> (CachedRuntime<S, R>, Arc<LeaseOrigin>) {
        // clone: the lease is a second owner of the origin by construction; the
        // cache field and the returned handle are the two owners that make the
        // strong count read two while a request holds it.
        let leased = Arc::clone(&origin);
        (
            CachedRuntime {
                state,
                runtime,
                database,
                last_used: AtomicU64::new(now_millis()),
                origin,
            },
            leased,
        )
    }

    /// Build a runtime that no request holds, for the waker to insert a dormant
    /// tenant it woke to run due work.
    ///
    /// Unleased from the start, so a sweep may evict it once it has idled past
    /// its residency; the waker backdates its last use so it is the first idle
    /// victim and never displaces a request-touched runtime.
    pub(super) fn unleased(state: Arc<S>, runtime: R, database: TenantDatabaseName, origin: Arc<LeaseOrigin>) -> CachedRuntime<S, R> {
        CachedRuntime {
            state,
            runtime,
            database,
            last_used: AtomicU64::new(now_millis()),
            origin,
        }
    }

    /// Age this runtime's last use by `age`, so it sorts older for eviction.
    ///
    /// A woken runtime is backdated by the idle TTL minus its residency, leaving
    /// it exactly the residency of idle life before a sweep reclaims it.
    pub(super) fn backdate(&self, age: Duration) {
        let age_millis = u64::try_from(age.as_millis()).unwrap_or(u64::MAX);
        self.last_used.store(now_millis().saturating_sub(age_millis), Ordering::Relaxed);
    }

    /// When this runtime was last used.
    pub(super) fn last_used_instant(&self) -> Instant {
        *EPOCH + Duration::from_millis(self.last_used.load(Ordering::Relaxed))
    }

    /// The database this tenant's data lives in.
    pub(super) const fn database(&self) -> &TenantDatabaseName {
        &self.database
    }

    /// Whether a request currently holds a lease on this runtime.
    ///
    /// True while any [`RuntimeLease`](super::lease::RuntimeLease) cloned from
    /// [`origin`](Self::origin) is alive, which is the whole span of every
    /// request that resolved this runtime.
    pub(super) fn is_leased(&self) -> bool {
        Arc::strong_count(&self.origin) > 1
    }

    /// Whether this runtime was retired by a suspend while leased, and must no
    /// longer be handed to a request.
    pub(super) fn is_retired(&self) -> bool {
        self.origin.is_retired()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        database_name::TenantDatabaseName,
        runtime::{cached_runtime::CachedRuntime, lease::LeaseOrigin},
        state_factory::TenantRuntime,
    };
    use async_trait::async_trait;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::sync::Notify;

    /// A runtime counting how often it is told it was reached for.
    struct ActivityCountingRuntime {
        activity: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl TenantRuntime for ActivityCountingRuntime {
        async fn shutdown(self) {}

        fn record_activity(&self) {
            self.activity.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Pacing background work by use needs every touch to reach the runtime,
    /// not only the cache's own last-use stamp.
    #[test]
    fn every_touch_tells_the_runtime_it_was_reached_for() {
        let activity = Arc::new(AtomicUsize::new(0));
        let cached = CachedRuntime::unleased(
            Arc::new(()),
            ActivityCountingRuntime {
                activity: Arc::clone(&activity),
            },
            TenantDatabaseName::from("andromeda_tenant_acme".to_string()),
            LeaseOrigin::new(Arc::new(Notify::new())),
        );
        cached.touch();
        cached.touch();
        pretty_assertions::assert_eq!(activity.load(Ordering::Relaxed), 2);
    }
}
