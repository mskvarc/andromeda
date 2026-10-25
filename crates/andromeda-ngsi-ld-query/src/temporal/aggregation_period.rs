//! The `aggrPeriodDuration` bucket width of an aggregated temporal query
//! (clause 4.5.19).

use crate::{error::QueryError, temporal::period_grid::PeriodGrid};
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
        temporal::{aggregation_period::AggregationPeriod, period_grid::PeriodGrid},
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
    fn a_duration_of_days_and_time_has_an_exact_grid() {
        for raw in ["PT1H", "P1D", "PT1H30M", "P1DT12H", "PT0.5S"] {
            let period = AggregationPeriod::parse(raw).expect("parses").expect("non-zero");
            assert_eq!(period.grid(), PeriodGrid::Exact, "{raw}");
        }
    }
}
