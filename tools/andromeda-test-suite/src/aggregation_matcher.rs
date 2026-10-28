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

    /// Whether `actual` meets every requirement.
    fn holds(&self, actual: &AggregatedValue) -> bool {
        self.value.as_ref().is_none_or(|value| actual.value.equivalent(value))
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

/// The periods `method` yields for the attribute `name` of `entity`.
fn periods<'e>(entity: &'e AggregatedTemporalEntity, name: &str, method: AggregationMethod) -> Option<&'e [AggregatedValue]> {
    entity
        .attributes
        .iter()
        .find_map(|(candidate, attribute)| (candidate.to_string() == name).then_some(attribute))
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
            .any(|(candidate, attribute)| candidate.to_string() == self.name && attribute.kind == self.kind)
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
        aggregation_matcher::{aggregated_as, has_aggregate, period},
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
