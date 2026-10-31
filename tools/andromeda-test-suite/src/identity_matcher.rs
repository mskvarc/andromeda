//! Matchers on the identity of the resources a broker answers: one resource's id, and the ids of a page in their
//! order. Entities, projected and simplified entities, temporal entities and their simplified form, context source
//! registrations and subscriptions are each identified by their own id type; a projected entity may have none.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::{
    model::{
        Entity,
        EntityId,
        Output,
        Projected,
        SimplifiedEntity,
        evolution::{SimplifiedTemporalEntity, TemporalEntity},
    },
    registration::{CSourceRegistration, RegistrationId},
    subscription::{Subscription, SubscriptionId, SubscriptionKind},
};
use std::fmt::{Debug, Display};

/// A resource a broker answers with the id it is known by.
pub trait Identified {
    /// The type of the id.
    type Id: PartialEq + Display + Debug;

    /// The resource's id; `None` for a projected entity whose `pick` or `omit` dropped it (clause 4.21).
    fn identifier(&self) -> Option<&Self::Id>;
}

impl Identified for Entity<Output> {
    type Id = EntityId;

    fn identifier(&self) -> Option<&EntityId> {
        Some(&self.id)
    }
}

impl Identified for Entity<Output, Projected> {
    type Id = EntityId;

    fn identifier(&self) -> Option<&EntityId> {
        self.id.as_ref()
    }
}

impl Identified for SimplifiedEntity {
    type Id = EntityId;

    fn identifier(&self) -> Option<&EntityId> {
        Some(&self.id)
    }
}

impl Identified for TemporalEntity<Output> {
    type Id = EntityId;

    fn identifier(&self) -> Option<&EntityId> {
        Some(&self.id)
    }
}

impl Identified for TemporalEntity<Output, Projected> {
    type Id = EntityId;

    fn identifier(&self) -> Option<&EntityId> {
        self.id.as_ref()
    }
}

impl Identified for SimplifiedTemporalEntity {
    type Id = EntityId;

    fn identifier(&self) -> Option<&EntityId> {
        Some(&self.id)
    }
}

impl Identified for CSourceRegistration<Output> {
    type Id = RegistrationId;

    fn identifier(&self) -> Option<&RegistrationId> {
        Some(self.id())
    }
}

impl<K: SubscriptionKind> Identified for Subscription<Output, K> {
    type Id = SubscriptionId;

    fn identifier(&self) -> Option<&SubscriptionId> {
        Some(self.id())
    }
}

/// Matches a resource with the id `id`.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasId<I> {
    /// The id.
    id: I,
}

/// A resource whose id is `id`.
#[must_use]
pub fn has_id<I: Clone>(id: &I) -> HasId<I> {
    // Clone: the matcher outlives the borrow of the test's id.
    HasId { id: id.clone() }
}

impl<'r, R: Identified + Debug> Matcher<&'r R> for HasId<R::Id> {
    fn matches(&self, actual: &'r R) -> MatcherResult {
        (actual.identifier() == Some(&self.id)).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has the id {}", self.id).into(),
            MatcherResult::NoMatch => format!("hasn't the id {}", self.id).into(),
        }
    }
}

/// Matches a list of resources whose ids are exactly a given list, in its order.
#[derive(Debug, Clone, MatcherBase)]
pub struct IdsInOrder<I> {
    /// The ids, in order.
    ids: Vec<I>,
}

/// A list of resources whose ids are `ids`, in that order and no others.
#[must_use]
pub fn ids_in_order<I: Clone>(ids: &[I]) -> IdsInOrder<I> {
    // Clone: the matcher outlives the borrow of the test's ids.
    IdsInOrder { ids: ids.to_vec() }
}

impl<'r, R: Identified + Debug> Matcher<&'r [R]> for IdsInOrder<R::Id> {
    fn matches(&self, actual: &'r [R]) -> MatcherResult {
        actual.iter().map(Identified::identifier).eq(self.ids.iter().map(Some)).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("holds the resources {:?} in order", self.ids).into(),
            MatcherResult::NoMatch => format!("doesn't hold the resources {:?} in order", self.ids).into(),
        }
    }

    fn explain_match(&self, actual: &'r [R]) -> Description {
        let mismatch = actual
            .iter()
            .map(Identified::identifier)
            .zip(&self.ids)
            .enumerate()
            .find(|(_, (found, expected))| *found != Some(*expected));
        match mismatch {
            Some((index, (Some(found), expected))) => format!("which holds {found} at {index} where {expected} is expected").into(),
            Some((index, (None, expected))) => format!("which holds an entity without id at {index} where {expected} is expected").into(),
            None => format!("which holds {} resources, not {}", actual.len(), self.ids.len()).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::identity_matcher::{has_id, ids_in_order};
    use googletest::prelude::*;
    use ngsi_ld::model::{Entity, EntityId, Lossless, Output, Projected, SimplifiedEntity, evolution::SimplifiedTemporalEntity};
    use std::slice;

    /// A Building as a broker answers it.
    fn building() -> Result<Entity<Output>> {
        Ok(Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building"}"#,
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn ids_in_order_requires_every_id_in_its_place() -> Result<()> {
        let entities = [building()?];
        let id: EntityId = "urn:ngsi-ld:Building:1".parse()?;
        let other: EntityId = "urn:ngsi-ld:Building:2".parse()?;
        expect_that!(entities.as_slice(), ids_in_order(slice::from_ref(&id)));
        expect_that!(entities.as_slice(), not(ids_in_order(slice::from_ref(&other))));
        expect_that!(&entities[0], has_id(&id));
        verify_that!(entities.as_slice(), not(ids_in_order(&[id.clone(), id])))
    }

    #[gtest]
    fn a_simplified_temporal_entity_is_matched_by_its_id() -> Result<()> {
        let id: EntityId = "urn:ngsi-ld:Building:1".parse()?;
        let other: EntityId = "urn:ngsi-ld:Building:2".parse()?;
        let building = SimplifiedTemporalEntity::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","height":{"type":"Property","values":[[1,"2024-01-01T00:00:00Z"]]}}"#,
        )?;
        expect_that!(&building, has_id(&id));
        verify_that!(&building, not(has_id(&other)))
    }

    #[gtest]
    fn a_simplified_entity_is_matched_by_its_id() -> Result<()> {
        let id: EntityId = "urn:ngsi-ld:Building:1".parse()?;
        let building = SimplifiedEntity::parse(r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","name":"Eiffel"}"#)?;
        verify_that!(&building, has_id(&id))
    }

    #[gtest]
    fn a_projected_entity_is_matched_by_the_id_it_kept() -> Result<()> {
        let id: EntityId = "urn:ngsi-ld:Building:1".parse()?;
        let kept = Entity::<Output, Projected>::parse(r#"{"id":"urn:ngsi-ld:Building:1","speed":{"type":"Property","value":5}}"#, Lossless::Normalized)?;
        let dropped = Entity::<Output, Projected>::parse(r#"{"speed":{"type":"Property","value":5}}"#, Lossless::Normalized)?;
        expect_that!(&kept, has_id(&id));
        expect_that!(&dropped, not(has_id(&id)));
        verify_that!([kept, dropped].as_slice(), not(ids_in_order(slice::from_ref(&id))))
    }
}
