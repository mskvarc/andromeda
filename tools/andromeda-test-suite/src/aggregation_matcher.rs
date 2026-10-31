//! Matchers on the aggregated temporal entities a broker answers (clause 5.2.20 aggregated temporal representation),
//! read through the typed model: the periods one aggregation method yields for one attribute, and each period's
//! value and bounds.
//!
//! The periods of a method are matched as a slice, so googletest's list matchers (`len`, `contains`) and
//! [`nth`](crate::nth::nth) apply to them with a [`PeriodMatcher`] per period. Values and instants are compared by
//! [`Equivalent`].

use crate::equivalence::Equivalent;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{
    DateTime,
    TemporalError,
    Value,
    evolution::{AggregatedKind, AggregatedTemporalEntity, AggregatedValue, AggregationMethod},
};

/// The requirements on one aggregation period.
#[derive(Debug, Clone, Default, MatcherBase)]
pub struct PeriodMatcher {
    /// The aggregate, when required.
    value: Option<Value>,
    /// The start of the period, when required.
    start: Option<DateTime>,
    /// The end of the period, when required.
    end: Option<DateTime>,
    /// The digits the aggregate must be written with, when required.
    digits: Option<Digits>,
    /// The bounds the numeric aggregate must lie strictly between, when required.
    between: Option<(f64, f64)>,
    /// Whether the aggregate must be a string.
    text: bool,
}

/// The lexical form a numeric aggregate must have.
#[derive(Debug, Clone)]
enum Digits {
    /// Exactly this text.
    Exactly(String),
    /// A text starting with this one.
    StartingWith(String),
}

impl Digits {
    /// Whether `value` is a number written with these digits.
    fn hold(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Exactly(text), Value::Number(number)) => number.as_str() == text,
            (Self::StartingWith(prefix), Value::Number(number)) => number.as_str().starts_with(prefix.as_str()),
            (
                Self::Exactly(_) | Self::StartingWith(_),
                Value::Boolean(_) | Value::Text(_) | Value::Array(_) | Value::Object(_) | Value::Typed(_) | Value::JsonNull,
            ) => false,
        }
    }
}

/// A period of any value and bounds.
#[must_use]
pub fn period() -> PeriodMatcher {
    PeriodMatcher::default()
}

impl PeriodMatcher {
    /// Requires the aggregate to be `value`.
    #[must_use]
    pub fn value(self, value: impl Into<Value>) -> PeriodMatcher {
        PeriodMatcher {
            value: Some(value.into()),
            ..self
        }
    }

    /// Requires the period to start at the instant `start`.
    #[must_use]
    pub fn start(self, start: DateTime) -> PeriodMatcher {
        PeriodMatcher { start: Some(start), ..self }
    }

    /// Requires the period to end at the instant `end`.
    #[must_use]
    pub fn end(self, end: DateTime) -> PeriodMatcher {
        PeriodMatcher { end: Some(end), ..self }
    }

    /// Requires the aggregate to be a number written with exactly the digits `text`, which a comparison as a double
    /// would not tell from a neighbour.
    #[must_use]
    pub fn digits(self, text: &str) -> PeriodMatcher {
        PeriodMatcher {
            digits: Some(Digits::Exactly(text.to_owned())),
            ..self
        }
    }

    /// Requires the aggregate to be a number strictly between `above` and `below`, for an aggregate (a standard
    /// deviation) a double does not hold exactly.
    #[must_use]
    pub fn value_between(self, above: f64, below: f64) -> PeriodMatcher {
        PeriodMatcher {
            between: Some((above, below)),
            ..self
        }
    }

    /// Requires the aggregate to be a string, whichever one.
    #[must_use]
    pub fn text(self) -> PeriodMatcher {
        PeriodMatcher { text: true, ..self }
    }

    /// Requires the aggregate to be a number whose digits start with `prefix`.
    #[must_use]
    pub fn digits_starting(self, prefix: &str) -> PeriodMatcher {
        PeriodMatcher {
            digits: Some(Digits::StartingWith(prefix.to_owned())),
            ..self
        }
    }

    /// Whether `actual` meets every requirement.
    fn holds(&self, actual: &AggregatedValue) -> bool {
        (!self.text || matches!(actual.value, Value::Text(_)))
            && self.between.is_none_or(|(above, below)| match &actual.value {
                Value::Number(number) => number.as_f64() > above && number.as_f64() < below,
                Value::Boolean(_) | Value::Text(_) | Value::Array(_) | Value::Object(_) | Value::Typed(_) | Value::JsonNull => false,
            })
            && self.digits.as_ref().is_none_or(|digits| digits.hold(&actual.value))
            && self.value.as_ref().is_none_or(|value| actual.value.equivalent(value))
            && self.start.as_ref().is_none_or(|start| actual.start.equivalent(start))
            && self.end.as_ref().is_none_or(|end| actual.end.equivalent(end))
    }
}

