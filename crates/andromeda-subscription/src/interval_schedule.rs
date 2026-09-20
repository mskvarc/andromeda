//! The grid a periodic (`timeInterval`) subscription notifies on (clause 5.8.6).

use chrono::{DateTime, Duration, Utc};
use std::time::Duration as StdDuration;

/// The grid period a stored `time_interval` (positive seconds) describes.
///
/// Returns `None` for a non-positive, non-finite or out-of-range value, which a
/// caller skips rather than scheduling a subscription with no valid grid. Goes
/// through [`std::time::Duration::from_secs_f64`] so a fractional second stored
/// in `DOUBLE PRECISION` converts without a lossy integer cast.
#[must_use]
pub fn interval_duration(seconds: f64) -> Option<Duration> {
    if !(seconds.is_finite() && seconds > 0.0) {
        return None;
    }
    Duration::from_std(StdDuration::from_secs_f64(seconds)).ok()
}

/// The next instant strictly after `cutoff` at which a periodic subscription
/// anchored at `anchor` with period `interval` becomes due.
///
/// Grid points are `anchor + k * interval` for non-negative `k`. An anchor that
/// is already overdue relative to `cutoff` rolls forward to the following grid
/// point rather than returning a past instant, so a sweep that fell behind wakes
/// at the next real notification rather than busy-looping. A subscription whose
/// anchor was advanced to the sweep's own cutoff (because it matched nothing)
/// therefore becomes due exactly one interval after that cutoff. Clause 5.8.6
/// fixes the period; this only decides when the next one lands.
#[must_use]
pub fn next_interval_due(anchor: DateTime<Utc>, interval: Duration, cutoff: DateTime<Utc>) -> DateTime<Utc> {
    // A non-positive interval has no grid to advance along; never return a past
    // instant that would spin the drain.
    if interval <= Duration::zero() {
        return cutoff + Duration::seconds(1);
    }
    // An anchor still in the future is itself the next grid point.
    if anchor > cutoff {
        return anchor;
    }
    let elapsed_secs = (cutoff - anchor).num_seconds();
    let interval_secs = interval.num_seconds().max(1);
    // Full periods between the anchor and the cutoff; the next grid point is the
    // one after, so it is always strictly greater than the cutoff.
    let periods = elapsed_secs / interval_secs;
    let next_period = i32::try_from(periods.saturating_add(1)).unwrap_or(i32::MAX);
    anchor + interval * next_period
}

#[cfg(test)]
mod tests {
    use crate::interval_schedule::next_interval_due;
    use chrono::{DateTime, Duration, TimeZone, Utc};

    fn instant(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 20, 12, 0, second).single().expect("valid instant")
    }

    #[test]
    fn next_due_statement_rolls_an_overdue_anchor_to_the_following_grid_point() {
        // Anchor at :00, five-second grid, sweep cutoff at :17 -- three whole
        // periods have elapsed, so the next grid point is :20, the first strictly
        // after the cutoff, not :05 (long past) nor :15 (already reached).
        let anchor = instant(0);
        let cutoff = instant(17);
        assert_eq!(next_interval_due(anchor, Duration::seconds(5), cutoff), instant(20));
    }

    #[test]
    fn a_subscription_matching_nothing_is_next_due_one_interval_after_the_cutoff() {
        // A subscription that matched nothing has its anchor advanced to the
        // sweep's own cutoff, so its next due is exactly one interval later.
        let cutoff = instant(30);
        assert_eq!(next_interval_due(cutoff, Duration::seconds(5), cutoff), instant(35));
    }

    #[test]
    fn an_anchor_exactly_on_a_grid_point_advances_by_one_interval() {
        let anchor = instant(0);
        let cutoff = instant(10);
        assert_eq!(next_interval_due(anchor, Duration::seconds(5), cutoff), instant(15));
    }

    #[test]
    fn a_future_anchor_is_its_own_next_due() {
        let anchor = instant(40);
        let cutoff = instant(30);
        assert_eq!(next_interval_due(anchor, Duration::seconds(5), cutoff), instant(40));
    }
}
