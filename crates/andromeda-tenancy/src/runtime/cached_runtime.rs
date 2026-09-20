//! One tenant's live runtime, when it was last reached for, and the hold a
//! request takes against its eviction.

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
    /// Milliseconds since [`EPOCH`] at the last use.
    pub(super) last_used: AtomicU64,
    /// The eviction hold's origin.
    ///
    /// A request cloning this into a [`RuntimeLease`](super::lease::RuntimeLease)
    /// raises the strong count above one; every eviction path treats a count
    /// above one as "in use" and spares the runtime, so a request holding this
    /// runtime's state is never handed a pool eviction closed underneath it.
    pub(super) origin: Arc<()>,
}

impl<S, R> CachedRuntime<S, R> {
    /// Build a freshly leased runtime and the lease held against it.
    ///
    /// The origin is created here so the caller's lease and this runtime's
    /// field are the first two owners: the runtime is leased from the instant
    /// it enters the cache, before any concurrent trim can observe it.
    pub(super) fn newly_leased(state: Arc<S>, runtime: R) -> (CachedRuntime<S, R>, Arc<()>) {
        let origin = Arc::new(());
        // clone: the lease is a second owner of the origin by construction; the
        // cache field and the returned handle are the two owners that make the
        // strong count read two while a request holds it.
        let leased = Arc::clone(&origin);
        (
            CachedRuntime {
                state,
                runtime,
                last_used: AtomicU64::new(now_millis()),
                origin,
            },
            leased,
        )
    }

    /// Record that this runtime was just used.
    pub(super) fn touch(&self) {
        self.last_used.store(now_millis(), Ordering::Relaxed);
    }

    /// When this runtime was last used.
    pub(super) fn last_used_instant(&self) -> Instant {
        *EPOCH + Duration::from_millis(self.last_used.load(Ordering::Relaxed))
    }

    /// Whether a request currently holds a lease on this runtime.
    ///
    /// True while any [`RuntimeLease`](super::lease::RuntimeLease) cloned from
    /// [`origin`](Self::origin) is alive, which is the whole span of every
    /// request that resolved this runtime.
    pub(super) fn is_leased(&self) -> bool {
        Arc::strong_count(&self.origin) > 1
    }
}
