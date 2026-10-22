//! Calendar-aware expiry of an NGSI-LD lifetime.
//!
//! Snapshots (clause 5.13) and Entity Maps (clause 5.5.14) carry a lifetime
//! expressed as an ISO 8601 duration and derive an `expiresAt` instant from it.
//! An ISO 8601 duration mixes calendar components (years, months) with fixed
//! ones (days and below): a month spans 28 to 31 days and a year 365 or 366, so
//! the year and month parts only have a length once anchored to the instant
//! they start from. [`expiry`] advances the calendar month by month for the
//! year and month parts, adds the day-and-below part as a fixed offset, and
//! reports an unrepresentable result as [`None`] rather than panicking.

use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, time::resolution::to_microsecond};
use chrono::{DateTime, Datelike, Months, TimeDelta, Utc};
use std::time::Duration as StdDuration;

/// Largest whole-month advance considered. `DateTime<Utc>` spans roughly
/// 262 000 years, so any greater count cannot land in range; the bound also
/// keeps the month-by-month advance from spinning on a hostile lifetime.
const MAX_WHOLE_MONTHS: u32 = 4_000_000;

/// The instant a lifetime starting at `start` and lasting `lifetime` ends.
///
/// Whole years and months advance the calendar one month at a time, so `P1M`
/// from 31 January lands on the last day of February and `P3M` covers three
/// real months rather than three times the starting month's length. A
/// fractional month or year is charged against the length of the calendar month
/// the whole-month advance lands in, and the day, hour, minute and second parts
/// are added as a fixed offset.
///
/// Returns [`None`] when the lifetime is not finite or the result would fall
/// outside the representable `DateTime<Utc>` range, leaving the caller to decide
/// how to treat an unusable lifetime instead of inheriting a panic.
#[must_use]
pub fn expiry(start: DateTime<Utc>, lifetime: &Iso8601Duration) -> Option<DateTime<Utc>> {
    let total_months = f64::from(lifetime.years()) * 12.0 + f64::from(lifetime.months());
    if !total_months.is_finite() || total_months < 0.0 {
        return None;
    }
    let whole_months = total_months.trunc();
    let fractional_months = total_months - whole_months;
    if whole_months > f64::from(MAX_WHOLE_MONTHS) {
        return None;
    }

    // One calendar month per step keeps each month its real length without a
    // fallible float-to-integer conversion; `remaining` is a non-negative,
    // integer-valued count bounded by MAX_WHOLE_MONTHS.
    let mut advanced = start;
    let mut remaining = whole_months;
    while remaining >= 1.0 {
        advanced = advanced.checked_add_months(Months::new(1))?;
        remaining -= 1.0;
    }

    let mut offset_seconds =
        f64::from(lifetime.days()) * 86_400.0 + f64::from(lifetime.hours()) * 3_600.0 + f64::from(lifetime.minutes()) * 60.0 + f64::from(lifetime.seconds());
    if fractional_months > 0.0 {
        offset_seconds += fractional_months * f64::from(days_in_month(advanced)?) * 86_400.0;
    }

    let offset = StdDuration::try_from_secs_f64(offset_seconds).ok()?;
    advanced.checked_add_signed(TimeDelta::from_std(offset).ok()?).map(to_microsecond)
}

/// The number of days in the calendar month `instant` falls in.
fn days_in_month(instant: DateTime<Utc>) -> Option<u32> {
    let first_of_month = instant.date_naive().with_day(1)?;
    let first_of_next = first_of_month.checked_add_months(Months::new(1))?;
    u32::try_from(first_of_next.signed_duration_since(first_of_month).num_days()).ok()
}

#[cfg(test)]
mod tests {
    use crate::lifetime_expiry::expiry;
    use andromeda_ngsi_ld_lexicon::{duration::Iso8601Duration, time::rendering::instant_text};
    use chrono::{DateTime, TimeDelta, TimeZone, Utc};

    fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
            .single()
            .expect("valid test instant")
    }

    #[test]
    fn fixed_time_part_adds_exact_seconds() {
        let start = at(2020, 1, 1, 0, 0, 0);
        let lifetime: Iso8601Duration = "PT1H30M".parse().expect("valid duration");
        pretty_assertions::assert_eq!(expiry(start, &lifetime), Some(at(2020, 1, 1, 1, 30, 0)));
    }

    #[test]
    fn one_month_from_month_end_clamps_to_shorter_month() {
        let start = at(2021, 1, 31, 0, 0, 0);
        let lifetime: Iso8601Duration = "P1M".parse().expect("valid duration");
        // Calendar-aware: 31 January plus one month is the last day of February.
        pretty_assertions::assert_eq!(expiry(start, &lifetime), Some(at(2021, 2, 28, 0, 0, 0)));
    }

    #[test]
    fn multiple_months_advance_the_calendar_not_a_fixed_month_length() {
        let start = at(2021, 1, 1, 0, 0, 0);
        let lifetime: Iso8601Duration = "P3M".parse().expect("valid duration");
        // Three real months, so 1 April, not three times January's 31 days.
        pretty_assertions::assert_eq!(expiry(start, &lifetime), Some(at(2021, 4, 1, 0, 0, 0)));
    }

    #[test]
    fn one_year_from_leap_day_clamps_to_non_leap_year() {
        let start = at(2020, 2, 29, 12, 0, 0);
        let lifetime: Iso8601Duration = "P1Y".parse().expect("valid duration");
        pretty_assertions::assert_eq!(expiry(start, &lifetime), Some(at(2021, 2, 28, 12, 0, 0)));
    }

    #[test]
    fn fractional_month_is_charged_against_the_entered_month() {
        let start = at(2001, 2, 1, 0, 0, 0);
        let lifetime: Iso8601Duration = "P0.5M".parse().expect("valid duration");
        // Half of February's 28 days is 14 days.
        pretty_assertions::assert_eq!(expiry(start, &lifetime), Some(at(2001, 2, 15, 0, 0, 0)));
    }

    #[test]
    fn mixed_calendar_and_fixed_parts_compose() {
        let start = at(2021, 1, 1, 0, 0, 0);
        let lifetime: Iso8601Duration = "P1Y2M10DT2H30M".parse().expect("valid duration");
        // 2021-01-01 + 14 months = 2022-03-01, then 10 days and 2h30m.
        pretty_assertions::assert_eq!(expiry(start, &lifetime), Some(at(2022, 3, 11, 2, 30, 0)));
    }

    #[test]
    fn a_nanosecond_start_yields_a_microsecond_expiry() {
        let start = at(2026, 10, 6, 13, 29, 28) + TimeDelta::nanoseconds(90_386);
        let lifetime: Iso8601Duration = "PT1H".parse().expect("valid duration");
        let ends = expiry(start, &lifetime).expect("a representable expiry");
        pretty_assertions::assert_eq!(ends, at(2026, 10, 6, 14, 29, 28) + TimeDelta::microseconds(90));
        pretty_assertions::assert_eq!(instant_text(ends), "2026-10-06T14:29:28.000090Z");
    }

    #[test]
    fn zero_duration_returns_the_start_instant() {
        let start = at(2020, 6, 15, 8, 0, 0);
        let lifetime: Iso8601Duration = "PT0S".parse().expect("valid duration");
        pretty_assertions::assert_eq!(expiry(start, &lifetime), Some(start));
    }

    #[test]
    fn out_of_range_lifetime_returns_none_instead_of_panicking() {
        let start = at(2020, 1, 1, 0, 0, 0);
        let lifetime: Iso8601Duration = "P1000000000Y".parse().expect("valid duration");
        pretty_assertions::assert_eq!(expiry(start, &lifetime), None);
    }
}
