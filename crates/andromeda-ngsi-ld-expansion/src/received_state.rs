//! An entity received from a peer, read into the model under the context it
//! was written in (clauses 4.3.6.6, 5.7.1.4, 5.7.2.4, 5.8.6).
//!
//! A context source answers, and a broker notifies, in compact JSON-LD spelled
//! under the context of the request it answers. Read under that context, the
//! entity becomes the context-independent model every local entity already
//! is, so answers from holders that spell one attribute differently merge on
//! the model by expanded name, and are compacted once, under the client's
//! context, for the response.
//!
//! Linked Entities a holder nested under a Relationship's `entity` or a
//! `ListRelationship`'s `entityList` (clause 4.5.23 `join=inline`) are output
//! members no write carries, so they are lifted out before the entity is
//! expanded and read the same way, each into a received state of its own; the
//! Relationship keeps its targets.

use crate::{entity_expansion::EntityExpansion, error::ExpansionError, normalization::Normalization};
use andromeda_ngsi_ld_entity::{document::EntityDocument, entity_state::EntityState, refusal::EntityRefusal};
use andromeda_ngsi_ld_model::{
    attribute::{
        instance::{Attribute, AttributeValue},
        one_or_many::OneOrMany,
    },
    entity::Entity,
    entity_system_times::EntitySystemTimes,
};
use std::{future::Future, pin::Pin};
use thiserror::Error;

/// An entity a peer answered with, in the model.
#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedState {
    /// The entity.
    pub state: EntityState,
    /// Its entity-level system times.
    pub system_times: EntitySystemTimes,
    /// The Linked Entities the peer nested under its Relationships, read the
    /// same way.
    pub linked: Vec<ReceivedState>,
}

/// An entity a peer notified, in the model, its notified deletions kept
/// (clause 5.8.6): a notification reports a deleted attribute with the
/// NGSI-LD Null, which no state can hold.
#[derive(Clone, Debug, PartialEq)]
pub struct ReceivedDocument {
    /// The entity as the written document it expands to.
    pub document: EntityDocument,
    /// Its entity-level system times.
    pub system_times: EntitySystemTimes,
}

/// Why an entity a peer sent is not read into the model.
#[derive(Debug, Error)]
pub enum ReceivedRefusal {
    /// The entity does not expand under the context it was written in.
    #[error("the entity does not expand under its context: {0}")]
    Expansion(#[from] ExpansionError),
    /// The entity expands to a document no entity can be (no `id`, no type,
    /// an NGSI-LD Null, a repeated `datasetId`).
    #[error(transparent)]
    Refused(#[from] EntityRefusal),
}

impl EntityExpansion<'_> {
    /// Read an entity a peer answered with into the model under this
    /// expansion's context, its nested Linked Entities lifted out and read
    /// alongside it.
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedRefusal`] when the entity, or one Linked Entity it
    /// nests, does not expand or is no entity.
    pub fn received(&self, entity: Entity) -> Pin<Box<dyn Future<Output = Result<ReceivedState, ReceivedRefusal>> + Send + '_>> {
        Box::pin(async move {
            let mut entity = entity;
            let nested = lift_linked_entities(&mut entity);
            let system_times = EntitySystemTimes::of(&entity);
            let document = self.entity(&entity, Normalization::Entity).await?;
            let state = EntityState::try_from(document)?;
            let mut linked = Vec::with_capacity(nested.len());
            for linked_entity in nested {
                // A Linked Entity is nested at most as deep as the join that
                // reached it, which the request bounds.
                linked.push(self.received(linked_entity).await?);
            }
            Ok(ReceivedState { state, system_times, linked })
        })
    }

    /// Read an entity a peer notified into the model under this expansion's
    /// context, a first-level NGSI-LD Null kept as the deletion of its
    /// member (clause 5.8.6).
    ///
    /// # Errors
    ///
    /// Returns [`ReceivedRefusal::Expansion`] when the entity does not expand.
    pub async fn notified(&self, entity: &Entity) -> Result<ReceivedDocument, ReceivedRefusal> {
        let document = self.entity(entity, Normalization::MemberDeletions).await?;
        Ok(ReceivedDocument {
            document,
            system_times: EntitySystemTimes::of(entity),
        })
    }
}

/// Take every Linked Entity `entity` nests under a Relationship's `entity` or
/// a `ListRelationship`'s `entityList` out of it, at every attribute and
/// sub-attribute.
fn lift_linked_entities(entity: &mut Entity) -> Vec<Entity> {
    let mut lifted = Vec::new();
    let members = entity
        .attrs
        .values_mut()
        .chain(entity.location.iter_mut())
        .chain(entity.observation_space.iter_mut())
        .chain(entity.operation_space.iter_mut());
    for member in members {
        lift_from_member(member, &mut lifted);
    }
    lifted
}

/// Take the nested Linked Entities out of every instance of one member.
fn lift_from_member(member: &mut AttributeValue, lifted: &mut Vec<Entity>) {
    for instance in member.instances_mut() {
        let carried = match instance {
            Attribute::Relationship(relationship) => relationship.entity.take(),
            Attribute::ListRelationship(list) => list.entity_list.take(),
            Attribute::Property(_)
            | Attribute::GeoProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::VocabProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::ListProperty(_) => None,
        };
        lifted.extend(carried.into_iter().flat_map(OneOrMany::into_vec));
        for sub_attribute in instance.common_mut().sub_attributes.values_mut() {
            lift_from_member(sub_attribute, lifted);
        }
    }
}
