//! The step one aggregation period spans on the time axis (clause 4.5.19.1).
//!
//! An ISO 8601 duration mixes nominal calendar units with exact ones. The
//! store buckets either on whole calendar months or on a fixed span, and
//! refuses a width that combines the two, so a period steps in exactly one of
//! those ways. Fractions spill downwards the way the store reads them: a
//! fraction of a year into rounded months, a fraction of a month into days of a
//! thirty-day month, and a fraction of a day into seconds.

use chrono::{DateTime, Datelike, Months, TimeDelta, Utc};
use iso8601_duration::Duration;
use num_traits::ToPrimitive;

/// Months in a year.
const MONTHS_PER_YEAR: f64 = 12.0;
/// Days a fraction of a month is counted in.
const DAYS_PER_MONTH: f64 = 30.0;
/// Seconds in a day, which is always 24 hours on the UTC axis.
const SECONDS_PER_DAY: f64 = 86_400.0;
/// Seconds in an hour.
const SECONDS_PER_HOUR: f64 = 3_600.0;
/// Seconds in a minute.
const SECONDS_PER_MINUTE: f64 = 60.0;
/// Microseconds in a second, the store's instant precision.
const MICROS_PER_SECOND: f64 = 1_000_000.0;

/// How far one aggregation period reaches from its start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodStride {
    /// A whole number of calendar months, never zero.
    Months(u32),
    /// A fixed span, always positive.
    Fixed(TimeDelta),
}

impl PeriodStride {
    /// The stride `duration` steps by. `None` for a duration that is zero,
    /// negative, out of range, or mixes calendar months with days or time.
    #[must_use]
    pub fn of(duration: &Duration) -> Option<PeriodStride> {
        let year = f64::from(duration.year);
        let month = f64::from(duration.month);
        let months = year.trunc() * MONTHS_PER_YEAR + (year.fract() * MONTHS_PER_YEAR).round_ties_even() + month.trunc();
        let days = month.fract() * DAYS_PER_MONTH + f64::from(duration.day);
        let seconds =
            days * SECONDS_PER_DAY + f64::from(duration.hour) * SECONDS_PER_HOUR + f64::from(duration.minute) * SECONDS_PER_MINUTE + f64::from(duration.second);
        let micros = (seconds * MICROS_PER_SECOND).round_ties_even();
        match (months > 0.0, micros > 0.0) {
            (true, false) => months.to_u32().map(PeriodStride::Months),
            (false, true) => micros.to_i64().map(|micros| PeriodStride::Fixed(TimeDelta::microseconds(micros))),
            (true, true) | (false, false) => None,
        }
    }

    /// The instant `count` periods after `from`. `None` past the end of the
    /// representable time axis.
    #[must_use]
    pub fn advance(self, from: DateTime<Utc>, count: u64) -> Option<DateTime<Utc>> {
        match self {
            PeriodStride::Months(months) => from.checked_add_months(Months::new(months.checked_mul(u32::try_from(count).ok()?)?)),
            PeriodStride::Fixed(span) => from.checked_add_signed(span.checked_mul(i32::try_from(count).ok()?)?),
        }
    }

    /// The instant `count` periods before `from`. `None` past the start of the
    /// representable time axis.
    #[must_use]
    pub fn retreat(self, from: DateTime<Utc>, count: u64) -> Option<DateTime<Utc>> {
        match self {
            PeriodStride::Months(months) => from.checked_sub_months(Months::new(months.checked_mul(u32::try_from(count).ok()?)?)),
            PeriodStride::Fixed(span) => from.checked_sub_signed(span.checked_mul(i32::try_from(count).ok()?)?),
        }
    }

    /// The fewest periods that, laid end to end from `from`, reach `to`. Zero
    /// when `to` is not after `from`; `None` when the count leaves the
    /// representable time axis.
    #[must_use]
    pub fn periods_forward(self, from: DateTime<Utc>, to: DateTime<Utc>) -> Option<u64> {
        if to <= from {
            return Some(0);
        }
        match self {
            PeriodStride::Months(months) => {
                let mut count = month_distance(from, to)? / u64::from(months);
                while self.advance(from, count)? < to {
                    count += 1;
                }
                Some(count)
            }
            PeriodStride::Fixed(span) => fixed_periods(to - from, span),
        }
    }

