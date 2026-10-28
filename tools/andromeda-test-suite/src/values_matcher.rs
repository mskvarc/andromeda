//! Matchers on the simplified temporal entities a broker answers (clause 4.5.9, `format=temporalValues`), read
//! through the typed model: the `[value, time]` pairs a Property's `values` holds, and each pair's value and instant.
//!
//! The pairs are matched as a slice, so googletest's list matchers (`len`, `contains`) and [`nth`](crate::nth::nth)
//! apply to them with a [`PairMatcher`] per pair. Values and instants are compared by [`Equivalent`].

use crate::equivalence::Equivalent;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{
    DateTime,
    Value,
    evolution::{SimplifiedTemporalEntity, TemporalValue, TemporalValues},
};

/// The requirements on one `[value, time]` pair.
#[derive(Debug, Clone, Default, MatcherBase)]
pub struct PairMatcher {
    /// The value, when required.
    value: Option<Value>,
    /// The instant, when required.
    time: Option<DateTime>,
}

/// A pair of any value and instant.
#[must_use]
pub fn pair() -> PairMatcher {
    PairMatcher::default()
}

impl PairMatcher {
    /// Requires the value to be `value`.
    #[must_use]
    pub fn value(self, value: impl Into<Value>) -> PairMatcher {
        PairMatcher {
            value: Some(value.into()),
            ..self
        }
    }

    /// Requires the instant to be `time`.
    #[must_use]
    pub fn time(self, time: DateTime) -> PairMatcher {
        PairMatcher { time: Some(time), ..self }
    }
}

impl Matcher<&TemporalValue<Value>> for PairMatcher {
    fn matches(&self, actual: &TemporalValue<Value>) -> MatcherResult {
        (self.value.as_ref().is_none_or(|value| actual.value.equivalent(value)) && self.time.as_ref().is_none_or(|time| actual.time.equivalent(time))).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is a pair meeting {self:?}").into(),
            MatcherResult::NoMatch => format!("isn't a pair meeting {self:?}").into(),
        }
    }
}

/// Matches a simplified temporal entity whose Property `name` holds pairs meeting a matcher.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasValues<M> {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The requirements on the pairs.
    matcher: M,
}

/// A simplified temporal entity whose Property `name` holds `values` meeting `matcher`, a matcher on the slice of its
/// pairs; an attribute of another kind does not match.
#[must_use]
pub fn has_values<M>(name: &str, matcher: M) -> HasValues<M> {
    HasValues {
        name: name.to_owned(),
        matcher,
    }
}

impl<'e, M: for<'a> Matcher<&'a [TemporalValue<Value>]>> Matcher<&'e SimplifiedTemporalEntity> for HasValues<M> {
    fn matches(&self, actual: &'e SimplifiedTemporalEntity) -> MatcherResult {
        let pairs = actual
            .attributes
            .iter()
            .find_map(|(candidate, values)| (candidate.to_string() == self.name).then_some(values));
        match pairs {
            Some(TemporalValues::Property(pairs)) => self.matcher.matches(pairs.as_slice()),
            Some(
                TemporalValues::GeoProperty(_)
                | TemporalValues::LanguageProperty(_)
                | TemporalValues::VocabProperty(_)
                | TemporalValues::ListProperty(_)
                | TemporalValues::JsonProperty(_)
                | TemporalValues::Relationship(_)
                | TemporalValues::ListRelationship(_),
            )
            | None => MatcherResult::NoMatch,
        }
    }

    fn describe(&self, result: MatcherResult) -> Description {
        format!("has a Property {} whose values {}", self.name, self.matcher.describe(result)).into()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        nth::nth,
        values_matcher::{has_values, pair},
    };
    use googletest::prelude::*;
    use ngsi_ld::model::evolution::SimplifiedTemporalEntity;

    /// A Vehicle whose speed was 30 and then 40, as a broker answers it in the simplified temporal representation.
    fn vehicle() -> Result<SimplifiedTemporalEntity> {
        Ok(SimplifiedTemporalEntity::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property",
            "values":[[30,"2020-08-01T12:03:00Z"],[40,"2020-08-01T12:05:00Z"]]},
            "isParked":{"type":"Relationship","objects":[["urn:ngsi-ld:Parking:1","2020-08-01T12:03:00Z"]]}}"#,
        )?)
    }

    #[gtest]
    fn the_pairs_of_a_property_are_matched_as_a_list() -> Result<()> {
        let entity = vehicle()?;
        expect_that!(&entity, has_values("speed", len(eq(2))));
        expect_that!(&entity, has_values("speed", nth(1, pair().value(40).time("2020-08-01T12:05:00.000Z".parse()?))));
        verify_that!(&entity, not(has_values("speed", nth(0, pair().value(40)))))
    }

    #[gtest]
    fn a_relationship_or_a_missing_attribute_holds_no_property_values() -> Result<()> {
        let entity = vehicle()?;
        expect_that!(&entity, not(has_values("isParked", anything())));
        verify_that!(&entity, not(has_values("fuel", anything())))
    }
}
