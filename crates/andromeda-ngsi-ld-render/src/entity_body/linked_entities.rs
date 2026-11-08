//! The Linked Entities a clause 4.5.23 `join=inline` nests into the
//! Relationships of a typed entity.
//!
//! Clause 4.5.23.2 nests the entity a Relationship's `object` targets under the
//! Relationship's `entity` member (one entity for a single target URI, the
//! ordered array of them for an array of target URIs), and the entities a
//! `ListRelationship`'s `objectList` targets under its `entityList` member, each
//! written in the representation of the entity that carries it (clauses
//! 4.5.3.3, 4.5.22.3 and 4.5.4). Which copy is nested under which Relationship
//! instance, under which rule, and where nesting stops is decided by the join
//! plan; this walk only spells what the plan's cursor answers.
//!
//! An entity that already carries nested entities (a Context Source answered
//! with them) has them written in the same representation, as they were
//! carried. A nested entity is Entity data in the same payload, so the clause
//! 4.3.6.8 version fallback the carrying entity is written under reaches it.

use crate::{
    entity_body::{
        member_walk::MemberWalk,
        typed_entity::{IdentifierMember, TypedEntity},
    },
    temporal::{temporal_entity_body::TemporalEntityBody, temporal_evolutions::TemporalEvolutions},
};
use andromeda_linked_retrieval::{plan::OpenEntity, typed::TypedEntities};
use andromeda_native_stack::on_sufficient_stack;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{
    attribute::{list_relationship::ListRelationship, one_or_many::OneOrMany, relationship::Relationship},
    entity::Entity,
    temporal::temporal_body::TemporalBody,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use serde::{Serialize, Serializer};

/// The inline join one top-level attribute nests its targets by: the entity
/// carrying it, open on the join's path, and the attribute's key.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AttributeJoin<'a> {
    /// An Entity of a Core read or a notification.
    Entity {
        /// The entity carrying the attribute.
        open: &'a OpenEntity<'a, TypedEntities>,
        /// The attribute's key on that entity.
        attribute: &'a str,
    },
    /// The Temporal Evolution of an Entity, behind the temporal Linked Entity
    /// retrieval extension.
    Temporal {
        /// The evolution carrying the attribute.
        open: &'a OpenEntity<'a, TemporalEvolutions>,
        /// The attribute's key on that evolution.
        attribute: &'a str,
    },
}

/// A Linked Entity written beside, or nested in, the entity that names it.
#[derive(Debug)]
pub(crate) enum LinkedEntity<'a> {
    /// An Entity.
    Entity(TypedEntity<'a>),
    /// The Temporal Evolution of an Entity.
    Temporal(TemporalEntityBody<'a>),
}

impl Serialize for LinkedEntity<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            LinkedEntity::Entity(entity) => entity.serialize(serializer),
            LinkedEntity::Temporal(evolution) => evolution.serialize(serializer),
        }
    }
}

impl<'a> AttributeJoin<'a> {
    /// The copy of the entity `target` names that the plan nests here, written
    /// in `format` under its own rule, or `None` when `target` stays a plain
    /// URI.
    fn linked_entity(&self, target: &Uri, format: ResponseFormat, walk: MemberWalk<'a>) -> Option<LinkedEntity<'a>> {
        match *self {
            AttributeJoin::Entity { open, attribute } => {
                let (entity, cursor) = open.nested(attribute, target)?;
                Some(LinkedEntity::Entity(TypedEntity {
                    entity,
                    format,
                    walk: MemberWalk::new(walk.language_filter, walk.fallback),
                    projection: None,
                    identifier: IdentifierMember::Written,
                    context: None,
                    stamped: None,
                    join: Some(cursor),
                    narrowing: cursor.rule().restricting(),
                    changes: None,
                }))
            }
            AttributeJoin::Temporal { open, attribute } => {
                let (body, cursor) = open.nested(attribute, target)?;
                // An aggregated evolution names no Relationship instance, so
                // none is nested; the join is refused with those formats.
                let TemporalBody::Evolution(entity) = body else {
                    return None;
                };
                Some(LinkedEntity::Temporal(TemporalEntityBody::nested(
                    entity,
                    MemberWalk::new(walk.language_filter, walk.fallback),
                    cursor,
                )))
            }
        }
    }

    /// Every nested copy of `targets`, in the order they are named.
    fn linked_entities<'t>(&self, targets: impl IntoIterator<Item = &'t Uri>, format: ResponseFormat, walk: MemberWalk<'a>) -> Option<NestedEntities<'a>> {
        let nested: Vec<LinkedEntity<'a>> = targets.into_iter().filter_map(|target| self.linked_entity(target, format, walk)).collect();
        (!nested.is_empty()).then_some(NestedEntities::Many(nested))
    }
}

/// The Linked Entities one Relationship or `ListRelationship` instance nests.
#[derive(Debug)]
pub(crate) enum NestedEntities<'a> {
    /// The one entity a single target URI names.
    One(Box<LinkedEntity<'a>>),
    /// The ordered entities an array of targets names.
    Many(Vec<LinkedEntity<'a>>),
}

impl Serialize for NestedEntities<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The one point the walk recurses per nested Linked Entity. Clause
        // 4.5.23 sets no `joinLevel` cap, so the nesting depth is the data's,
        // and each level moves to a fresh stack segment once the current one
        // runs low.
        on_sufficient_stack(|| match self {
            NestedEntities::One(entity) => entity.serialize(serializer),
            NestedEntities::Many(entities) => serializer.collect_seq(entities),
        })
    }
}