impl Matcher<&AggregatedValue> for PeriodMatcher {
    fn matches(&self, actual: &AggregatedValue) -> MatcherResult {
        self.holds(actual).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is a period meeting {self:?}").into(),
            MatcherResult::NoMatch => format!("isn't a period meeting {self:?}").into(),
        }
    }
}

/// Matches the periods of an aggregation that follow each other: period `i` holds `first + i` and runs from the
/// instant `bounds[i]` to `bounds[i + 1]`, so the list holds one period fewer than there are bounds.
#[derive(Debug, Clone, MatcherBase)]
pub struct SuccessivePeriods {
    /// The aggregate of the first period.
    first: i64,
    /// The instants that bound the periods, from the start of the first to the end of the last.
    bounds: Vec<DateTime>,
}

/// The periods that follow each other over `bounds`, the first aggregating `first` and each next one more.
///
/// # Errors
///
/// Returns [`TemporalError`] when a bound is not an instant.
pub fn successive_periods(first: i64, bounds: &[&str]) -> Result<SuccessivePeriods, TemporalError> {
    Ok(SuccessivePeriods {
        first,
        bounds: bounds.iter().map(|bound| bound.parse()).collect::<Result<_, _>>()?,
    })
}

impl Matcher<&[AggregatedValue]> for SuccessivePeriods {
    fn matches(&self, actual: &[AggregatedValue]) -> MatcherResult {
        (actual.len() + 1 == self.bounds.len()
            && actual
                .iter()
                .zip(self.bounds.windows(2))
                .zip(self.first..)
                .all(|((period, bound), value)| match bound {
                    [start, end] => period.value.equivalent(&Value::from(value)) && period.start.equivalent(start) && period.end.equivalent(end),
                    [] | [_] | [_, _, _, ..] => false,
                }))
        .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        let bounds = self.bounds.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        match result {
            MatcherResult::Match => format!("is the periods from {} over the bounds {bounds}", self.first).into(),
            MatcherResult::NoMatch => format!("isn't the periods from {} over the bounds {bounds}", self.first).into(),
        }
    }
}

/// The periods `method` yields for the attribute `name` of `entity`.
fn periods<'e>(entity: &'e AggregatedTemporalEntity, name: &str, method: AggregationMethod) -> Option<&'e [AggregatedValue]> {
    entity
        .attributes
        .iter()
        .find_map(|(candidate, attribute)| (candidate.as_str() == name).then_some(attribute))
        .and_then(|attribute| attribute.methods.get(&method))
        .map(Vec::as_slice)
}

/// Matches an aggregated temporal entity whose aggregation `method` of the attribute `name` yields periods meeting a
/// matcher.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasAggregate<M> {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The aggregation method.
    method: AggregationMethod,
    /// The requirements on the periods.
    matcher: M,
}

/// An aggregated temporal entity whose `method` aggregation of the attribute `name` yields periods meeting `matcher`,
/// a matcher on the slice of its periods.
#[must_use]
pub fn has_aggregate<M>(name: &str, method: AggregationMethod, matcher: M) -> HasAggregate<M> {
    HasAggregate {
        name: name.to_owned(),
        method,
        matcher,
    }
}

impl<'e, M: for<'a> Matcher<&'a [AggregatedValue]>> Matcher<&'e AggregatedTemporalEntity> for HasAggregate<M> {
    fn matches(&self, actual: &'e AggregatedTemporalEntity) -> MatcherResult {
        periods(actual, &self.name, self.method).map_or(MatcherResult::NoMatch, |found| self.matcher.matches(found))
    }

    fn describe(&self, result: MatcherResult) -> Description {
        format!("has a {} aggregate of {} which {}", self.method, self.name, self.matcher.describe(result)).into()
    }

    fn explain_match(&self, actual: &'e AggregatedTemporalEntity) -> Description {
        periods(actual, &self.name, self.method).map_or_else(
            || format!("which has no {} aggregate of {}", self.method, self.name).into(),
            |found| self.matcher.explain_match(found),
        )
    }
}

/// Matches an aggregated temporal entity whose attribute `name` is aggregated as `kind`.
#[derive(Debug, Clone, MatcherBase)]
pub struct AggregatedAs {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The kind of attribute the aggregation names.
    kind: AggregatedKind,
}

/// An aggregated temporal entity whose attribute `name` carries the type `kind`.
#[must_use]
pub fn aggregated_as(name: &str, kind: AggregatedKind) -> AggregatedAs {
    AggregatedAs { name: name.to_owned(), kind }
}

