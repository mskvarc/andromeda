//! The pure timing policy a dormant-tenant wake pass follows.
//!
//! Separated from the wake loop so the arithmetic (when a tenant next becomes
//! due, how a just-evicted tenant is held off, how a woken runtime is aged so
//! it never displaces a request-touched one) is decided without a clock, a
//! database or a cache, and can be tested as plain values. The loop that owns
//! the cache and the schedule applies these decisions.

use crate::pending_work::PendingWork;
use std::time::{Duration, SystemTime};

/// The timing knobs a wake pass follows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WakerPolicy {
    /// The shortest delay before a just-evicted tenant is woken again.
    ///
    /// A tenant whose earliest obligation is sooner than this is still not woken
    /// until `min_dormancy` has passed, so evicting and immediately re-loading a
    /// tenant that is merely a little overdue cannot thrash.
    min_dormancy: Duration,
    /// How long a woken runtime is guaranteed to stay resident to do its work.
    wake_residency: Duration,
    /// The idle time-to-live the janitor evicts against.
    idle_ttl: Duration,
    /// How many tenants a single wake pass loads at most.
    max_wakes_per_pass: usize,
}

impl WakerPolicy {
    /// A policy from the configured durations and counts.
    #[must_use]
    pub const fn new(min_dormancy: Duration, wake_residency: Duration, idle_ttl: Duration, max_wakes_per_pass: usize) -> WakerPolicy {
        WakerPolicy {
            min_dormancy,
            wake_residency,
            idle_ttl,
            max_wakes_per_pass,
        }
    }

    /// The most tenants one pass loads.
    #[must_use]
    pub const fn max_wakes_per_pass(&self) -> usize {
        self.max_wakes_per_pass
    }

    /// When a tenant should next be woken given what its probe found, or `None`
    /// when it has nothing due and should stay dormant.
    ///
    /// A due instant sooner than `now + min_dormancy` is clamped up to it, so a
    /// tenant just evicted is not immediately reloaded; a later instant is kept
    /// as is.
    #[must_use]
    pub fn next_wake(&self, work: PendingWork, now: SystemTime) -> Option<SystemTime> {
        match work {
            PendingWork::None => None,
            PendingWork::DueAt(due) => Some(due.max(now + self.min_dormancy)),
        }
    }

    /// How far to backdate a freshly woken runtime's last use so it is the
    /// first idle-eviction victim yet keeps `wake_residency` of idle life.
    ///
    /// The runtime cache measures last use as an `Instant`, so it applies this
    /// duration directly rather than the `SystemTime` [`woken_last_used`](Self::woken_last_used)
    /// computes for value tests.
    #[must_use]
    pub fn woken_backdate(&self) -> Duration {
        self.idle_ttl.checked_sub(self.wake_residency).unwrap_or(Duration::ZERO)
    }

    /// How long a woken runtime is guaranteed to stay resident to do its work,
    /// spared both the idle sweep and an unload to make room.
    #[must_use]
    pub const fn wake_residency(&self) -> Duration {
        self.wake_residency
    }

    /// When a tenant whose probe failed should be re-probed.
    ///
    /// A failed probe records a retry instant rather than `None`: leaving it
    /// unscheduled would strand a tenant whose store hiccuped once, so it is
    /// retried after `min_dormancy` instead of being treated as having nothing
    /// due.
    #[must_use]
    pub fn retry_wake(&self, now: SystemTime) -> SystemTime {
        now + self.min_dormancy
    }

    /// When a wake deferred for want of room is retried.
    ///
    /// `room_in` is how long until the first runtime that only its woken
    /// lifetime protects may be unloaded, or `None` when every runtime is held
    /// by a request. The retry comes when that room opens, and no later than
    /// `min_dormancy`: retried a fixed interval later instead, a tenant whose
    /// periodic work shares that interval meets the same crowded cache at every
    /// attempt.
    #[must_use]
    pub fn deferred_wake(&self, now: SystemTime, room_in: Option<Duration>) -> SystemTime {
        now + room_in.map_or(self.min_dormancy, |room_in| room_in.min(self.min_dormancy))
    }

