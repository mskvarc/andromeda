//! The entities an inline join nests in an answer (clause 4.5.23.2): a Relationship holds them in its `entity` and a
//! `ListRelationship` in its `entityList`, each a projected entity since the request's projection applies to them
//! too, so a test checks them with the entity and identity matchers.

use ngsi_ld::model::{Attribute, Completeness, Entity, EntityId, OneOrMany, Output, Projected};
use std::iter;

/// The linked entities an inline join nested at the end of `path`, a chain of Relationship names: the first hop is
/// read on `entity`, each further one on the first entity the previous hop nested. The targets of a Relationship
/// come in the order of its `object`, of a `ListRelationship` in the order of its `objectList`; empty when a hop names
/// no such attribute or the join nested nothing under it.
#[must_use]
pub fn linked_entities<'e, C: Completeness>(entity: &'e Entity<Output, C>, path: &[&str]) -> &'e [Entity<Output, Projected>] {
    match path.split_first() {
        None => &[],
        Some((name, [])) => nested(entity, name),
        Some((name, rest)) => nested(entity, name).first().map_or(&[], |linked| linked_entities(linked, rest)),
    }
}

/// Whether the last Relationship of `path` (read as [`linked_entities`] reads it) wrote its linked entity as one
/// object rather than an array, as a Relationship with one object does (clause 4.5.23.2).
#[must_use]
pub fn linked_entity_alone<C: Completeness>(entity: &Entity<Output, C>, path: &[&str]) -> bool {
    match path.split_first() {
        None => false,
        Some((name, [])) => matches!(first_instance(entity, name), Some(Attribute::Relationship(relationship))
            if relationship.output().entity.as_ref().is_some_and(|linked| !linked.is_many())),
        Some((name, rest)) => nested(entity, name).first().is_some_and(|linked| linked_entity_alone(linked, rest)),
    }
}

/// The entities an inline join nested along the Relationship `name` followed hop by hop: the first hop is read on
/// `entity`, each further one on the first entity the previous hop nested, until a hop nests none. A chain of
/// Relationships nests as deep as `joinLevel` asks (clause 4.5.23.2).
fn chain<'e, C: Completeness>(entity: &'e Entity<Output, C>, name: &'e str) -> impl Iterator<Item = &'e Entity<Output, Projected>> {
    iter::successors(nested(entity, name).first(), move |linked| nested(linked, name).first())
}

/// How many entities an inline join nested along the Relationship `name` of `entity`, followed hop by hop.
#[must_use]
pub fn chain_length<C: Completeness>(entity: &Entity<Output, C>, name: &str) -> usize {
    chain(entity, name).count()
}

/// The entity an inline join nested `depth` hops down the Relationship `name` of `entity` (1 for the entity the first
/// hop nests), or `None` when the chain ends above it.
#[must_use]
pub fn chain_entity<'e, C: Completeness>(entity: &'e Entity<Output, C>, name: &'e str, depth: usize) -> Option<&'e Entity<Output, Projected>> {
    chain(entity, name).nth(depth.checked_sub(1)?)
}

/// The objects of the first instance of the Relationship `name` of `entity`, whether written alone or in an array;
/// empty when the entity holds no such Relationship.
#[must_use]
pub fn relationship_objects<'e, C: Completeness>(entity: &'e Entity<Output, C>, name: &str) -> &'e [EntityId] {
    if let Some(Attribute::Relationship(relationship)) = first_instance(entity, name) {
        relationship.object().as_slice()
    } else {
        &[]
    }
}

/// The first instance of the attribute `name` of `entity`.
fn first_instance<'e, C: Completeness>(entity: &'e Entity<Output, C>, name: &str) -> Option<&'e Attribute<Output>> {
    entity
        .attributes
        .iter()
        .find_map(|(candidate, instances)| (candidate.as_str() == name).then_some(instances.first()))
}

