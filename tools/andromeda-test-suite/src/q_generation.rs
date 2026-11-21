//! Entities and `q` filters drawn from a seed, for a differential of Query Entities against Subscriptions.
//!
//! The fixed corpus states what each of its filters selects. A generated case states nothing: it draws Entities and
//! filters from a seed and expects Query Entities and a Subscription with each filter to select the same Entities,
//! whatever those are, so a filter no corpus case thought of can still find the two executions disagreeing. Every
//! seed draws the same case on every run. Each Entity is written twice, created and then changed by a second write
//! that appends Attributes to it, and the two executions are compared after each.
//!
//! An Entity holds one to seven Attributes named from a pool of eight, of every attribute family: a Property holding a number, a string, a Boolean, an array of numbers, a declared Date, `DateTime` or Time, an `xsd:double` (not-a-number and the infinities among them), a JSON object or an array of JSON objects, with a second instance in a dataset, a sub-property, an observation time or none of them; a `GeoProperty` holding a point, a line or a polygon of the geometries the generated geoqueries share; a `JsonProperty` whose object, or array of objects, holds the JSON `null` among other values; a Relationship with one object or several, or a `ListRelationship`, whose objects are URIs of a pool or Entities of the case at lower positions; a `ListProperty`, a `VocabProperty` or a `LanguageProperty`. Each name is drawn with a family of its own three times as often as with any family, so that several Entities hold a name alike.
//!
//! The filters are drawn after the Entities, each from one of them, its anchor, the Entities taken in turn, after its creation or after its change. A filter is up to four Query Terms joined by `;` and `|` and grouped up to two deep. Most terms are anchored: a path into an Attribute the anchor holds and a comparison its value satisfies, which holds for that value alone or for the range, ordering, pattern or inequality it falls in, of every attribute family, or the same through a Relationship to an Attribute of the Linked Entity, with or without a type hint. Now and then an anchored term takes the opposite polarity. The other terms are free, every operator over every path form and operand type of the pools, a sub-property or observation time, a trailing path, a language, or a member of an Attribute's own representation named as if it were a sub-Attribute. A filter is asked with `jsonKeys` and `expandValues` naming what its terms need and some names beside, with `join` and `joinLevel` when it reads through a Relationship, and with a geoquery over the anchor's `GeoProperty` now and then. No two filters of a case are the same.
//!
//! The weights favour comparisons that hold for some of the values of a pool and not for others, and `|` over `;`, over comparisons that hold for the drawn value alone or for every value an Entity can hold under a name, so that most filters select an Entity, the filters that select any most often select several, and two filters of a case rarely select the same Entities.

pub mod anchored_term;
pub mod attribute_strategy;
pub mod drawn_attribute;
pub mod drawn_entity;
pub mod drawn_filter;
pub mod drawn_object;
pub mod drawn_pool;
pub mod drawn_query;
pub mod free_term;
pub mod held_term;
pub mod member_term;
pub mod placeholder;
pub mod scalar_comparison;

use crate::{
    q_generation::{
        attribute_strategy::drawn_entity,
        drawn_entity::DrawnEntity,
        drawn_filter::{DrawnFilter, drawn_filter},
    },
    seeded_draw::drawn_from_seed,
};
use googletest::prelude::*;
use ngsi_ld::model::{DatasetId, Entity, EntityFragment, EntityId, EntityTypeName, Input};
use proptest::prelude::{BoxedStrategy, Just, Strategy};
use std::sync::Arc;

/// How many filters a case draws for each it asks, so that enough remain once the repeated ones are dropped.
const CANDIDATES_PER_FILTER: usize = 2;

/// A drawn case: Entities and the filters asked of them.
#[derive(Debug, Clone)]
pub struct GeneratedCase {
    /// Each Entity, at its position.
    entities: Arc<[DrawnEntity]>,
    /// The filters, no two the same.
    filters: Vec<DrawnFilter>,
}