    /// The last-used stamp to give a freshly woken runtime so it is the first
    /// idle-eviction victim yet survives `wake_residency` to do its work.
    ///
    /// A woken runtime exists only to drain the work the probe found, and must
    /// never displace a tenant a request touched. Backdating its last use to
    /// `now - (idle_ttl - wake_residency)` leaves it exactly `wake_residency`
    /// of idle life: long enough to work, short enough that any
    /// request-touched runtime (last used at `now`) outranks it and is evicted
    /// later.
    #[must_use]
    pub fn woken_last_used(&self, now: SystemTime) -> SystemTime {
        let age = self.idle_ttl.checked_sub(self.wake_residency).unwrap_or(Duration::ZERO);
        now.checked_sub(age).unwrap_or(now)
    }
}

#[cfg(test)]
mod tests {
    use crate::{pending_work::PendingWork, waker::WakerPolicy};
    use std::time::{Duration, SystemTime};

    /// `min_dormancy` 30 s, `wake_residency` 60 s, `idle_ttl` 900 s, 4 wakes/pass.
    const MIN_DORMANCY_SECS: u64 = 30;
    const WAKE_RESIDENCY_SECS: u64 = 60;
    const IDLE_TTL_SECS: u64 = 900;

    fn policy() -> WakerPolicy {
        WakerPolicy::new(
            Duration::from_secs(MIN_DORMANCY_SECS),
            Duration::from_secs(WAKE_RESIDENCY_SECS),
            Duration::from_secs(IDLE_TTL_SECS),
            4,
        )
    }

    #[test]
    fn an_evicted_tenant_records_the_probed_instant_clamped_to_min_dormancy() {
        let policy = policy();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);

        // A due instant sooner than now + min_dormancy is clamped up to it.
        let soon = PendingWork::DueAt(now + Duration::from_secs(5));
        assert_eq!(policy.next_wake(soon, now), Some(now + Duration::from_secs(30)));

        // A due instant beyond min_dormancy is kept as it is.
        let later = PendingWork::DueAt(now + Duration::from_secs(100));
        assert_eq!(policy.next_wake(later, now), Some(now + Duration::from_secs(100)));

        // Nothing due schedules no wake at all.
        assert_eq!(policy.next_wake(PendingWork::None, now), None);
    }

    #[test]
    fn a_failed_probe_records_a_retry_instant_rather_than_none() {
        let policy = policy();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        // The retry is a real instant one min_dormancy out, never None.
        assert_eq!(policy.retry_wake(now), now + Duration::from_secs(30));
    }

    #[test]
    fn a_deferred_wake_is_retried_when_a_woken_lifetime_ends_and_no_later_than_min_dormancy() {
        let policy = policy();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        assert_eq!(policy.deferred_wake(now, Some(Duration::from_millis(400))), now + Duration::from_millis(400));
        assert_eq!(policy.deferred_wake(now, Some(Duration::from_secs(45))), now + Duration::from_secs(30));
        assert_eq!(policy.deferred_wake(now, None), now + Duration::from_secs(30), "only requests hold the cache");
    }

    #[test]
    fn a_woken_runtime_is_backdated_to_evict_before_a_request_touched_one() {
        let policy = policy();
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        // idle_ttl minus wake_residency of age, so it is evicted wake_residency
        // from now and is older than a runtime a request just touched at `now`.
        let woken = policy.woken_last_used(now);
        let expected_age = Duration::from_secs(IDLE_TTL_SECS - WAKE_RESIDENCY_SECS);
        assert_eq!(woken + expected_age, now, "a woken runtime is aged by idle_ttl minus wake_residency");
        assert!(woken < now, "a woken runtime outranks a request-touched runtime for eviction");
    }

    #[test]
    fn a_wake_residency_at_least_the_idle_ttl_backdates_to_now() {
        // Degenerate configuration: residency swallows the whole idle TTL.
        let policy = WakerPolicy::new(Duration::from_secs(30), Duration::from_mins(15), Duration::from_mins(15), 4);
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        assert_eq!(policy.woken_last_used(now), now);
    }
}
