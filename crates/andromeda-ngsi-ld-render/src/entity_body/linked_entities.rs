//! The Linked Entities a clause 4.5.23 `join=inline` nests into the
//! Relationships of a typed entity.
//!
//! Clause 4.5.23.2 nests the entity a Relationship's `object` targets under the
//! Relationship's `entity` member -- one entity for a single target URI, the
//! ordered array of them for an array of target URIs -- and the entities a
//! `ListRelationship`'s `objectList` targets under its `entityList` member, each
//! written in the representation of the entity that carries it (clauses
//! 4.5.3.3, 4.5.22.3 and 4.5.4). Only a target the traversal resolved is nested
//! (clause 4.5.23.1). Every copy is narrowed by the clause 4.21 sub-projection
//! the Relationship that reached it carried, so two Relationships naming one
//! target under different sub-projections nest two differently narrowed copies
//! of it. The nesting recurses into the linked entities' own Relationships,
//! except into a target already open on the path from the root: the linked
//! entities legitimately reference one another, and following a cycle would
//! never end.
//!
//! An entity that already carries nested entities -- a Context Source answered
//! with them -- has them written in the same representation, as they were
//! carried.

use crate::{
    entity_body::{
        member_walk::MemberWalk,
        typed_entity::{IdentifierMember, TypedEntity},
    },
    nested_projection::{LinkedProjection, NestedProjection},
};
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, uri::Uri};
use andromeda_ngsi_ld_model::{
    attribute::{list_relationship::ListRelationship, one_or_many::OneOrMany, relationship::Relationship},
    entity::Entity,
};
use andromeda_ngsi_ld_representation::response_format::ResponseFormat;
use indexmap::IndexMap;
use serde::{Serialize, Serializer};
use std::iter::successors;

/// The clause 4.5.23 inline join one entity is written under.
#[derive(Debug, Clone, Copy)]
pub(crate) struct EntityJoin<'a> {
    /// The entities the traversal resolved, keyed by identifier.
    linked: &'a IndexMap<Identifier, Entity>,
    /// The clause 4.21 sub-projection governing what this entity's
    /// Relationships link to.
    projection: NestedProjection<'a>,
    /// The entities open on the path down to this one.
    parent: Option<&'a JoinPath<'a>>,
}

impl<'a> EntityJoin<'a> {
    /// The join a response's own entity is written under: every entity
    /// `linked` holds may be nested, narrowed by the sub-projections of
    /// `projection`.
    pub(crate) fn root(linked: &'a IndexMap<Identifier, Entity>, projection: Option<LinkedProjection<'a>>) -> EntityJoin<'a> {
        EntityJoin {
            linked,
            projection: projection.map_or(NestedProjection::NONE, NestedProjection::root),
            parent: None,
        }
    }

    /// The entities open on the path above the entity this join writes.
    pub(crate) const fn parent(&self) -> Option<&'a JoinPath<'a>> {
        self.parent
    }

    /// The join the attribute keyed `name` nests its targets by, the entity
    /// carrying it open at `path`.
    pub(crate) fn attribute<'p>(&self, name: &str, path: &'p JoinPath<'p>) -> AttributeJoin<'p>
    where
        'a: 'p,
    {
        AttributeJoin {
            linked: self.linked,
            target: self.projection.descend(name),
            path,
        }
    }
}

/// The entities open on the path from the root down to the entity being
/// written, innermost first.
#[derive(Debug, Clone, Copy)]
pub(crate) struct JoinPath<'a> {
    /// The identifier of the entity being written.
    id: &'a Uri,
    /// The path above it.
    parent: Option<&'a JoinPath<'a>>,
}

impl<'a> JoinPath<'a> {
    /// The entity `id`, open below `parent`.
    pub(crate) const fn new(id: &'a Uri, parent: Option<&'a JoinPath<'a>>) -> JoinPath<'a> {
        JoinPath { id, parent }
    }

    /// Whether `target` is open anywhere on the path.
    fn holds(&self, target: &Uri) -> bool {
        successors(Some(self), |path| path.parent).any(|path| path.id == target)
    }
}

/// The inline join one top-level attribute nests its targets by.
#[derive(Debug, Clone, Copy)]
pub(crate) struct AttributeJoin<'a> {
    /// The entities the traversal resolved, keyed by identifier.
    linked: &'a IndexMap<Identifier, Entity>,
    /// The sub-projection the attribute carried to whatever it links to.
    target: NestedProjection<'a>,
    /// The path down to the entity carrying the attribute.
    path: &'a JoinPath<'a>,
}

impl<'a> AttributeJoin<'a> {
    /// The resolved entity `target` names, written in `format` and narrowed
    /// by the attribute's sub-projection, or `None` when the traversal did not
    /// resolve it.
    fn linked_entity(&self, target: &Uri, format: ResponseFormat, walk: MemberWalk<'a>) -> Option<TypedEntity<'a>> {
        let entity = self.linked.get(target)?;
        let restricted = !self.target.is_unrestricted();
        Some(TypedEntity {
            entity,
            format,
            walk: MemberWalk::new(walk.decoding, walk.language_filter),
            projection: restricted.then(|| self.target.structural()),
            identifier: IdentifierMember::Written,
            context: None,
            stamped: None,
            join: (!self.path.holds(target)).then_some(EntityJoin {
                linked: self.linked,
                projection: self.target,
                parent: Some(self.path),
            }),
            narrowing: restricted.then_some(self.target),
            changes: None,
        })
    }

    /// Every resolved entity of `targets`, in the order they are named.
    fn linked_entities<'t>(&self, targets: impl IntoIterator<Item = &'t Uri>, format: ResponseFormat, walk: MemberWalk<'a>) -> Option<NestedEntities<'a>> {
        let nested: Vec<TypedEntity<'a>> = targets.into_iter().filter_map(|target| self.linked_entity(target, format, walk)).collect();
        (!nested.is_empty()).then_some(NestedEntities::Many(nested))
    }
}

/// The Linked Entities one Relationship or `ListRelationship` instance nests.
#[derive(Debug)]
pub(crate) enum NestedEntities<'a> {
    /// The one entity a single target URI names.
    One(Box<TypedEntity<'a>>),
    /// The ordered entities an array of targets names.
    Many(Vec<TypedEntity<'a>>),
}

impl Serialize for NestedEntities<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            NestedEntities::One(entity) => entity.serialize(serializer),
            NestedEntities::Many(entities) => serializer.collect_seq(entities),
        }
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

/// Entities a Relationship already carries, written in `format` as they were
/// carried.
fn carried_entities<'a>(carried: &'a OneOrMany<Entity>, format: ResponseFormat, walk: MemberWalk<'a>) -> NestedEntities<'a> {
    let written = |entity| TypedEntity {
        entity,
        format,
        walk: MemberWalk::new(walk.decoding, None),
        projection: None,
        identifier: IdentifierMember::Written,
        context: None,
        stamped: None,
        join: None,
        narrowing: None,
        changes: None,
    };
    match carried {
        OneOrMany::One(entity) => NestedEntities::One(Box::new(written(entity))),
        OneOrMany::Many(entities) => NestedEntities::Many(entities.iter().map(written).collect()),
    }
}