impl Matcher<&AggregatedTemporalEntity> for AggregatedAs {
    fn matches(&self, actual: &AggregatedTemporalEntity) -> MatcherResult {
        actual
            .attributes
            .iter()
            .any(|(candidate, attribute)| candidate.as_str() == self.name && attribute.kind == self.kind)
            .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("aggregates {} as {:?}", self.name, self.kind).into(),
            MatcherResult::NoMatch => format!("doesn't aggregate {} as {:?}", self.name, self.kind).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        aggregation_matcher::{aggregated_as, has_aggregate, period, successive_periods},
        nth::nth,
    };
    use googletest::prelude::*;
    use ngsi_ld::model::evolution::{AggregatedKind, AggregatedTemporalEntity, AggregationMethod};

    /// A Vehicle whose speed is summed over two periods, as a broker answers it.
    fn vehicle() -> Result<AggregatedTemporalEntity> {
        Ok(AggregatedTemporalEntity::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property",
            "sum":[[30,"2020-01-01T00:00:00Z","2020-01-01T01:00:00Z"],[12.5,"2020-01-01T01:00:00Z","2020-01-01T02:00:00Z"]]}}"#,
        )?)
    }

    #[gtest]
    fn the_digits_of_an_aggregate_are_matched_as_written() -> Result<()> {
        let entity = AggregatedTemporalEntity::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property",
            "avg":[[0.150000000000000005,"2020-01-01T00:00:00Z","2020-01-01T01:00:00Z"]]}}"#,
        )?;
        expect_that!(
            &entity,
            has_aggregate("speed", AggregationMethod::Avg, nth(0, period().digits("0.150000000000000005")))
        );
        expect_that!(
            &entity,
            has_aggregate("speed", AggregationMethod::Avg, nth(0, period().digits_starting("0.15000")))
        );
        expect_that!(&entity, not(has_aggregate("speed", AggregationMethod::Avg, nth(0, period().digits("0.15")))));
        verify_that!(
            &entity,
            not(has_aggregate("speed", AggregationMethod::Avg, nth(0, period().digits_starting("0.2"))))
        )
    }

    #[gtest]
    fn a_number_is_matched_strictly_between_two_bounds() -> Result<()> {
        let entity = AggregatedTemporalEntity::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property",
            "stddev":[[10.000000000000002,"2020-01-01T00:00:00Z","2020-01-01T01:00:00Z"]]}}"#,
        )?;
        expect_that!(
            &entity,
            has_aggregate("speed", AggregationMethod::Stddev, nth(0, period().value_between(9.99, 10.01)))
        );
        verify_that!(
            &entity,
            not(has_aggregate("speed", AggregationMethod::Stddev, nth(0, period().value_between(10.01, 11.0))))
        )
    }

    #[gtest]
    fn an_aggregate_is_matched_as_a_string() -> Result<()> {
        let entity = AggregatedTemporalEntity::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","seen":{"type":"Property",
            "min":[["2024-03-01","2020-01-01T00:00:00Z","2020-01-01T01:00:00Z"]],
            "max":[[3,"2020-01-01T00:00:00Z","2020-01-01T01:00:00Z"]]}}"#,
        )?;
        expect_that!(&entity, has_aggregate("seen", AggregationMethod::Min, nth(0, period().text())));
        verify_that!(&entity, not(has_aggregate("seen", AggregationMethod::Max, nth(0, period().text()))))
    }

    #[gtest]
    fn periods_are_matched_as_a_chain_over_their_bounds() -> Result<()> {
        let entity = AggregatedTemporalEntity::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property",
            "sum":[[30,"2020-01-01T00:00:00Z","2020-01-01T01:00:00Z"],[31,"2020-01-01T01:00:00Z","2020-01-01T02:00:00Z"]]}}"#,
        )?;
        expect_that!(
            &entity,
            has_aggregate(
                "speed",
                AggregationMethod::Sum,
                successive_periods(30, &["2020-01-01T00:00:00Z", "2020-01-01T01:00:00Z", "2020-01-01T02:00:00Z"])?
            )
        );
        expect_that!(
            &entity,
            not(has_aggregate(
                "speed",
                AggregationMethod::Sum,
                successive_periods(30, &["2020-01-01T00:00:00Z", "2020-01-01T01:00:00Z"])?
            ))
        );
        verify_that!(
            &entity,
            not(has_aggregate(
                "speed",
                AggregationMethod::Sum,
                successive_periods(31, &["2020-01-01T00:00:00Z", "2020-01-01T01:00:00Z", "2020-01-01T02:00:00Z"])?
            ))
        )
    }

    #[gtest]
    fn the_periods_of_a_method_are_matched_as_a_list() -> Result<()> {
        let entity = vehicle()?;
        expect_that!(&entity, has_aggregate("speed", AggregationMethod::Sum, len(eq(2))));
        expect_that!(
            &entity,
            has_aggregate(
                "speed",
                AggregationMethod::Sum,
                nth(0, period().value(30).start("2020-01-01T00:00:00.000Z".parse()?))
            )
        );
        expect_that!(&entity, not(has_aggregate("speed", AggregationMethod::Avg, len(eq(2)))));
        verify_that!(&entity, aggregated_as("speed", AggregatedKind::Property))
    }
}