impl GeneratedCase {
    /// The case `seed` draws: `entities` Entities and `filters` filters, the first `filters` distinct ones of twice as
    /// many drawn. The filters take the Entities in turn as their anchors, so that no Entity anchors many more filters
    /// than another and two filters drawn from one Entity, which tend to select alike, are rare.
    ///
    /// # Errors
    ///
    /// Fails when the case holds no Entity, the generator rejects its draw, or fewer than `filters` of the filters
    /// drawn are distinct.
    pub fn drawn(seed: u64, entities: usize, filters: usize) -> Result<GeneratedCase> {
        if entities == 0 {
            fail!("a case of no Entity has no anchor to draw a filter from")?;
        }
        let strategy = (0..entities)
            .map(drawn_entity)
            .collect::<Vec<BoxedStrategy<DrawnEntity>>>()
            .prop_flat_map(move |drawn| {
                let drawn: Arc<[DrawnEntity]> = drawn.into();
                let candidates: Vec<BoxedStrategy<DrawnFilter>> = (0..filters * CANDIDATES_PER_FILTER)
                    .map(|index| drawn_filter(Arc::clone(&drawn), index % entities))
                    .collect();
                (Just(drawn), candidates)
            });
        let (entities, candidates) = drawn_from_seed(seed, &strategy)?;
        Ok(GeneratedCase {
            entities,
            filters: distinct(seed, candidates, filters)?,
        })
    }

    /// How many Entities the case holds.
    #[must_use]
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// The filters of the case.
    #[must_use]
    pub fn filters(&self) -> &[DrawnFilter] {
        &self.filters
    }

    /// The Attributes of the Entity at `position` and its change, as the generator drew them, for a failure to name.
    #[must_use]
    pub fn described(&self, position: usize) -> Option<String> {
        self.entities
            .get(position)
            .map(|entity| format!("created with {:?}, then appended {:?}", entity.attributes, entity.change))
    }

    /// The Entities of the case, of `entity_type`, the one at each position identified by `ids` at that position,
    /// a second instance of a Property in `dataset`.
    ///
    /// # Errors
    ///
    /// Fails when there are fewer identifiers than Entities, or a drawn value or Entity does not build.
    pub fn documents(&self, entity_type: &EntityTypeName, dataset: &DatasetId, ids: &[EntityId]) -> Result<Vec<Entity<Input>>> {
        if ids.len() < self.entities.len() {
            fail!("{} identifiers for {} entities", ids.len(), self.entities.len())?;
        }
        let mut documents = Vec::with_capacity(self.entities.len());
        for (entity, id) in self.entities.iter().zip(ids) {
            let mut builder = Entity::builder().id(id.clone()).entity_type(entity_type.clone());
            for attribute in &entity.attributes {
                for instance in attribute.instances(dataset, ids)? {
                    builder = builder.attribute(attribute.name, instance);
                }
            }
            documents.push(builder.build()?);
        }
        Ok(documents)
    }

    /// The change of each Entity, at its position, as the fragment Append Entity Attributes takes, a second instance
    /// of a Property in `dataset` and `ids` identifying the Entity at each position.
    ///
    /// # Errors
    ///
    /// Fails when a drawn value or fragment does not build.
    pub fn changes(&self, dataset: &DatasetId, ids: &[EntityId]) -> Result<Vec<EntityFragment>> {
        let mut changes = Vec::with_capacity(self.entities.len());
        for entity in self.entities.iter() {
            let mut builder = EntityFragment::builder();
            for attribute in &entity.change {
                for instance in attribute.instances(dataset, ids)? {
                    builder = builder.attribute(attribute.name, instance);
                }
            }
            changes.push(builder.build()?);
        }
        Ok(changes)
    }
}

/// The first `wanted` distinct filters of `candidates`, which the case `seed` drew.
///
/// # Errors
///
/// Fails when fewer than `wanted` of them are distinct.
fn distinct(seed: u64, candidates: Vec<DrawnFilter>, wanted: usize) -> Result<Vec<DrawnFilter>> {
    let mut kept: Vec<DrawnFilter> = Vec::with_capacity(wanted);
    for candidate in candidates {
        if kept.len() == wanted {
            break;
        }
        if !kept.contains(&candidate) {
            kept.push(candidate);
        }
    }
    if kept.len() < wanted {
        fail!("seed {seed} drew {} distinct filters, fewer than the {wanted} its case asks", kept.len())?;
    }
    Ok(kept)
}

#[cfg(test)]
mod tests {
    use crate::{
        geo_generation::pooled_geometry::Outline,
        q_generation::{
            GeneratedCase,
            drawn_attribute::{DrawnFamily, DrawnJson, PropertyValue},
            drawn_object::DrawnObject,
        },
    };
    use andromeda_q_corpus::corpus::Traversal;
    use googletest::prelude::*;
    use ngsi_ld::{
        model::{DatasetId, EntityId, EntityTypeName},
        query::q::Query,
    };
    use std::collections::{BTreeSet, HashSet};