/// The linked entities the first instance of the attribute `name` of `entity` nests.
fn nested<'e, C: Completeness>(entity: &'e Entity<Output, C>, name: &str) -> &'e [Entity<Output, Projected>] {
    match first_instance(entity, name) {
        Some(Attribute::Relationship(relationship)) => relationship.output().entity.as_ref().map_or(&[], OneOrMany::as_slice),
        Some(Attribute::ListRelationship(relationship)) => relationship.output().entity_list.as_deref().unwrap_or_default(),
        Some(
            Attribute::Property(_)
            | Attribute::GeoProperty(_)
            | Attribute::LanguageProperty(_)
            | Attribute::VocabProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::JsonProperty(_),
        )
        | None => &[],
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_matcher::any_attribute,
        entity_matcher::has_attribute,
        identity_matcher::has_id,
        linked_entity::{chain_entity, chain_length, linked_entities, linked_entity_alone, relationship_objects},
        nth::nth,
    };
    use googletest::prelude::*;
    use ngsi_ld::model::{Entity, EntityId, Lossless, Output};

    #[gtest]
    fn a_relationship_nests_its_linked_entity() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","locatedAt":{"type":"Relationship","object":"urn:ngsi-ld:Building:1",
            "entity":{"id":"urn:ngsi-ld:Building:1","type":"Building","height":{"type":"Property","value":12}}}}"#,
            Lossless::Normalized,
        )?;
        let building: EntityId = "urn:ngsi-ld:Building:1".parse()?;
        let linked = linked_entities(&entity, &["locatedAt"]);
        expect_that!(linked_entity_alone(&entity, &["locatedAt"]), eq(true));
        expect_that!(linked, nth(0, has_id(&building)));
        verify_that!(linked, nth(0, has_attribute("height", any_attribute().value(12))))
    }

    #[gtest]
    fn a_relationship_with_several_objects_nests_its_targets_in_order() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","linkedTo":{"type":"Relationship","object":["urn:ngsi-ld:Building:2","urn:ngsi-ld:Building:1"],
            "entity":[{"id":"urn:ngsi-ld:Building:2","type":"Building"},{"id":"urn:ngsi-ld:Building:1","type":"Building"}]}}"#,
            Lossless::Normalized,
        )?;
        let second: EntityId = "urn:ngsi-ld:Building:2".parse()?;
        let linked = linked_entities(&entity, &["linkedTo"]);
        expect_that!(linked_entity_alone(&entity, &["linkedTo"]), eq(false));
        expect_that!(linked, len(eq(2)));
        verify_that!(linked, nth(0, has_id(&second)))
    }

    #[gtest]
    fn the_objects_of_a_relationship_are_read_alone_or_in_an_array() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","one":{"type":"Relationship","object":"urn:ngsi-ld:Building:1"},
            "many":{"type":"Relationship","object":["urn:ngsi-ld:Building:2"]}}"#,
            Lossless::Normalized,
        )?;
        let first: EntityId = "urn:ngsi-ld:Building:1".parse()?;
        let second: EntityId = "urn:ngsi-ld:Building:2".parse()?;
        expect_that!(relationship_objects(&entity, "one"), elements_are![eq(&first)]);
        expect_that!(relationship_objects(&entity, "many"), elements_are![eq(&second)]);
        verify_that!(relationship_objects(&entity, "absent"), is_empty())
    }

    #[gtest]
    fn a_relationship_without_a_join_nests_nothing() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","linkedTo":{"type":"Relationship","object":"urn:ngsi-ld:Building:1"}}"#,
            Lossless::Normalized,
        )?;
        expect_that!(linked_entities(&entity, &["linkedTo"]), is_empty());
        verify_that!(linked_entities(&entity, &["absent", "linkedTo"]), is_empty())
    }

    #[gtest]
    fn a_second_hop_is_read_on_the_first_linked_entity() -> Result<()> {
        let entity = Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","locatedAt":{"type":"Relationship","object":"urn:ngsi-ld:City:1",
            "entity":{"id":"urn:ngsi-ld:City:1","type":"City","isInCountry":{"type":"Relationship","object":"urn:ngsi-ld:Country:1",
            "entity":{"id":"urn:ngsi-ld:Country:1","type":"Country"}}}}}"#,
            Lossless::Normalized,
        )?;
        let country: EntityId = "urn:ngsi-ld:Country:1".parse()?;
        expect_that!(linked_entity_alone(&entity, &["locatedAt", "isInCountry"]), eq(true));
        verify_that!(linked_entities(&entity, &["locatedAt", "isInCountry"]), nth(0, has_id(&country)))
    }

    #[gtest]
    fn a_chain_thousands_of_hops_deep_is_counted_and_walked_to_its_end() -> Result<()> {
        let depth = 3000;
        let opening = (0..depth)
            .map(|hop| {
                format!(
                    r#"{{"id":"urn:ngsi-ld:Building:{hop}","type":"Building","linkedTo":{{"type":"Relationship","object":"urn:ngsi-ld:Building:{}","entity":"#,
                    hop + 1
                )
            })
            .collect::<Vec<_>>()
            .concat();
        let text = format!(r#"{opening}{{"id":"urn:ngsi-ld:Building:{depth}","type":"Building"}}{}"#, "}}".repeat(depth));
        let entity = Entity::<Output>::parse(&text, Lossless::Normalized)?;
        let last: EntityId = format!("urn:ngsi-ld:Building:{depth}").parse()?;
        expect_that!(chain_length(&entity, "linkedTo"), eq(depth));
        expect_that!(chain_length(&entity, "absent"), eq(0));
        expect_that!(chain_entity(&entity, "linkedTo", 0), none());
        expect_that!(chain_entity(&entity, "linkedTo", depth + 1), none());
        verify_that!(chain_entity(&entity, "linkedTo", depth), some(has_id(&last)))
    }
}
