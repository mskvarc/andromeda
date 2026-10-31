//! Matchers on the temporal entities a broker answers (clause 5.2.20 normalized and concise temporal representation),
//! read through the typed model: the evolution of an attribute, as the list of its instances, and the attributes a
//! temporal entity lacks.
//!
//! An evolution is matched as a slice of instances, so googletest's list matchers (`len`, `contains`, `each`) and
//! [`nth`](crate::nth::nth) apply to it with an [`AttributeMatcher`](crate::attribute_matcher::AttributeMatcher) per
//! instance.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{Attribute, AttributeName, InstanceId, Output, evolution::TemporalEntity};
use std::fmt::{self, Debug};

/// The instances of the evolution `name` of `entity`, in the answer's order.
pub(crate) fn instances<'e>(entity: &'e TemporalEntity<Output>, name: &str) -> Option<&'e [Attribute<Output>]> {
    entity
        .attributes
        .iter()
        .find_map(|(candidate, evolution)| (candidate.as_str() == name).then(|| evolution.as_slice()))
}

/// Which instance of an evolution a test names.
#[derive(Debug)]
pub enum InstanceChoice<M> {
    /// The first instance meeting the matcher.
    Meeting(M),
    /// The instance at this position, from 0.
    At(usize),
}

// Hand-written: a matcher is named by its `Debug` form, which googletest matchers all carry.
impl<M: Debug> fmt::Display for InstanceChoice<M> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InstanceChoice::Meeting(matcher) => write!(formatter, "meeting {matcher:?}"),
            InstanceChoice::At(index) => write!(formatter, "at {index}"),
        }
    }
}

/// The temporal entity holds no instance the test names.
#[derive(Debug, thiserror::Error)]
#[error("the evolution of {name} holds no instance {which} carrying an instanceId")]
pub struct InstanceNotFound<M: Debug> {
    /// The attribute name.
    name: AttributeName,
    /// Which instance was looked for.
    which: InstanceChoice<M>,
}

/// The `instanceId` of the first instance of the evolution `name` meeting `matcher` (clause 4.5.7).
///
/// # Errors
///
/// Returns [`InstanceNotFound`] when no instance meets it or the one that does carries no `instanceId`.
pub fn instance_id_where<M: for<'a> Matcher<&'a Attribute<Output>> + Debug>(
    entity: &TemporalEntity<Output>,
    name: &AttributeName,
    matcher: M,
) -> Result<InstanceId, InstanceNotFound<M>> {
    instances(entity, name.as_ref())
        .and_then(|found| found.iter().find(|instance| matcher.matches(instance).is_match()))
        .and_then(|instance| instance.meta().output.instance_id.clone())
        .ok_or_else(|| InstanceNotFound {
            // clone: the error names the attribute the caller keeps
            name: name.clone(),
            which: InstanceChoice::Meeting(matcher),
        })
}

/// The `instanceId` of the instance at `index` (from 0) of the evolution `name` (clause 4.5.7).
///
/// # Errors
///
/// Returns [`InstanceNotFound`] when there is no such instance or it carries no `instanceId`.
pub fn instance_id_at(entity: &TemporalEntity<Output>, name: &AttributeName, index: usize) -> Result<InstanceId, InstanceNotFound<()>> {
    instances(entity, name.as_ref())
        .and_then(|found| found.get(index))
        .and_then(|instance| instance.meta().output.instance_id.clone())
        .ok_or_else(|| InstanceNotFound {
            // clone: the error names the attribute the caller keeps
            name: name.clone(),
            which: InstanceChoice::At(index),
        })
}

/// Matches a temporal entity whose evolution `name` meets a matcher on its instances.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasEvolution<M> {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The requirements on the instances.
    matcher: M,
}

/// A temporal entity whose evolution of the attribute `name` meets `matcher`, a matcher on the slice of its
/// instances.
#[must_use]
pub fn has_evolution<M>(name: &str, matcher: M) -> HasEvolution<M> {
    HasEvolution {
        name: name.to_owned(),
        matcher,
    }
}

impl<'e, M: for<'a> Matcher<&'a [Attribute<Output>]>> Matcher<&'e TemporalEntity<Output>> for HasEvolution<M> {
    fn matches(&self, actual: &'e TemporalEntity<Output>) -> MatcherResult {
        instances(actual, &self.name).map_or(MatcherResult::NoMatch, |found| self.matcher.matches(found))
    }

    fn describe(&self, result: MatcherResult) -> Description {
        format!("has an evolution of {} which {}", self.name, self.matcher.describe(result)).into()
    }

    fn explain_match(&self, actual: &'e TemporalEntity<Output>) -> Description {
        instances(actual, &self.name).map_or_else(
            || format!("which has no evolution of {}", self.name).into(),
            |found| self.matcher.explain_match(found),
        )
    }
}

/// Matches a temporal entity without an evolution of the attribute `name`.
#[derive(Debug, Clone, MatcherBase)]
pub struct LacksEvolution {
    /// The attribute name, as the answer's terms write it.
    name: String,
}

/// A temporal entity without an evolution of the attribute `name`.
#[must_use]
pub fn lacks_evolution(name: &str) -> LacksEvolution {
    LacksEvolution { name: name.to_owned() }
}

impl Matcher<&TemporalEntity<Output>> for LacksEvolution {
    fn matches(&self, actual: &TemporalEntity<Output>) -> MatcherResult {
        instances(actual, &self.name).is_none().into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has no evolution of {}", self.name).into(),
            MatcherResult::NoMatch => format!("has an evolution of {}", self.name).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_matcher::property,
        evolution_matcher::{has_evolution, lacks_evolution},
        nth::nth,
    };
    use googletest::prelude::*;
    use ngsi_ld::model::{Lossless, Output, evolution::TemporalEntity};

    /// A Vehicle whose speed evolved through two instances, as a broker answers it.
    fn vehicle() -> Result<TemporalEntity<Output>> {
        Ok(TemporalEntity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[
            {"type":"Property","value":120,"observedAt":"2018-08-01T12:03:00Z"},
            {"type":"Property","value":80,"observedAt":"2018-08-01T12:05:00Z"}]}"#,
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn an_evolution_is_matched_as_the_list_of_its_instances() -> Result<()> {
        let entity = vehicle()?;
        expect_that!(&entity, has_evolution("speed", len(eq(2))));
        expect_that!(&entity, has_evolution("speed", nth(1, property().value(80))));
        expect_that!(&entity, has_evolution("speed", contains(property().value(120))));
        expect_that!(&entity, not(has_evolution("speed", contains(property().value(90)))));
        verify_that!(&entity, lacks_evolution("temperature"))
    }
}