/// The entities a Relationship's `entity` member holds in `format`: those its
/// targets resolved to under the walk's join, or else those it carries.
///
/// The member mirrors how `object` is spelled rather than how many targets
/// resolved: one URI nests one entity, an array of URIs nests an array.
pub(crate) fn relationship_entities<'a>(relationship: &'a Relationship, format: ResponseFormat, walk: MemberWalk<'a>) -> Option<NestedEntities<'a>> {
    let resolved = walk.join.and_then(|join| match &relationship.object {
        OneOrMany::One(target) => join.linked_entity(target, format, walk).map(|entity| NestedEntities::One(Box::new(entity))),
        OneOrMany::Many(targets) => join.linked_entities(targets, format, walk),
    });
    resolved.or_else(|| relationship.entity.as_ref().map(|carried| carried_entities(carried, format, walk)))
}

/// The entities a `ListRelationship`'s `entityList` member holds in `format`:
/// those its targets resolved to under the walk's join, or else those it
/// carries.
pub(crate) fn list_relationship_entities<'a>(
    list_relationship: &'a ListRelationship,
    format: ResponseFormat,
    walk: MemberWalk<'a>,
) -> Option<NestedEntities<'a>> {
    let resolved = walk.join.and_then(|join| join.linked_entities(&list_relationship.object_list, format, walk));
    resolved.or_else(|| list_relationship.entity_list.as_ref().map(|carried| carried_entities(carried, format, walk)))
}

/// The entities a `ListRelationship` reformatted as a Relationship nests under
/// `entity` (Table 4.3.6.8-1): those its targets resolved to under the walk's
/// join, or else those it carries.
///
/// The member mirrors the `object` the list is written as, as a
/// Relationship's does: one target nests one entity, several nest the ordered
/// array of them.
pub(crate) fn list_as_relationship_entities<'a>(
    list_relationship: &'a ListRelationship,
    format: ResponseFormat,
    walk: MemberWalk<'a>,
) -> Option<NestedEntities<'a>> {
    let resolved = walk.join.and_then(|join| match list_relationship.object_list.as_slice() {
        [only] => join.linked_entity(only, format, walk).map(|entity| NestedEntities::One(Box::new(entity))),
        targets => join.linked_entities(targets, format, walk),
    });
    resolved.or_else(|| list_relationship.entity_list.as_ref().map(|carried| carried_entities(carried, format, walk)))
}

/// Entities a Relationship already carries, written in `format` as they were
/// carried.
fn carried_entities<'a>(carried: &'a OneOrMany<Entity>, format: ResponseFormat, walk: MemberWalk<'a>) -> NestedEntities<'a> {
    match carried {
        OneOrMany::One(entity) => NestedEntities::One(Box::new(carried_entity(entity, format, walk))),
        OneOrMany::Many(entities) => NestedEntities::Many(entities.iter().map(|entity| carried_entity(entity, format, walk)).collect()),
    }
}

/// One entity a Relationship already carries, written in `format`.
const fn carried_entity<'a>(entity: &'a Entity, format: ResponseFormat, walk: MemberWalk<'a>) -> LinkedEntity<'a> {
    LinkedEntity::Entity(TypedEntity {
        entity,
        format,
        walk: MemberWalk::new(None, walk.fallback),
        projection: None,
        identifier: IdentifierMember::Written,
        context: None,
        stamped: None,
        join: None,
        narrowing: None,
        changes: None,
    })
}

#[cfg(test)]
mod tests;
