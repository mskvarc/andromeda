//! The `aggrPeriodDuration` bucket width of an aggregated temporal query
//! (clause 4.5.19).

use crate::error::QueryError;
use iso8601_duration::Duration;

/// A non-zero aggregation bucket width.
///
/// Clause 4.5.19: when `aggrPeriodDuration` is absent, empty, or zero, the
/// whole time range is aggregated into a single bucket. Those three inputs mean
/// one thing, so they all compile to no period at all and this type can only
/// ever hold a width that actually buckets. That is what removes the
/// `"" | "PT0S" | "P0D"` test the SQL builder used to carry: a zero period
/// cannot reach it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AggregationPeriod(Duration);

impl AggregationPeriod {
    /// Parse an `aggrPeriodDuration` value.
    ///
    /// Returns `Ok(None)` for the inputs clause 4.5.19 treats as "no
    /// bucketing", and an error for a value that is neither those nor a valid
    /// ISO 8601 duration.
    pub fn parse(raw: &str) -> Result<Option<AggregationPeriod>, QueryError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        let duration = Duration::parse(trimmed).map_err(|_| QueryError::ParamAggrPeriodDurationInvalid(trimmed.to_string()))?;
        if is_zero(&duration) {
            return Ok(None);
        }
        Ok(Some(AggregationPeriod(duration)))
    }

    /// The ISO 8601 rendering PostgreSQL reads as an `interval`.
    ///
    /// PostgreSQL accepts the ISO 8601 duration form directly, so no
    /// conversion into its own `'1 hour'` syntax is needed. The rendering is
    /// only well formed because a zero duration cannot be constructed: the
    /// upstream `Display` writes a bare `P` for one, which is not an interval.
    pub fn as_interval_literal(&self) -> String {
        self.0.to_string()
    }
}

/// Whether every component of a duration is zero.
///
/// The upstream type has no such predicate, and comparing against a
/// `Duration::new(0.0, …)` would compare `f32`s just as directly with more
/// ceremony.
fn is_zero(duration: &Duration) -> bool {
    [duration.year, duration.month, duration.day, duration.hour, duration.minute, duration.second]
        .iter()
        .all(|component| *component == 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_period_of_one_hour_renders_as_an_iso_8601_interval() {
        let period = AggregationPeriod::parse("PT1H").expect("parses").expect("non-zero");
        assert_eq!(period.as_interval_literal(), "PT1H");
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
        assert_eq!(period.as_interval_literal(), "P1M");
    }
}