    /// The seeds the suite draws its cases from.
    const SEEDS: u64 = 64;

    /// The cases of the suite seeds, of 20 Entities and 30 filters each.
    fn suite_cases() -> Result<Vec<GeneratedCase>> {
        (0..SEEDS).map(|seed| GeneratedCase::drawn(seed, 20, 30)).collect()
    }

    /// Whether `q` reads through a Relationship: a name followed by the brace opening a Linked Entity path, where a
    /// brace of a pattern follows a bracket or an atom.
    fn reads_through_a_relationship(q: &str) -> bool {
        q.as_bytes()
            .windows(2)
            .any(|pair| matches!(pair, [before, b'{'] if before.is_ascii_alphanumeric()))
    }

    /// Whether `objects` name an Entity of the case.
    fn names_an_entity(objects: &[DrawnObject]) -> bool {
        objects.iter().any(|object| object.position().is_some())
    }

    /// Names each Attribute shape a case can hold, so that a shape added to the generator is also counted here.
    fn shape(family: &DrawnFamily) -> &'static str {
        match family {
            DrawnFamily::Property(value, _) => match value {
                PropertyValue::Number(_) => "number",
                PropertyValue::Text(_) => "string",
                PropertyValue::Boolean(_) => "boolean",
                PropertyValue::Numbers(_) => "array",
                PropertyValue::Date(_) => "date",
                PropertyValue::DateTime(_) => "date-time",
                PropertyValue::Time(_) => "time",
                PropertyValue::Double(_) => "double",
                PropertyValue::Structured(_) => "object",
                PropertyValue::Structures(_) => "array-of-objects",
            },
            DrawnFamily::Geometry(geometry) => match geometry.outline() {
                Outline::Point(..) => "geo-point",
                Outline::LineString(_) => "geo-line-string",
                Outline::Polygon(_) => "geo-polygon",
            },
            DrawnFamily::Json(DrawnJson::Object(members)) if members.iter().any(|member| member.value.is_none()) => "json-object-with-null",
            DrawnFamily::Json(DrawnJson::Objects(objects)) if objects.iter().flatten().any(|member| member.value.is_none()) => "json-objects-with-null",
            DrawnFamily::Json(_) => "json-property",
            DrawnFamily::Relationship(objects) if names_an_entity(objects) => "relationship-to-an-earlier-entity",
            DrawnFamily::Relationship(objects) if objects.len() > 1 => "relationship-to-several",
            DrawnFamily::Relationship(_) => "relationship",
            DrawnFamily::ListRelationship(objects) if names_an_entity(objects) => "list-relationship-to-an-earlier-entity",
            DrawnFamily::ListRelationship(_) => "list-relationship",
            DrawnFamily::ListProperty(_) => "list-property",
            DrawnFamily::Vocab(_) => "vocab-property",
            DrawnFamily::Language(_, _) => "language-property",
        }
    }

    #[gtest]
    fn a_seed_draws_the_same_case_on_every_run() -> Result<()> {
        let first = GeneratedCase::drawn(7, 20, 30)?;
        let second = GeneratedCase::drawn(7, 20, 30)?;
        verify_that!(format!("{second:?}"), eq(&format!("{first:?}")))
    }

    #[gtest]
    fn the_suite_seeds_draw_every_attribute_shape() -> Result<()> {
        let mut shapes = BTreeSet::new();
        for case in suite_cases()? {
            for entity in case.entities.iter() {
                shapes.extend(entity.attributes.iter().chain(&entity.change).map(|attribute| shape(&attribute.family)));
            }
        }
        verify_that!(
            shapes,
            eq(&BTreeSet::from([
                "array",
                "array-of-objects",
                "boolean",
                "date",
                "date-time",
                "double",
                "geo-line-string",
                "geo-point",
                "geo-polygon",
                "json-object-with-null",
                "json-objects-with-null",
                "json-property",
                "language-property",
                "list-property",
                "list-relationship",
                "list-relationship-to-an-earlier-entity",
                "number",
                "object",
                "relationship",
                "relationship-to-an-earlier-entity",
                "relationship-to-several",
                "string",
                "time",
                "vocab-property",
            ]))
        )
    }

    #[gtest]
    fn every_object_naming_an_entity_of_the_case_names_one_at_a_lower_position() -> Result<()> {
        for case in suite_cases()? {
            for (position, entity) in case.entities.iter().enumerate() {
                for attribute in entity.attributes.iter().chain(&entity.change) {
                    for target in attribute.family.objects().into_iter().flatten().filter_map(|object| object.position()) {
                        expect_that!(target, lt(position), "{attribute:?} of the Entity at {position}");
                    }
                }
            }
        }
        Ok(())
    }

    #[gtest]
    fn the_suite_seeds_draw_every_operator_path_and_operand_form() -> Result<()> {
        let mut filters = String::new();
        for case in suite_cases()? {
            for filter in case.filters() {
                filters.push_str(&filter.q);
                filters.push('\n');
            }
        }
        for form in [
            "==",
            "!=",
            ">",
            ">=",
            "<",
            "<=",
            "~=\"",
            "!~=\"",
            ";",
            "|",
            "((",
            ".sub",
            ".observedAt",
            "[a]",
            "[a.b]",
            "[*]",
            "[en]",
            "[fr]",
            "[type]",
            "[coordinates]",
            "[value]",
            ".value",
            ".object",
            ".languageMap",
            ".valueList",
            ".objectList",
            ".json",
            ".vocab",
            "..",
            "\"a\"..\"b\"",
            "08:30:00Z..",
            "==urn:x:",
            "\"urn:x:1\",\"urn:x:2\"",
            "true,false",
            "12:00:00Z",
            ".000Z",
            "(?i)",
            "2.5",
            "-1",
            "==@",
            "==\"@",
            "}==",
            "{@type@:",
            "{Other:",
        ] {
            expect_that!(filters, contains_substring(form), "no filter writes {form}");
        }
        Ok(())
    }

    #[gtest]
    fn a_filter_reads_through_a_relationship_exactly_when_it_is_asked_with_one_hop() -> Result<()> {
        let mut hops = 0_usize;
        for case in suite_cases()? {
            for filter in case.filters() {
                expect_that!(
                    reads_through_a_relationship(&filter.q),
                    eq(filter.traversal == Traversal::OneHop),
                    "{}",
                    filter.q
                );
                hops += usize::from(filter.traversal == Traversal::OneHop);
            }
        }
        verify_that!(hops, gt(0))
    }

    #[gtest]
    fn some_filters_carry_a_geoquery_over_a_geo_property() -> Result<()> {
        let mut carried = 0_usize;
        for case in suite_cases()? {
            for filter in case.filters() {
                expect_that!(filter.geo_query(), ok(anything()), "{filter:?}");
                carried += usize::from(filter.geo.is_some());
            }
        }
        verify_that!(carried, gt(0))
    }

    #[gtest]
    fn no_two_filters_of_a_suite_case_are_the_same() -> Result<()> {
        for (seed, case) in suite_cases()?.iter().enumerate() {
            for (position, filter) in case.filters().iter().enumerate() {
                let earlier = case.filters().iter().take(position).any(|other| other == filter);
                expect_false!(earlier, "seed {seed} draws {filter:?} twice");
            }
        }
        Ok(())
    }

    #[gtest]
    fn at_least_95_percent_of_the_q_texts_of_the_suite_seeds_are_distinct() -> Result<()> {
        let texts: Vec<String> = suite_cases()?.into_iter().flat_map(|case| case.filters).map(|filter| filter.q).collect();
        let distinct: HashSet<&String> = texts.iter().collect();
        expect_true!(distinct.len() * 100 >= texts.len() * 95, "{} distinct of {}", distinct.len(), texts.len());
        Ok(())
    }

    #[gtest]
    fn every_drawn_filter_is_a_query_and_every_drawn_entity_and_change_builds() -> Result<()> {
        let entity_type: EntityTypeName = "Drawn".parse()?;
        let dataset: DatasetId = "urn:ngsi-ld:Dataset:drawn".parse()?;
        let ids = (0..20)
            .map(|position| format!("urn:ngsi-ld:Drawn:{position}").parse())
            .collect::<Result<Vec<EntityId>, _>>()?;
        for (seed, case) in suite_cases()?.iter().enumerate() {
            for filter in case.filters() {
                let q = filter.resolved_q(&ids, &entity_type)?;
                expect_that!(q.parse::<Query>(), ok(anything()), "seed {seed}: {q}");
            }
            expect_that!(case.documents(&entity_type, &dataset, &ids), ok(len(eq(20))), "seed {seed}");
            expect_that!(case.changes(&dataset, &ids), ok(len(eq(20))), "seed {seed}");
        }
        Ok(())
    }
}
