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
use ngsi_ld::model::{Attribute, InstanceId, Output, evolution::TemporalEntity};
use std::fmt::Debug;

/// The instances of the evolution `name` of `entity`, in the answer's order.
pub(crate) fn instances<'e>(entity: &'e TemporalEntity<Output>, name: &str) -> Option<&'e [Attribute<Output>]> {
    entity
        .attributes
        .iter()
        .find_map(|(candidate, evolution)| (candidate.to_string() == name).then(|| evolution.as_slice()))
}

/// The temporal entity holds no instance the test names.
#[derive(Debug, thiserror::Error)]
#[error("the evolution of {name} holds no instance {which} carrying an instanceId")]
pub struct InstanceNotFound {
    /// The attribute name.
    name: String,
    /// Which instance was looked for.
    which: String,
}

/// The `instanceId` of the first instance of the evolution `name` meeting `matcher` (clause 4.5.7).
///
/// # Errors
///
/// Returns [`InstanceNotFound`] when no instance meets it or the one that does carries no `instanceId`.
pub fn instance_id_where<M: for<'a> Matcher<&'a Attribute<Output>> + Debug>(
    entity: &TemporalEntity<Output>,
    name: &str,
    matcher: M,
) -> Result<InstanceId, InstanceNotFound> {
    instances(entity, name)
        .and_then(|found| found.iter().find(|instance| matcher.matches(instance).is_match()))
        .and_then(|instance| instance.meta().output.instance_id.clone())
        .ok_or_else(|| InstanceNotFound {
            name: name.to_owned(),
            which: format!("meeting {matcher:?}"),
        })
}

/// The `instanceId` of the instance at `index` (from 0) of the evolution `name` (clause 4.5.7).
///
/// # Errors
///
/// Returns [`InstanceNotFound`] when there is no such instance or it carries no `instanceId`.
pub fn instance_id_at(entity: &TemporalEntity<Output>, name: &str, index: usize) -> Result<InstanceId, InstanceNotFound> {
    instances(entity, name)
        .and_then(|found| found.get(index))
        .and_then(|instance| instance.meta().output.instance_id.clone())
        .ok_or_else(|| InstanceNotFound {
            name: name.to_owned(),
            which: format!("at {index}"),
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
