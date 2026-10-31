//! Matchers on the entity types a resource names (clause 4.6.3): one type written alone, a type among a list of them,
//! how many the list holds, and whether a projected entity names a type at all (a projection may leave `type` out,
//! clause 4.21). A type written alone and a list holding only that type are different members.

use crate::spelling_matcher::{SpellingMatcher, alone, listing, listing_count};
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{Entity, EntityTypeName, OneOrMany, Output, Projected, SimplifiedEntity, evolution::TemporalEntity};
use std::fmt::Debug;

/// A resource that names its entity types.
pub trait Typed {
    /// The resource's types, as the broker wrote them; `None` for a projected entity the projection left without its
    /// `type`.
    fn entity_types(&self) -> Option<&OneOrMany<EntityTypeName>>;
}

impl Typed for Entity<Output> {
    fn entity_types(&self) -> Option<&OneOrMany<EntityTypeName>> {
        Some(&self.entity_type)
    }
}

impl Typed for Entity<Output, Projected> {
    fn entity_types(&self) -> Option<&OneOrMany<EntityTypeName>> {
        self.entity_type.as_ref()
    }
}

impl Typed for SimplifiedEntity {
    fn entity_types(&self) -> Option<&OneOrMany<EntityTypeName>> {
        Some(&self.entity_type)
    }
}

impl Typed for TemporalEntity<Output> {
    fn entity_types(&self) -> Option<&OneOrMany<EntityTypeName>> {
        Some(&self.entity_type)
    }
}

/// Matches a resource by its entity types.
#[derive(Debug, Clone, MatcherBase)]
pub struct TypeMatcher {
    /// The requirement on the types.
    types: SpellingMatcher<EntityTypeName>,
}

/// A resource whose type is `entity_type` written alone.
#[must_use]
pub fn has_type(entity_type: &EntityTypeName) -> TypeMatcher {
    TypeMatcher { types: alone(entity_type) }
}

/// A resource whose types are a list holding `entity_type`.
#[must_use]
pub fn lists_type(entity_type: &EntityTypeName) -> TypeMatcher {
    TypeMatcher { types: listing(entity_type) }
}

/// A resource whose types are a list of `count` types.
#[must_use]
pub const fn type_count(count: usize) -> TypeMatcher {
    TypeMatcher { types: listing_count(count) }
}

impl<R: Typed + Debug> Matcher<&R> for TypeMatcher {
    fn matches(&self, actual: &R) -> MatcherResult {
        actual.entity_types().map_or(MatcherResult::NoMatch, |types| self.types.matches(types))
    }

    fn describe(&self, result: MatcherResult) -> Description {
        format!("has types which {}", self.types.describe(result)).into()
    }
}

/// Matches a resource by whether it names a type at all.
#[derive(Debug, Clone, Copy, MatcherBase)]
pub struct TypePresence {
    /// Whether the resource must name a type.
    named: bool,
}

/// A resource naming a type, whichever.
#[must_use]
pub const fn any_type() -> TypePresence {
    TypePresence { named: true }
}

/// A resource naming no type, as a projection leaving out `type` answers an entity.
#[must_use]
pub const fn no_type() -> TypePresence {
    TypePresence { named: false }
}

impl<R: Typed + Debug> Matcher<&R> for TypePresence {
    fn matches(&self, actual: &R) -> MatcherResult {
        (actual.entity_types().is_some() == self.named).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match (result, self.named) {
            (MatcherResult::Match, true) | (MatcherResult::NoMatch, false) => "names a type".into(),
            (MatcherResult::Match, false) | (MatcherResult::NoMatch, true) => "names no type".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::type_matcher::{any_type, has_type, lists_type, no_type, type_count};
    use googletest::prelude::*;
    use ngsi_ld::model::{Entity, EntityTypeName, Lossless, OneOrMany, Output, Projected, SimplifiedEntity};

    /// An entity of `types`, as a broker answers it.
    fn entity(types: OneOrMany<EntityTypeName>) -> Result<Entity<Output>> {
        let mut entity = Entity::<Output>::parse(r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"}"#, Lossless::Normalized)?;
        entity.entity_type = types;
        Ok(entity)
    }

    #[gtest]
    fn a_type_written_alone_is_not_a_list_of_it() -> Result<()> {
        let vehicle: EntityTypeName = "Vehicle".parse()?;
        let alone = entity(OneOrMany::one(vehicle.clone()))?;
        let listed = entity(OneOrMany::try_from(vec![vehicle.clone()])?)?;
        expect_that!(&alone, has_type(&vehicle));
        expect_that!(&alone, not(lists_type(&vehicle)));
        expect_that!(&listed, lists_type(&vehicle));
        verify_that!(&listed, not(has_type(&vehicle)))
    }

    #[gtest]
    fn a_list_of_types_is_counted() -> Result<()> {
        let listed = entity(OneOrMany::try_from(vec!["Vehicle".parse()?, "Car".parse()?])?)?;
        expect_that!(&listed, type_count(2));
        expect_that!(&listed, lists_type(&"Car".parse()?));
        verify_that!(&listed, not(type_count(1)))
    }

    #[gtest]
    fn a_simplified_entity_is_matched_by_its_type() -> Result<()> {
        let vehicle: EntityTypeName = "Vehicle".parse()?;
        let simplified = SimplifiedEntity::parse(r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"}"#)?;
        expect_that!(&simplified, has_type(&vehicle));
        verify_that!(&simplified, not(lists_type(&vehicle)))
    }

    #[gtest]
    fn a_projected_entity_is_matched_by_whether_it_names_a_type() -> Result<()> {
        let vehicle: EntityTypeName = "Vehicle".parse()?;
        let typed = Entity::<Output, Projected>::parse(r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle"}"#, Lossless::Normalized)?;
        let untyped = Entity::<Output, Projected>::parse(r#"{"id":"urn:ngsi-ld:Vehicle:1"}"#, Lossless::Normalized)?;
        expect_that!(&typed, all!(any_type(), has_type(&vehicle)));
        expect_that!(&untyped, all!(no_type(), not(has_type(&vehicle))));
        verify_that!(&untyped, not(any_type()))
    }
}
