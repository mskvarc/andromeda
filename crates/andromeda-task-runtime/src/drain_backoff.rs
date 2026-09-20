//! Pure backoff schedule for a wake-triggered drain's idle intervals.

use crate::wake_trigger::DrainFallbackPolicy;
use std::time::{Duration, SystemTime};

/// How long a drain sleeps before its next pass, decided by how the last pass resolved.
///
/// An idle pass with no known next-due time backs off geometrically from the
/// policy floor toward its ceiling, so a tenant that keeps finding nothing costs
/// ever fewer wake-ups. A pass that found work, or an external wake, resets the
/// schedule to the floor so the following pass runs promptly. A pass that reports
/// a precise next-due instant sleeps until then, clamped into the policy's
/// `[floor, ceiling]` band. The schedule keeps no clock of its own: the caller
/// supplies the current instant when a next-due time must become a duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DrainBackoff {
    /// The shortest sleep the schedule ever returns.
    floor: Duration,
    /// The longest sleep the schedule ever returns.
    ceiling: Duration,
    /// The sleep the next idle pass returns before doubling.
    next_idle: Duration,
}

impl DrainBackoff {
    /// Start a schedule at the policy floor.
    #[must_use]
    pub const fn new(policy: DrainFallbackPolicy) -> DrainBackoff {
        DrainBackoff {
            floor: policy.floor(),
            ceiling: policy.ceiling(),
            next_idle: policy.floor(),
        }
    }

    /// Record an idle pass with no known next-due time, returning the sleep to take.
    ///
    /// Returns the current idle delay, then doubles the delay held for the pass
    /// after it, clamped at the ceiling.
    pub fn record_idle(&mut self) -> Duration {
        let sleep = self.next_idle;
        self.next_idle = self.ceiling.min(self.next_idle.saturating_mul(2));
        sleep
    }

    /// Record a pass that found work, returning the floor sleep and resetting the schedule.
    pub const fn record_worked(&mut self) -> Duration {
        self.next_idle = self.floor;
        self.floor
    }

    /// Record a pass whose next work is due at `due`, returning the clamped sleep until then.
    ///
    /// Resets the geometric schedule so a later bare-idle pass starts from the
    /// floor again.
    pub fn record_idle_until(&mut self, due: SystemTime, now: SystemTime) -> Duration {
        self.next_idle = self.floor;
        let remaining = due.duration_since(now).unwrap_or(Duration::ZERO);
        remaining.clamp(self.floor, self.ceiling)
    }

    /// Reset the schedule to the floor in response to an external wake.
    pub const fn record_wake(&mut self) {
        self.next_idle = self.floor;
    }
}

#[cfg(test)]
mod tests {
    use crate::{drain_backoff::DrainBackoff, wake_trigger::DrainFallbackPolicy};
    use std::time::{Duration, SystemTime};

    fn policy(floor_secs: u64, ceiling_secs: u64) -> DrainFallbackPolicy {
        DrainFallbackPolicy::new(Duration::from_secs(floor_secs), Duration::from_secs(ceiling_secs)).expect("valid policy")
    }

    #[test]
    fn consecutive_idle_passes_double_the_delay_up_to_the_ceiling() {
        let mut backoff = DrainBackoff::new(policy(1, 8));
        let sequence: Vec<u64> = (0..5).map(|_| backoff.record_idle().as_secs()).collect();
        assert_eq!(sequence, vec![1, 2, 4, 8, 8]);
    }

    #[test]
    fn a_wake_resets_the_delay_to_the_floor() {
        let mut backoff = DrainBackoff::new(policy(1, 8));
        assert_eq!(backoff.record_idle().as_secs(), 1);
        assert_eq!(backoff.record_idle().as_secs(), 2);
        backoff.record_wake();
        assert_eq!(backoff.record_idle().as_secs(), 1, "a wake returns the next idle sleep to the floor");
    }

    #[test]
    fn a_pass_that_found_work_resets_the_delay_to_the_floor() {
        let mut backoff = DrainBackoff::new(policy(1, 8));
        assert_eq!(backoff.record_idle().as_secs(), 1);
        assert_eq!(backoff.record_idle().as_secs(), 2);
        assert_eq!(backoff.record_worked().as_secs(), 1, "a working pass sleeps the floor");
        assert_eq!(backoff.record_idle().as_secs(), 1, "the idle pass after work starts at the floor");
    }

    #[test]
    fn idle_until_in_the_past_clamps_to_the_floor() {
        let mut backoff = DrainBackoff::new(policy(2, 30));
        let now = SystemTime::now();
        let due = now - Duration::from_secs(10);
        assert_eq!(backoff.record_idle_until(due, now).as_secs(), 2);
    }

    #[test]
    fn idle_until_beyond_the_ceiling_clamps_to_the_ceiling() {
        let mut backoff = DrainBackoff::new(policy(2, 30));
        let now = SystemTime::now();
        let due = now + Duration::from_secs(1_000);
        assert_eq!(backoff.record_idle_until(due, now).as_secs(), 30);
    }

    #[test]
    fn idle_until_within_the_band_sleeps_until_due() {
        let mut backoff = DrainBackoff::new(policy(2, 30));
        let now = SystemTime::now();
        let due = now + Duration::from_secs(9);
        assert_eq!(backoff.record_idle_until(due, now).as_secs(), 9);
    }

    #[test]
    fn idle_until_resets_the_geometric_schedule() {
        let mut backoff = DrainBackoff::new(policy(1, 8));
        assert_eq!(backoff.record_idle().as_secs(), 1);
        assert_eq!(backoff.record_idle().as_secs(), 2);
        let now = SystemTime::now();
        backoff.record_idle_until(now + Duration::from_secs(5), now);
        assert_eq!(backoff.record_idle().as_secs(), 1, "a bare idle after a next-due pass starts at the floor");
    }
}
