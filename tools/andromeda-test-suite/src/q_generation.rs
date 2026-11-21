//! Entities and `q` filters drawn from a seed, for a differential of Query Entities against Subscriptions.
//!
//! The fixed corpus states what each of its filters selects. A generated case states nothing: it draws Entities and
//! filters from a seed and expects Query Entities and a Subscription with each filter to select the same Entities,
//! whatever those are, so a filter no corpus case thought of can still find the two executions disagreeing. Every
//! seed draws the same case on every run.
//!
//! An Entity holds one to four Attributes named from a pool of eight, of every attribute family: a Property holding a number, a string, a Boolean, an array of numbers, a declared Date, `DateTime` or Time, an `xsd:double` (not-a-number and the infinities among them), a JSON object or an array of JSON objects, with a second instance in a dataset, a sub-property, an observation time or none of them; a `GeoProperty` holding a Point or a `LineString`; a `JsonProperty` whose object, or array of objects, holds the JSON `null` among other values; a Relationship with one object or several; a `ListRelationship`, a `ListProperty`, a `VocabProperty` or a `LanguageProperty`. A filter is up to four Query Terms over the same names, joined by `;` and `|` and grouped up to two deep, with every operator and every operand type, ranges and value lists among them. A term reads a name, its sub-property or observation time, a trailing path into its value, a language of it, or a member of its own representation named as if it were a sub-Attribute, and the filter is asked with `jsonKeys` and `expandValues` naming some of the names.

pub mod attribute_strategy;
pub mod drawn_attribute;
pub mod drawn_filter;
pub mod drawn_pool;

use crate::{
    q_generation::{
        attribute_strategy::drawn_entity,
        drawn_attribute::DrawnAttribute,
        drawn_filter::{DrawnFilter, drawn_filter},
    },
    seeded_draw::drawn_from_seed,
};
use googletest::prelude::*;
use ngsi_ld::model::{DatasetId, Entity, EntityId, EntityTypeName, Input};
use proptest::collection::vec;

/// A drawn case: Entities and the filters asked of them.
#[derive(Debug, Clone)]
pub struct GeneratedCase {
    /// The Attributes of each Entity.
    entities: Vec<Vec<DrawnAttribute>>,
    /// The filters.
    filters: Vec<DrawnFilter>,
}

impl GeneratedCase {
    /// The case `seed` draws: `entities` Entities and `filters` filters.
    ///
    /// # Errors
    ///
    /// Fails when the generator rejects its draw.
    pub fn drawn(seed: u64, entities: usize, filters: usize) -> Result<GeneratedCase> {
        let (entities, filters) = drawn_from_seed(seed, &(vec(drawn_entity(), entities), vec(drawn_filter(), filters)))?;
        Ok(GeneratedCase { entities, filters })
    }

    /// How many Entities the case holds.
    #[must_use]
    pub const fn entity_count(&self) -> usize {
        self.entities.len()
    }

    /// The filters of the case.
    #[must_use]
    pub fn filters(&self) -> &[DrawnFilter] {
        &self.filters
    }

    /// The Attributes of the Entity at `position`, as the generator drew them, for a failure to name.
    #[must_use]
    pub fn described(&self, position: usize) -> Option<String> {
        self.entities.get(position).map(|attributes| format!("{attributes:?}"))
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
        for (attributes, id) in self.entities.iter().zip(ids) {
            let mut builder = Entity::builder().id(id.clone()).entity_type(entity_type.clone());
            for attribute in attributes {
                for instance in attribute.instances(dataset)? {
                    builder = builder.attribute(attribute.name, instance);
                }
            }
            documents.push(builder.build()?);
        }
        Ok(documents)
    }
}

#[cfg(test)]
mod tests {
    use crate::q_generation::{
        GeneratedCase,
        drawn_attribute::{DrawnFamily, DrawnJson, PropertyValue},
    };
    use googletest::prelude::*;
    use ngsi_ld::{
        model::{DatasetId, EntityId, EntityTypeName},
        query::q::Query,
    };
    use std::collections::BTreeSet;

    /// The seeds the suite draws its cases from.
    const SEEDS: u64 = 64;

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
            DrawnFamily::Geometry(_) => "geo-property",
            DrawnFamily::Json(DrawnJson::Object(members)) if members.iter().any(|member| member.value.is_none()) => "json-object-with-null",
            DrawnFamily::Json(DrawnJson::Objects(objects)) if objects.iter().flatten().any(|member| member.value.is_none()) => "json-objects-with-null",
            DrawnFamily::Json(_) => "json-property",
            DrawnFamily::Relationship(objects) if objects.len() > 1 => "relationship-to-several",
            DrawnFamily::Relationship(_) => "relationship",
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
        for seed in 0..SEEDS {
            for attributes in GeneratedCase::drawn(seed, 20, 30)?.entities {
                shapes.extend(attributes.iter().map(|attribute| shape(&attribute.family)));
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
                "geo-property",
                "json-object-with-null",
                "json-objects-with-null",
                "json-property",
                "language-property",
                "list-property",
                "list-relationship",
                "number",
                "object",
                "relationship",
                "relationship-to-several",
                "string",
                "time",
                "vocab-property",
            ]))
        )
    }

    #[gtest]
    fn the_suite_seeds_draw_every_operator_path_and_operand_form() -> Result<()> {
        let mut filters = String::new();
        for seed in 0..SEEDS {
            for filter in GeneratedCase::drawn(seed, 20, 30)?.filters {
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
            "2.5",
            "-1",
        ] {
            expect_that!(filters, contains_substring(form), "no filter writes {form}");
        }
        Ok(())
    }

    #[gtest]
    fn every_drawn_filter_is_a_query_and_every_drawn_entity_builds() -> Result<()> {
        let entity_type: EntityTypeName = "Drawn".parse()?;
        let dataset: DatasetId = "urn:ngsi-ld:Dataset:drawn".parse()?;
        let ids = (0..20)
            .map(|position| format!("urn:ngsi-ld:Drawn:{position}").parse())
            .collect::<Result<Vec<EntityId>, _>>()?;
        for seed in 0..SEEDS {
            let case = GeneratedCase::drawn(seed, 20, 30)?;
            for filter in case.filters() {
                expect_that!(filter.q.parse::<Query>(), ok(anything()), "seed {seed}: {}", filter.q);
            }
            expect_that!(case.documents(&entity_type, &dataset, &ids), ok(len(eq(20))), "seed {seed}");
        }
        Ok(())
    }
}
