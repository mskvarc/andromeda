//! Where a `q` hop reads the entities a Relationship links to (clause 4.9).
//!
//! The `LinkedEntityRelation` production, written `rel{…}`, "makes a sub-query
//! on matching target objects" (clause 4.9 EXAMPLE 13): the entities the
//! Relationship's objects name, which the entity under evaluation does not
//! hold. Whoever evaluates a filter in memory therefore hands the evaluator a
//! source of those entities beside the entity itself.

use crate::queried_entity::{
    attribute_reading::{QueriedEntity, QueriedInstance, QueriedValueNode},
    instance_metadata::InstanceMetadata,
    value_item::ValueItem,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use std::{borrow::Cow, iter::empty};

/// The entities a hop may cross into, looked up by the Relationship object
/// naming them.
pub trait LinkedEntities<'a>: Copy {
    /// One linked entity, read the way the evaluator reads any entity.
    type Entity: QueriedEntity<'a>;

    /// The entity `object` names, or `None` when the source holds no entity
    /// by that identifier: a hop then reaches nothing through that object.
    fn linked(self, object: &str) -> Option<Self::Entity>;
}

/// A source holding no linked entity, for an evaluation whose hops are
/// answered elsewhere or cannot occur.
///
/// A store compiles every hop of a query's `q` itself, and the Context Source
/// Properties a `csf` reads carry no Relationship; an in-memory evaluation of
/// either reaches nothing through a hop.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoLinkedEntities;

/// The entity a [`NoLinkedEntities`] source never yields, uninhabited.
#[derive(Clone, Copy, Debug)]
pub enum NoLinkedEntity {}

impl LinkedEntities<'_> for NoLinkedEntities {
    type Entity = NoLinkedEntity;

    fn linked(self, _object: &str) -> Option<NoLinkedEntity> {
        None
    }
}

impl<'a> QueriedEntity<'a> for NoLinkedEntity {
    type Instance = NoLinkedEntity;

    fn attribute_instances(self, _attribute: &AttributeName) -> impl Iterator<Item = NoLinkedEntity> {
        empty()
    }

    fn types(self) -> impl Iterator<Item = &'a EntityTypeName> {
        empty()
    }
}

impl<'a> QueriedInstance<'a> for NoLinkedEntity {
    type Node = NoLinkedEntity;

    fn family(self) -> AttributeType {
        match self {}
    }

    fn values(self) -> impl Iterator<Item = ValueItem<'a, NoLinkedEntity>> {
        empty()
    }

    fn language_values(self, _language: &str) -> impl Iterator<Item = ValueItem<'a, NoLinkedEntity>> {
        empty()
    }

    fn sub_attribute(self, _name: &AttributeName) -> Option<NoLinkedEntity> {
        match self {}
    }

    fn metadata(self, _member: InstanceMetadata) -> Option<Cow<'a, str>> {
        match self {}
    }
}

impl<'a> QueriedValueNode<'a> for NoLinkedEntity {
    fn id(self) -> Option<&'a str> {
        match self {}
    }

    fn first_member_value(self, _member: &AttributeName) -> Option<ValueItem<'a, NoLinkedEntity>> {
        match self {}
    }
}

#[cfg(test)]
mod tests {
    use crate::queried_entity::linked_entities::{LinkedEntities, NoLinkedEntities};

    #[test]
    fn a_source_holding_no_linked_entity_links_nothing() {
        assert!(NoLinkedEntities.linked("urn:ngsi-ld:Parking:1").is_none());
    }
}
