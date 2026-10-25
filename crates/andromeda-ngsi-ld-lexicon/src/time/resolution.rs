//! The microsecond resolution every instant and time of day is held at.
//!
//! Clause 4.6.3 caps a written fraction at six digits and PostgreSQL
//! `TIMESTAMPTZ` stores microseconds, so a microsecond is the finest unit the
//! broker accepts, stores and renders. Text finer than that is refused by the
//! reader in [`crate::time::lexical`]; a value the broker produces itself (a
//! clock reading, a computed expiry) can be finer, and is brought to the
//! microsecond here, truncating so the result is never later than the value
//! given.

use chrono::SubsecRound;

/// The fractional-second digits of the microsecond resolution.
const MICROSECOND_DIGITS: u16 = 6;

/// `value` with any sub-microsecond remainder discarded.
///
/// Truncation works on the nanosecond field, which is never negative, so the
/// result is never later than `value` on either side of the epoch, and a value
/// already at the microsecond is returned unchanged.
#[must_use]
pub fn to_microsecond<T: SubsecRound>(value: T) -> T {
    value.trunc_subsecs(MICROSECOND_DIGITS)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::time::resolution::to_microsecond;
    use chrono::{DateTime, NaiveTime, TimeDelta, Timelike};

    #[test]
    fn a_nanosecond_instant_is_truncated_to_its_microsecond() {
        let instant = DateTime::UNIX_EPOCH + TimeDelta::nanoseconds(1_234_567_891);
        pretty_assertions::assert_eq!(to_microsecond(instant).nanosecond(), 234_567_000);
    }

    #[test]
    fn an_instant_before_the_epoch_is_truncated_towards_the_past() {
        let instant = DateTime::UNIX_EPOCH - TimeDelta::nanoseconds(500);
        let truncated = to_microsecond(instant);
        assert!(truncated <= instant, "{truncated} is later than {instant}");
        pretty_assertions::assert_eq!(truncated, DateTime::UNIX_EPOCH - TimeDelta::microseconds(1));
    }

    #[test]
    fn a_time_of_day_is_truncated_the_same_way() {
        let time = NaiveTime::from_hms_nano_opt(12, 0, 0, 123_456_789).expect("a time of day");
        pretty_assertions::assert_eq!(to_microsecond(time).nanosecond(), 123_456_000);
    }

    #[test]
    fn truncating_twice_changes_nothing() {
        let once = to_microsecond(DateTime::UNIX_EPOCH + TimeDelta::nanoseconds(987_654_321));
        pretty_assertions::assert_eq!(to_microsecond(once), once);
    }
}
