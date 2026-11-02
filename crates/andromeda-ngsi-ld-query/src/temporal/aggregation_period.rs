//! The `aggrPeriodDuration` bucket width of an aggregated temporal query
//! (clause 4.5.19).

use crate::{
    error::QueryError,
    temporal::{period_grid::PeriodGrid, window::TimeInterval},
};
use andromeda_ngsi_ld_lexicon::duration::Iso8601Duration;

/// A non-zero aggregation bucket width.
///
/// Clause 4.5.19: when `aggrPeriodDuration` is absent, empty, or zero, the
/// whole time range is aggregated into a single bucket. Those three inputs mean
/// one thing, so they all compile to no period at all and this type can only
/// ever hold a width that actually buckets. A SQL builder needs no
/// `"" | "PT0S" | "P0D"` test: a zero period cannot reach it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AggregationPeriod(Iso8601Duration);

impl AggregationPeriod {
    /// Parse an `aggrPeriodDuration` value.
    ///
    /// Returns `Ok(None)` for the inputs clause 4.5.19 treats as "no
    /// bucketing", and an error for a value that is neither those nor a valid
    /// ISO 8601 duration.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::ParamAggrPeriodDurationInvalid`] when the value is
    /// neither one of those inputs nor a valid ISO 8601 duration.
    pub fn parse(raw: &str) -> Result<Option<AggregationPeriod>, QueryError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let duration = trimmed.parse::<Iso8601Duration>().map_err(QueryError::ParamAggrPeriodDurationInvalid)?;
        if duration.is_zero() {
            return Ok(None);
        }
        Ok(Some(AggregationPeriod(duration)))
    }

    /// The ISO 8601 rendering of the bucket width.
    ///
    /// This is the clause 4.5.19 form the client wrote.
    #[must_use]
    pub fn as_iso8601(&self) -> String {
        self.0.to_string()
    }

    /// How the periods of this duration lie on the timeline: exact for a
    /// duration of days and time, calendar for one with a year or month part,
    /// fractional ones included.
    #[must_use]
    pub const fn grid(&self) -> PeriodGrid {
        if self.0.years() == 0.0 && self.0.months() == 0.0 {
            PeriodGrid::Exact
        } else {
            PeriodGrid::Calendar
        }
    }

    /// Whether no grid of this duration, wherever its origin lies, lays more
    /// than `periods` periods over `interval`: never for an interval open at
    /// either end or a calendar grid, whose periods have no fixed length.
    ///
    /// An interval of span s overlaps at most ⌈s / w⌉ + 1 periods of width w;
    /// one more is allowed for the rounding of a fractional width.
    #[must_use]
    pub fn spans_at_most(&self, interval: TimeInterval, periods: u32) -> bool {
        let (Some(from), Some(to)) = (interval.opening(), interval.closing()) else {
            return false;
        };
        let (Some(width), Ok(span)) = (self.0.to_std(), (*to - *from).to_std()) else {
            return false;
        };
        (span.as_secs_f64() / width.as_secs_f64()).ceil() + 2.0 <= f64::from(periods)
    }

    /// Whether the duration is a whole number of months and nothing else, so
    /// every boundary of its grid is the origin moved by whole months.
    #[must_use]
    pub fn is_whole_months(&self) -> bool {
        let months = self.0.years() * 12.0 + self.0.months();
        self.0.days() == 0.0 && self.0.hours() == 0.0 && self.0.minutes() == 0.0 && self.0.seconds() == 0.0 && months.fract() == 0.0
    }
}

/// Serialize and deserialize an optional [`AggregationPeriod`] as an ISO 8601
/// duration.
///
/// A dedicated adapter rather than a `#[serde(try_from)]` on the type itself,
/// because clause 4.5.19 folds three distinct inputs — absent, empty and zero —
/// into one meaning, and only the `Option` can express that a present value
/// deserialized to no period at all.
pub mod optional {
    use crate::temporal::aggregation_period::AggregationPeriod;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    /// Render the period when present, and JSON `null` when absent.
    ///
    /// # Errors
    ///
    /// Propagates whatever the underlying serializer reports.
    pub fn serialize<S>(period: &Option<AggregationPeriod>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match period {
            Some(period) => serializer.serialize_some(&period.as_iso8601()),
            None => serializer.serialize_none(),
        }
    }

