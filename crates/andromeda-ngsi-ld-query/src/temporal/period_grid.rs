//! How the periods of an `aggrPeriodDuration` lie on the timeline (clause
//! 4.5.19.1).
//!
//! A duration of days and time is an exact number of seconds, so every period
//! lasts as long as the next and the boundaries are the anchor moved by whole
//! multiples of the duration. A duration with a year or month part lasts as
//! long as the calendar months it covers. Each boundary is computed from the
//! anchor and never stepped from the boundary before it, so a month-end clamp
//! cannot carry forward. A boundary keeps the anchor's day of month, clamped
//! to the month's last day, unless the anchor is itself the last day of its
//! month: then every boundary is the last day of its month.
//!
//! A continuation of a page that ends on a boundary opens at that boundary and
//! lays its own grid from there. The page may therefore end only on a boundary
//! from which the same rule reproduces the rest of the grid; [`lays_the_grid`]
//! decides that.

use chrono::{DateTime, Datelike, TimeDelta, Utc};

/// The layout of the periods of one aggregation duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodGrid {
    /// A duration without a year or month part: every period lasts the same
    /// number of seconds.
    Exact,
    /// A duration with a year or month part: a period lasts as long as the
    /// calendar months it covers, 28 to 31 days for each.
    Calendar,
}

/// The day of month every boundary of a calendar grid falls on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalendarDay {
    /// The anchor is the last day of its month, so every boundary is the last
    /// day of its month.
    MonthEnd,
    /// The anchor is not the last day of its month: every boundary keeps this
    /// day of month, clamped to the last day of the month it falls in.
    Fixed(u32),
}

impl CalendarDay {
    /// The day rule the grid anchored at `anchor` follows.
    #[must_use]
    pub fn of_anchor(anchor: DateTime<Utc>) -> CalendarDay {
        if is_month_end(anchor) {
            CalendarDay::MonthEnd
        } else {
            CalendarDay::Fixed(anchor.day())
        }
    }
}

/// Whether `instant` falls on the last day of its UTC month.
#[must_use]
pub fn is_month_end(instant: DateTime<Utc>) -> bool {
    instant.checked_add_signed(TimeDelta::days(1)).is_some_and(|next| next.day() == 1)
}

/// Whether `boundary` lays the calendar grid anchored at `anchor`: the day rule
/// applied to `boundary` alone reproduces every boundary of the grid after it.
///
/// A boundary of a month-end grid is a month end, so every one lays it. A
/// boundary of a fixed-day grid lays it when it keeps the day and is not a
/// clamped month end: a month end of 30 days gives the next month's 31st where
/// the grid gives the 30th.
#[must_use]
pub fn lays_the_grid(anchor: DateTime<Utc>, boundary: DateTime<Utc>) -> bool {
    boundary.time() == anchor.time()
        && match CalendarDay::of_anchor(anchor) {
            CalendarDay::MonthEnd => is_month_end(boundary),
            CalendarDay::Fixed(day) => boundary.day() == day && !is_month_end(boundary),
        }
}

#[cfg(test)]
mod tests {
    use crate::temporal::period_grid::{CalendarDay, is_month_end, lays_the_grid};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;

    fn at(raw: &str) -> DateTime<Utc> {
        raw.parse().expect("a valid instant")
    }

    /// The 30th of January is not a month end, so the grid keeps the 30th.
    #[test]
    fn a_grid_from_the_30th_keeps_the_30th_and_skips_the_clamped_and_month_end_boundaries() {
        let anchor = at("2024-01-30T10:00:00Z");
        assert_eq!(CalendarDay::of_anchor(anchor), CalendarDay::Fixed(30));
        assert!(!lays_the_grid(anchor, at("2024-02-29T10:00:00Z")), "clamped to the month end");
        assert!(lays_the_grid(anchor, at("2024-03-30T10:00:00Z")));
        assert!(!lays_the_grid(anchor, at("2024-04-30T10:00:00Z")), "a month end gives the 31st after it");
        assert!(lays_the_grid(anchor, at("2024-05-30T10:00:00Z")));
    }

    /// The 31st of January is a month end, so every boundary is one.
    #[test]
    fn a_grid_from_the_31st_of_january_is_a_month_end_grid() {
        let anchor = at("2024-01-31T00:00:00Z");
        assert_eq!(CalendarDay::of_anchor(anchor), CalendarDay::MonthEnd);
        assert!(lays_the_grid(anchor, at("2024-02-29T00:00:00Z")));
        assert!(lays_the_grid(anchor, at("2024-03-31T00:00:00Z")));
        assert!(lays_the_grid(anchor, at("2024-04-30T00:00:00Z")));
        assert!(!lays_the_grid(anchor, at("2024-03-30T00:00:00Z")), "not a month end");
    }

    /// A 29 February is a month end, so a yearly grid keeps 28 February in
    /// non-leap years and 29 February in leap years.
    #[test]
    fn a_yearly_grid_from_29_february_is_a_month_end_grid() {
        let anchor = at("2024-02-29T00:00:00Z");
        assert_eq!(CalendarDay::of_anchor(anchor), CalendarDay::MonthEnd);
        assert!(lays_the_grid(anchor, at("2025-02-28T00:00:00Z")));
        assert!(lays_the_grid(anchor, at("2028-02-29T00:00:00Z")));
        assert!(!lays_the_grid(anchor, at("2025-02-27T00:00:00Z")));
    }

    /// 28 February of a leap year is not a month end, so the grid keeps the
    /// 28th, and in a non-leap year that 28th is a month end, which is not a
    /// legal cut.
    #[test]
    fn a_yearly_grid_from_28_february_of_a_leap_year_skips_the_non_leap_month_ends() {
        let anchor = at("2024-02-28T00:00:00Z");
        assert_eq!(CalendarDay::of_anchor(anchor), CalendarDay::Fixed(28));
        assert!(!lays_the_grid(anchor, at("2025-02-28T00:00:00Z")), "month end in a non-leap year");
        assert!(lays_the_grid(anchor, at("2028-02-28T00:00:00Z")));
    }

    #[test]
    fn a_grid_from_the_15th_lays_every_boundary_on_the_15th() {
        let anchor = at("2024-01-15T00:00:00Z");
        assert!(lays_the_grid(anchor, at("2024-02-15T00:00:00Z")));
        assert!(!lays_the_grid(anchor, at("2024-02-14T00:00:00Z")));
    }

    /// The boundary must sit at the anchor's time of day too.
    #[test]
    fn a_boundary_at_another_time_of_day_lays_no_grid() {
        assert!(!lays_the_grid(at("2024-01-15T10:00:00Z"), at("2024-02-15T11:00:00Z")));
    }

    #[test]
    fn month_end_recognises_the_last_day_of_each_month() {
        assert!(is_month_end(at("2024-02-29T00:00:00Z")));
        assert!(is_month_end(at("2025-02-28T23:00:00Z")));
        assert!(!is_month_end(at("2024-02-28T00:00:00Z")));
        assert!(is_month_end(at("2024-04-30T00:00:00Z")));
    }
}
