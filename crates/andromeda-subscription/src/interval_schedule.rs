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

/// The first grid point strictly after `after` of a periodic subscription
/// created at `anchor` with period `interval`, or `None` for a non-positive
/// interval or an instant out of range.
///
/// Grid points are `anchor + k * interval` for `k >= 1`: clause 5.8.6 sends a
/// periodic notification each time the interval is reached, and none at
/// creation. The creation instant alone fixes the grid, so a sweep that runs
/// late delays the notification it sends without moving the ones after it.
/// Given a sweep's cutoff this is when the sweep next has work; given the
/// instant a subscription last answered its grid it is the notification the
/// subscription is owed, in the past for a tenant that slept across it.
#[must_use]
pub fn next_interval_due(anchor: DateTime<Utc>, interval: Duration, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let period = interval.num_microseconds().filter(|period| *period > 0)?;
    let elapsed = (after - anchor).num_microseconds()?;
    let reached = if elapsed < 0 { 0 } else { elapsed / period };
    let next = i32::try_from(reached.checked_add(1)?).ok()?;
    anchor.checked_add_signed(interval.checked_mul(next)?)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::interval_schedule::{interval_duration, next_interval_due};
    use chrono::{DateTime, Duration, TimeZone, Utc};

    fn instant(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 20, 12, 0, second).single().expect("valid instant")
    }

    #[test]
    fn an_instant_past_several_grid_points_rolls_to_the_following_one() {
        assert_eq!(next_interval_due(instant(0), Duration::seconds(5), instant(17)), Some(instant(20)));
    }

    #[test]
    fn an_instant_exactly_on_a_grid_point_is_followed_by_the_next_one() {
        assert_eq!(next_interval_due(instant(0), Duration::seconds(5), instant(10)), Some(instant(15)));
    }

    #[test]
    fn a_subscription_never_answered_is_first_owed_one_interval_after_its_creation() {
        assert_eq!(next_interval_due(instant(30), Duration::seconds(5), instant(30)), Some(instant(35)));
    }

    #[test]
    fn a_subscription_created_after_the_cutoff_is_first_due_one_interval_after_its_creation() {
        assert_eq!(next_interval_due(instant(40), Duration::seconds(5), instant(30)), Some(instant(45)));
    }

    /// Clause 5.8.6 fixes the period: the sweep that answered :10 thirteen
    /// seconds late, past the :20 grid point, still leaves :30 as the next one.
    #[test]
    fn a_sweep_answering_past_the_interval_does_not_push_the_next_due_instant() {
        assert_eq!(next_interval_due(instant(0), Duration::seconds(10), instant(14)), Some(instant(20)));
        assert_eq!(next_interval_due(instant(0), Duration::seconds(10), instant(23)), Some(instant(30)));
    }

    #[test]
    fn a_fractional_interval_keeps_its_fraction_on_the_grid() {
        let interval = interval_duration(2.5).expect("a positive interval");
        assert_eq!(
            next_interval_due(instant(0), interval, instant(6)),
            Some(instant(7) + Duration::milliseconds(500))
        );
    }

    #[test]
    fn a_non_positive_interval_has_no_grid_point() {
        assert_eq!(next_interval_due(instant(0), Duration::zero(), instant(6)), None);
        assert_eq!(next_interval_due(instant(0), Duration::seconds(-5), instant(6)), None);
    }
}