    /// Parse an optional ISO 8601 duration into a bucket width.
    ///
    /// # Errors
    ///
    /// Returns a deserializer error when a value is present and is not an
    /// ISO 8601 duration. A zero duration is not an error: clause 4.5.19 reads
    /// it as no bucketing, which is `None`.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<AggregationPeriod>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw: Option<String> = Option::deserialize(deserializer)?;
        match raw {
            Some(raw) => AggregationPeriod::parse(&raw).map_err(Error::custom),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        error::QueryError,
        temporal::{aggregation_period::AggregationPeriod, period_grid::PeriodGrid, window::TimeInterval},
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_period_of_one_hour_renders_as_an_iso_8601_interval() {
        let period = AggregationPeriod::parse("PT1H").expect("parses").expect("non-zero");
        assert_eq!(period.as_iso8601(), "PT1H");
    }

    #[test]
    fn an_aggregation_period_of_zero_covers_the_whole_range() {
        // Clause 4.5.19: absent, empty and zero all mean one bucket over the
        // whole range, so none of them is representable as a period.
        for raw in ["", "   ", "PT0S", "P0D"] {
            assert_eq!(AggregationPeriod::parse(raw).expect("parses"), None, "{raw}");
        }
    }

    #[test]
    fn a_value_that_is_not_an_iso_8601_duration_is_rejected() {
        let error = AggregationPeriod::parse("one hour").expect_err("must reject");
        assert!(matches!(error, QueryError::ParamAggrPeriodDurationInvalid(_)), "got {error:?}");
    }

    #[test]
    fn a_calendar_period_survives_parsing() {
        let period = AggregationPeriod::parse("P1M").expect("parses").expect("non-zero");
        assert_eq!(period.as_iso8601(), "P1M");
    }

    /// Clause 4.5.19.1 admits a duration mixing years or months with days or
    /// time; a period of any of them lasts as long as the months it covers.
    #[test]
    fn a_duration_with_a_year_or_month_part_has_a_calendar_grid() {
        for raw in ["P1M", "P1Y", "P1M1D", "P1MT2H", "P0.5M", "P2Y6M"] {
            let period = AggregationPeriod::parse(raw).expect("parses").expect("non-zero");
            assert_eq!(period.grid(), PeriodGrid::Calendar, "{raw}");
        }
    }

    #[test]
    fn a_duration_of_years_and_months_alone_is_whole_months() {
        for raw in ["P1M", "P1Y", "P2Y6M", "P0.5Y"] {
            let period = AggregationPeriod::parse(raw).expect("parses").expect("non-zero");
            assert!(period.is_whole_months(), "{raw}");
        }
    }

    #[test]
    fn a_duration_with_a_fraction_of_a_month_or_a_day_or_time_part_is_not_whole_months() {
        for raw in ["P0.5M", "P1M1D", "P1MT2H", "P1W", "PT1H"] {
            let period = AggregationPeriod::parse(raw).expect("parses").expect("non-zero");
            assert!(!period.is_whole_months(), "{raw}");
        }
    }

    fn between(from: &str, to: &str) -> TimeInterval {
        TimeInterval::Between {
            from: from.parse().expect("a valid instant"),
            to: to.parse().expect("a valid instant"),
        }
    }

    /// A day overlaps at most 25 hourly periods (24 plus one the origin may
    /// split), so it fits a page of 26 and not one of 25, which keeps the
    /// one-period allowance for a fractional width.
    #[test]
    fn a_bounded_window_on_an_exact_grid_spans_its_periods_plus_two() {
        let hourly = AggregationPeriod::parse("PT1H").expect("parses").expect("non-zero");
        let day = between("2024-01-01T00:00:00Z", "2024-01-02T00:00:00Z");
        assert!(hourly.spans_at_most(day, 26));
        assert!(!hourly.spans_at_most(day, 25));
        let quarter_hours = AggregationPeriod::parse("PT15M").expect("parses").expect("non-zero");
        assert!(quarter_hours.spans_at_most(between("2024-01-01T00:00:00Z", "2024-01-01T00:20:00Z"), 4));
        assert!(!quarter_hours.spans_at_most(between("2024-01-01T00:00:00Z", "2024-01-01T00:20:00Z"), 3));
    }

    /// The count of an open window or a calendar grid is not known before the
    /// instances are read.
    #[test]
    fn an_open_window_or_a_calendar_grid_spans_no_known_count() {
        let hourly = AggregationPeriod::parse("PT1H").expect("parses").expect("non-zero");
        let instant = "2024-01-01T00:00:00Z".parse().expect("a valid instant");
        for open in [TimeInterval::Unbounded, TimeInterval::After(instant), TimeInterval::Before(instant)] {
            assert!(!hourly.spans_at_most(open, u32::MAX), "{open:?}");
        }
        let monthly = AggregationPeriod::parse("P1M").expect("parses").expect("non-zero");
        assert!(!monthly.spans_at_most(between("2024-01-01T00:00:00Z", "2024-01-02T00:00:00Z"), u32::MAX));
    }

    #[test]
    fn a_duration_of_days_and_time_has_an_exact_grid() {
        for raw in ["PT1H", "P1D", "PT1H30M", "P1DT12H", "PT0.5S"] {
            let period = AggregationPeriod::parse(raw).expect("parses").expect("non-zero");
            assert_eq!(period.grid(), PeriodGrid::Exact, "{raw}");
        }
    }
}