    /// The fewest periods that, laid end to end back from `from`, reach `to`.
    /// Zero when `to` is not before `from`; `None` when the count leaves the
    /// representable time axis.
    #[must_use]
    pub fn periods_backward(self, from: DateTime<Utc>, to: DateTime<Utc>) -> Option<u64> {
        if to >= from {
            return Some(0);
        }
        match self {
            PeriodStride::Months(months) => {
                let mut count = month_distance(to, from)? / u64::from(months);
                while self.retreat(from, count)? > to {
                    count += 1;
                }
                Some(count)
            }
            PeriodStride::Fixed(span) => fixed_periods(from - to, span),
        }
    }
}

/// The calendar months from `earlier`'s month to `later`'s, ignoring the day.
fn month_distance(earlier: DateTime<Utc>, later: DateTime<Utc>) -> Option<u64> {
    let index = |instant: DateTime<Utc>| i64::from(instant.year()) * 12 + i64::from(instant.month0());
    u64::try_from(index(later) - index(earlier)).ok()
}

/// The fewest fixed spans covering `distance`, which is positive.
fn fixed_periods(distance: TimeDelta, span: TimeDelta) -> Option<u64> {
    let distance = u64::try_from(distance.num_microseconds()?).ok()?;
    let span = u64::try_from(span.num_microseconds()?).ok()?;
    Some(distance.div_ceil(span))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn stride(raw: &str) -> Option<PeriodStride> {
        PeriodStride::of(&Duration::parse(raw).expect("an ISO 8601 duration"))
    }

    fn instant(raw: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(raw).expect("valid instant").with_timezone(&Utc)
    }

    #[test]
    fn calendar_units_step_in_whole_months() {
        assert_eq!(stride("P1M"), Some(PeriodStride::Months(1)));
        assert_eq!(stride("P1Y"), Some(PeriodStride::Months(12)));
        assert_eq!(stride("P1.5Y"), Some(PeriodStride::Months(18)));
        assert_eq!(stride("P1.1Y"), Some(PeriodStride::Months(13)));
    }

    #[test]
    fn exact_units_step_in_a_fixed_span_with_days_of_24_hours() {
        assert_eq!(stride("PT1H"), Some(PeriodStride::Fixed(TimeDelta::hours(1))));
        assert_eq!(stride("P1.5D"), Some(PeriodStride::Fixed(TimeDelta::hours(36))));
        assert_eq!(stride("P1.5W"), Some(PeriodStride::Fixed(TimeDelta::hours(252))));
        assert_eq!(stride("PT0.1S"), Some(PeriodStride::Fixed(TimeDelta::milliseconds(100))));
    }

    #[test]
    fn a_width_mixing_months_with_days_or_time_has_no_stride() {
        assert_eq!(stride("P1M1D"), None);
        assert_eq!(stride("P1.5M"), None);
        assert_eq!(stride("P1MT1H"), None);
        assert_eq!(stride("PT0S"), None);
    }

    #[test]
    fn a_fixed_stride_counts_the_spans_that_cover_a_distance() {
        let hour = PeriodStride::Fixed(TimeDelta::hours(1));
        let start = instant("2024-01-01T00:00:00Z");
        assert_eq!(hour.periods_forward(start, instant("2024-01-01T03:00:00Z")), Some(3));
        assert_eq!(hour.periods_forward(start, instant("2024-01-01T02:10:00Z")), Some(3));
        assert_eq!(hour.periods_forward(start, start), Some(0));
        assert_eq!(hour.periods_backward(instant("2024-01-01T02:00:00Z"), instant("2024-01-01T00:30:00Z")), Some(2));
        assert_eq!(hour.advance(start, 3), Some(instant("2024-01-01T03:00:00Z")));
        assert_eq!(hour.retreat(start, 2), Some(instant("2023-12-31T22:00:00Z")));
    }

    #[test]
    fn a_month_stride_counts_calendar_months() {
        let quarter = PeriodStride::Months(3);
        let start = instant("2024-01-01T00:00:00Z");
        assert_eq!(quarter.periods_forward(start, instant("2024-07-01T00:00:00Z")), Some(2));
        assert_eq!(quarter.periods_forward(start, instant("2024-07-02T00:00:00Z")), Some(3));
        assert_eq!(quarter.periods_backward(start, instant("2023-10-01T00:00:00Z")), Some(1));
        assert_eq!(quarter.periods_backward(start, instant("2023-09-30T00:00:00Z")), Some(2));
        assert_eq!(quarter.advance(start, 2), Some(instant("2024-07-01T00:00:00Z")));
        assert_eq!(quarter.retreat(start, 1), Some(instant("2023-10-01T00:00:00Z")));
    }
}
