//! When a cached runtime was last reached for, as an orderable stamp.

use crate::runtime::process_epoch::now_millis;
use std::time::Duration;

/// Milliseconds since the process epoch at which a runtime was last used.
///
/// Signed: a backdated stamp may precede the epoch, and clamping it to the
/// epoch would tie it with a runtime a request touched in the process's first
/// milliseconds. Later stamps order greater, whatever the process's uptime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct LastUsed(i64);

impl LastUsed {
    /// The stamp of this moment.
    pub(super) fn now() -> LastUsed {
        LastUsed(i64::try_from(now_millis()).unwrap_or(i64::MAX))
    }

    /// The stamp of a use `age` ago, which may precede the process epoch.
    pub(super) fn aged(age: Duration) -> LastUsed {
        let age_millis = i64::try_from(age.as_millis()).unwrap_or(i64::MAX);
        LastUsed(LastUsed::now().0.saturating_sub(age_millis))
    }

    /// The stamp as stored in an atomic.
    pub(super) const fn to_stored(self) -> i64 {
        self.0
    }

    /// The stamp held in an atomic.
    pub(super) const fn from_stored(stored: i64) -> LastUsed {
        LastUsed(stored)
    }

    /// Whether at least `ttl` has passed since the use.
    ///
    /// Stamps have millisecond resolution, so a zero `ttl` holds for a use in
    /// the current millisecond: the comparison is inclusive.
    pub(super) fn has_been_idle_for(self, ttl: Duration) -> bool {
        let ttl_millis = i64::try_from(ttl.as_millis()).unwrap_or(i64::MAX);
        LastUsed::now().0.saturating_sub(self.0) >= ttl_millis
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::last_used::LastUsed;
    use std::time::Duration;

    #[test]
    fn a_stamp_older_than_the_process_uptime_sorts_strictly_older_than_a_fresh_one() {
        let ancient = LastUsed::aged(Duration::from_hours(10 * 365 * 24));
        assert!(ancient < LastUsed::now());
    }

    #[test]
    fn two_aged_stamps_sort_by_age_even_when_both_precede_the_epoch() {
        assert!(LastUsed::aged(Duration::from_secs(20_000)) < LastUsed::aged(Duration::from_secs(10_000)));
    }

    #[test]
    fn a_stamp_survives_the_round_trip_through_its_stored_form() {
        let stamp = LastUsed::aged(Duration::from_secs(5));
        assert_eq!(LastUsed::from_stored(stamp.to_stored()), stamp);
    }

    #[test]
    fn a_use_has_been_idle_for_a_ttl_only_once_that_long_has_passed() {
        assert!(LastUsed::aged(Duration::from_secs(120)).has_been_idle_for(Duration::from_secs(60)));
        assert!(!LastUsed::now().has_been_idle_for(Duration::from_secs(60)));
        assert!(LastUsed::now().has_been_idle_for(Duration::ZERO));
    }
}
