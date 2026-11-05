//! One tenant's live runtime, when it was last reached for, and the hold a
//! request takes against its eviction.

use crate::{
    due_work::DueWorkSource,
    runtime::{
        eviction_rank::{Backlog, EvictionRank, PeriodicWork},
        lease::LeaseOrigin,
        woken_residency::WokenResidency,
    },
    state_factory::TenantRuntime,
    tenant_existence::TenantExistence,
};
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use std::{
    sync::{
        Arc,
        LazyLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
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
    /// Whether the tenant was seen to exist. Moves from provisional to
    /// established once and never back: an established tenant stays so until
    /// it is removed from the registry.
    established: AtomicBool,
    /// The place a runtime the waker loaded for due work holds until its work
    /// has run or its woken lifetime ends; `None` for a runtime a request
    /// loaded.
    woken: Option<Arc<WokenResidency>>,
}

impl<S, R> CachedRuntime<S, R> {
    /// Record that this runtime was just used, and tell the runtime so.
    pub(super) fn touch(&self) {
        self.last_used.store(now_millis(), Ordering::Relaxed);
        self.origin.record_activity();
    }

    /// Build a freshly leased runtime and the lease held against it.
    ///
    /// `origin` is fresh, so the caller's lease and this runtime's field are its
    /// first two owners: the runtime is leased from the instant it enters the
    /// cache, before any concurrent trim can observe it.
    pub(super) fn newly_leased(
        state: Arc<S>,
        runtime: R,
        database: TenantDatabaseName,
        existence: TenantExistence,
        origin: Arc<LeaseOrigin>,
    ) -> (CachedRuntime<S, R>, Arc<LeaseOrigin>) {
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
                established: AtomicBool::new(existence == TenantExistence::Established),
                woken: None,
            },
            leased,
        )
    }

    /// Build a runtime that no request holds, for the waker to insert a dormant
    /// tenant it woke to run due work.
    ///
    /// Unleased from the start, so a sweep may evict it once it has idled past
    /// its residency; the waker backdates its last use so it is the first idle
    /// victim and never displaces a request-touched runtime. For `residency`
    /// it is not unloaded to make room either, so the work it was woken for
    /// runs before anything can take it away again, unless it answers through
    /// `due_work` that the work has run.
    pub(super) fn woken(
        state: Arc<S>,
        runtime: R,
        database: TenantDatabaseName,
        existence: TenantExistence,
        origin: Arc<LeaseOrigin>,
        residency: Duration,
        due_work: Arc<dyn DueWorkSource>,
    ) -> CachedRuntime<S, R> {
        CachedRuntime {
            state,
            runtime,
            database,
            last_used: AtomicU64::new(now_millis()),
            origin,
            established: AtomicBool::new(existence == TenantExistence::Established),
            woken: WokenResidency::new(residency, due_work).map(Arc::new),
        }
    }

    /// Whether this runtime was loaded for due work less than its residency
    /// ago and has not answered that the work has run, and so is not unloaded
    /// to make room.
    pub(super) fn is_within_wake_residency(&self, now: Instant) -> bool {
        self.woken.as_ref().is_some_and(|woken| woken.holds_place(now))
    }

    /// The place this runtime holds as one the waker loaded, `None` for a
    /// runtime a request loaded.
    pub(super) const fn woken_residency(&self) -> Option<&Arc<WokenResidency>> {
        self.woken.as_ref()
    }

    /// Whether this runtime may be unloaded at `now` to make room: no request
    /// holds it and it is past any woken lifetime.
    pub(super) fn is_unloadable(&self, now: Instant) -> bool {
        !self.is_leased() && !self.is_within_wake_residency(now)
    }

    /// How long from `now` until this runtime's place may free: its woken
    /// lifetime ends, or it may next be asked whether its work has run. `None`
    /// when it holds no place as a woken runtime.
    pub(super) fn place_frees_in(&self, now: Instant) -> Option<Duration> {
        self.woken.as_ref().and_then(|woken| woken.frees_in(now))
    }

    /// Whether the tenant this runtime serves was seen to exist.
    pub(super) fn existence(&self) -> TenantExistence {
        if self.established.load(Ordering::Acquire) {
            TenantExistence::Established
        } else {
            TenantExistence::Provisional
        }
    }

    /// Record that the tenant this runtime serves exists.
    pub(super) fn establish(&self) {
        self.established.store(true, Ordering::Release);
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

impl<S, R: TenantRuntime> CachedRuntime<S, R> {
    /// Where this runtime stands in the order runtimes are unloaded to make
    /// room, given `horizon`, the instant before which an unloaded tenant is
    /// not woken again.
    pub(super) fn eviction_rank(&self, horizon: SystemTime) -> EvictionRank {
        EvictionRank {
            backlog: Backlog::of(self.runtime.undelivered_changes()),
            work: PeriodicWork::of(self.runtime.next_periodic_due(), horizon),
            last_used: self.last_used_instant(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        due_work::{DueWork, DueWorkSource},
        runtime::{
            cached_runtime::CachedRuntime,
            lease::{LeaseOrigin, RuntimeLease},
        },
        state_factory::RuntimeActivity,
        tenant_existence::TenantExistence,
        wake_alarm::WakeAlarm,
    };
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use async_trait::async_trait;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };
    use tokio::{sync::Notify, time::timeout};

    /// Counts every use reported to a runtime.
    #[derive(Default)]
    struct ActivityCount(AtomicUsize);

    impl RuntimeActivity for ActivityCount {
        fn record_activity(&self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn cached_reporting_to(activity: &Arc<ActivityCount>, existence: TenantExistence) -> CachedRuntime<(), ()> {
        woken_for(activity, existence, Duration::from_secs(60))
    }

    /// A runtime whose work is never done.
    struct StillWorking;

    #[async_trait]
    impl DueWorkSource for StillWorking {
        async fn due_work(&self) -> DueWork {
            DueWork::Outstanding
        }
    }

    fn woken_for(activity: &Arc<ActivityCount>, existence: TenantExistence, residency: Duration) -> CachedRuntime<(), ()> {
        CachedRuntime::woken(
            Arc::new(()),
            (),
            TenantDatabaseName::from("andromeda_tenant_acme".to_string()),
            existence,
            LeaseOrigin::new(
                Arc::new(Notify::new()),
                Arc::new(WakeAlarm::new()),
                Arc::clone(activity) as Arc<dyn RuntimeActivity>,
            ),
            residency,
            Arc::new(StillWorking),
        )
    }

    #[test]
    fn a_woken_runtime_is_within_its_wake_residency_until_the_residency_ends() {
        let cached = woken_for(&Arc::new(ActivityCount::default()), TenantExistence::Established, Duration::from_secs(60));
        let now = Instant::now();
        assert!(cached.is_within_wake_residency(now));
        assert!(cached.is_within_wake_residency(now + Duration::from_secs(59)));
        assert!(!cached.is_within_wake_residency(now + Duration::from_secs(61)));
    }

    #[test]
    fn a_runtime_is_unloadable_once_no_lease_holds_it_and_its_woken_lifetime_ended() {
        let cached = woken_for(&Arc::new(ActivityCount::default()), TenantExistence::Established, Duration::from_secs(60));
        let now = Instant::now();
        let after_lifetime = now + Duration::from_secs(61);
        assert!(!cached.is_unloadable(now), "within its woken lifetime");
        assert!(cached.is_unloadable(after_lifetime));
        let lease = RuntimeLease::clone_from(&cached.origin);
        assert!(!cached.is_unloadable(after_lifetime), "a request holds it");
        drop(lease);
        assert!(cached.is_unloadable(after_lifetime));
    }

    #[tokio::test]
    async fn letting_the_last_lease_go_rings_a_waker_sleeping_for_room() {
        let alarm = Arc::new(WakeAlarm::new());
        let (cached, held) = CachedRuntime::<(), ()>::newly_leased(
            Arc::new(()),
            (),
            TenantDatabaseName::from("andromeda_tenant_acme".to_string()),
            TenantExistence::Established,
            LeaseOrigin::new(
                Arc::new(Notify::new()),
                Arc::clone(&alarm),
                Arc::new(ActivityCount::default()) as Arc<dyn RuntimeActivity>,
            ),
        );
        let first = RuntimeLease::hold(held);
        let second = RuntimeLease::clone_from(&cached.origin);
        alarm.begin_pass();
        alarm.sleep_for_room(None);

        drop(first);
        assert!(
            timeout(Duration::from_millis(50), alarm.rung()).await.is_err(),
            "another request still holds the runtime"
        );
        drop(second);
        assert!(timeout(Duration::from_millis(50), alarm.rung()).await.is_ok());
    }

    #[test]
    fn a_runtime_a_request_loaded_holds_no_wake_residency() {
        let (cached, _lease) = CachedRuntime::newly_leased(
            Arc::new(()),
            (),
            TenantDatabaseName::from("andromeda_tenant_acme".to_string()),
            TenantExistence::Established,
            LeaseOrigin::new(
                Arc::new(Notify::new()),
                Arc::new(WakeAlarm::new()),
                Arc::new(ActivityCount::default()) as Arc<dyn RuntimeActivity>,
            ),
        );
        assert!(!cached.is_within_wake_residency(Instant::now()));
    }

    /// Pacing background work by use needs every touch to reach the runtime,
    /// not only the cache's own last-use stamp.
    #[test]
    fn every_touch_tells_the_runtime_it_was_reached_for() {
        let activity = Arc::new(ActivityCount::default());
        let cached = cached_reporting_to(&activity, TenantExistence::Established);
        cached.touch();
        cached.touch();
        pretty_assertions::assert_eq!(activity.0.load(Ordering::Relaxed), 2);
    }

    /// A request commits what it wrote before it lets the runtime go, so the
    /// release is reported too: a runtime that read its due work while the
    /// request was in flight learns that it must read again.
    #[test]
    fn letting_a_lease_go_tells_the_runtime_it_was_used() {
        let activity = Arc::new(ActivityCount::default());
        let cached = cached_reporting_to(&activity, TenantExistence::Established);
        cached.touch();
        let lease = RuntimeLease::clone_from(&cached.origin);
        pretty_assertions::assert_eq!(activity.0.load(Ordering::Relaxed), 1);
        drop(lease);
        pretty_assertions::assert_eq!(activity.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn a_provisional_runtime_is_established_once_and_stays_so() {
        let cached = cached_reporting_to(&Arc::new(ActivityCount::default()), TenantExistence::Provisional);
        pretty_assertions::assert_eq!(cached.existence(), TenantExistence::Provisional);
        cached.establish();
        pretty_assertions::assert_eq!(cached.existence(), TenantExistence::Established);
        cached.establish();
        pretty_assertions::assert_eq!(cached.existence(), TenantExistence::Established);
    }

    #[test]
    fn a_runtime_built_for_an_established_tenant_starts_established() {
        let cached = cached_reporting_to(&Arc::new(ActivityCount::default()), TenantExistence::Established);
        pretty_assertions::assert_eq!(cached.existence(), TenantExistence::Established);
    }
}
