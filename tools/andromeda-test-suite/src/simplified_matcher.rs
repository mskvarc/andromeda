//! Matchers on the entities a broker answers in simplified representation (clause 4.5.4, `keyValues`), read through
//! the typed model: the value each attribute reduces to, a member inside it, and the attributes an entity lacks.
//!
//! A `GeoProperty` reduces to its geometry, which is read as the `GeoJSON` object it is written as, so its members are
//! reached by path like those of an object value. Values are compared by [`Equivalent`].

use crate::{attribute_reading::member_at, equivalence::Equivalent, geometry::geometry_value, value::object};
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use indexmap::IndexMap;
use ngsi_ld::model::{SimplifiedEntity, SimplifiedValue, Value};

/// The value the attribute `name` of `entity` reduces to, as a [`Value`] when it is a value, a geometry or a dataset
/// of them.
fn reduced(entity: &SimplifiedEntity, name: &str) -> Option<Value> {
    let value = entity
        .attributes
        .iter()
        .find_map(|(candidate, value)| (candidate.as_str() == name).then_some(value))?;
    reduction(value)
}

/// What a simplified value is written as, when it is a value, a geometry, or a `dataset` member of such values
/// keyed by their `datasetId` (clause 4.5.4).
fn reduction(value: &SimplifiedValue) -> Option<Value> {
    match value {
        SimplifiedValue::Value(value) => Some(value.clone()),
        SimplifiedValue::Geometry(geometry) => Some(geometry_value(geometry)),
        SimplifiedValue::Dataset(instances) => {
            let members = instances
                .iter()
                .map(|(dataset_id, instance)| reduction(instance).map(|reduced| (dataset_id.to_string(), reduced)))
                .collect::<Option<IndexMap<_, _>>>()?;
            Some(object([("dataset", Value::Object(members))]))
        }
        SimplifiedValue::LanguageMap(_) | SimplifiedValue::Vocab(_) | SimplifiedValue::Json(_) => None,
    }
}

/// Matches a simplified entity whose attribute `name` holds `expected` at `path` inside its reduced value.
#[derive(Debug, Clone, MatcherBase)]
pub struct HasValue {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The members leading into the value; empty for the value itself.
    path: Vec<String>,
    /// The value required there.
    expected: Value,
}

/// A simplified entity whose attribute `name` reduces to `expected`.
#[must_use]
pub fn has_value(name: &str, expected: impl Into<Value>) -> HasValue {
    HasValue {
        name: name.to_owned(),
        path: Vec::new(),
        expected: expected.into(),
    }
}

/// A simplified entity whose attribute `name` reduces to a value holding `expected` at `path`.
#[must_use]
pub fn has_value_at<const N: usize>(name: &str, path: [&str; N], expected: impl Into<Value>) -> HasValue {
    HasValue {
        name: name.to_owned(),
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        expected: expected.into(),
    }
}

impl Matcher<&SimplifiedEntity> for HasValue {
    fn matches(&self, actual: &SimplifiedEntity) -> MatcherResult {
        reduced(actual, &self.name)
            .is_some_and(|value| member_at(&value, &self.path).is_some_and(|found| found.equivalent(&self.expected)))
            .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        let place = if self.path.is_empty() {
            String::new()
        } else {
            format!(" at {:?}", self.path)
        };
        match result {
            MatcherResult::Match => format!("has {} holding {}{place}", self.name, self.expected).into(),
            MatcherResult::NoMatch => format!("hasn't {} holding {}{place}", self.name, self.expected).into(),
        }
    }
}

/// Matches a simplified entity without the attribute `name`.
#[derive(Debug, Clone, MatcherBase)]
pub struct LacksValue {
    /// The attribute name, as the answer's terms write it.
    name: String,
}

/// A simplified entity without the attribute `name`.
#[must_use]
pub fn lacks_value(name: &str) -> LacksValue {
    LacksValue { name: name.to_owned() }
}

impl Matcher<&SimplifiedEntity> for LacksValue {
    fn matches(&self, actual: &SimplifiedEntity) -> MatcherResult {
        actual.attributes.keys().all(|name| name.as_str() != self.name).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has no attribute {}", self.name).into(),
            MatcherResult::NoMatch => format!("has an attribute {}", self.name).into(),
        }
    }
}

/// Matches a simplified entity whose attribute `name` reduces to a value holding an array of `length` items at
/// `path`, as a `ListProperty`, a Relationship's objects or a geometry's coordinates are written (clause 4.5.4).
#[derive(Debug, Clone, MatcherBase)]
pub struct HasLength {
    /// The attribute name, as the answer's terms write it.
    name: String,
    /// The members leading into the array; empty for the value itself.
    path: Vec<String>,
    /// The number of items the array holds.
    length: usize,
}

/// A simplified entity whose attribute `name` reduces to a value holding an array of `length` items at `path`.
#[must_use]
pub fn has_length_at<const N: usize>(name: &str, path: [&str; N], length: usize) -> HasLength {
    HasLength {
        name: name.to_owned(),
        path: path.iter().map(|segment| (*segment).to_owned()).collect(),
        length,
    }
}

impl Matcher<&SimplifiedEntity> for HasLength {
    fn matches(&self, actual: &SimplifiedEntity) -> MatcherResult {
        reduced(actual, &self.name)
            .is_some_and(|value| matches!(member_at(&value, &self.path), Some(Value::Array(items)) if items.len() == self.length))
            .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("has {} holding {} items at {:?}", self.name, self.length, self.path).into(),
            MatcherResult::NoMatch => format!("hasn't {} holding {} items at {:?}", self.name, self.length, self.path).into(),
        }
    }
}

/// The number of members the document of a simplified entity holds: `id`, `type`, each attribute, and `scope` and
/// `@context` when present (clause 4.5.4).
#[must_use]
pub fn simplified_member_count(entity: &SimplifiedEntity) -> usize {
    let optional = [entity.scope.is_some(), entity.context.is_some()];
    2 + entity.attributes.len() + optional.iter().filter(|present| **present).count()
}

#[cfg(test)]
mod tests {
    use crate::simplified_matcher::{has_length_at, has_value, has_value_at, lacks_value, simplified_member_count};
    use googletest::prelude::*;
    use ngsi_ld::model::SimplifiedEntity;

    /// A Building in simplified representation, as a broker answers it.
    fn building() -> Result<SimplifiedEntity> {
        Ok(SimplifiedEntity::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","height":12,"address":{"city":"Paris"},
            "location":{"type":"Point","coordinates":[2.29,48.85]}}"#,
        )?)
    }

    #[gtest]
    fn a_reduced_value_is_matched_whole_or_by_path() -> Result<()> {
        let entity = building()?;
        expect_that!(&entity, has_value("height", 12));
        expect_that!(&entity, not(has_value("height", 13)));
        expect_that!(&entity, has_value_at("address", ["city"], "Paris"));
        expect_that!(&entity, has_value_at("location", ["type"], "Point"));
        verify_that!(&entity, lacks_value("name"))
    }

    #[gtest]
    fn a_dataset_of_values_is_read_by_its_dataset_ids() -> Result<()> {
        let entity = SimplifiedEntity::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","locatedAt":{"dataset":{"urn:ngsi-ld:Dataset:a":"urn:ngsi-ld:City:1"}}}"#,
        )?;
        expect_that!(&entity, has_value_at("locatedAt", ["dataset", "urn:ngsi-ld:Dataset:a"], "urn:ngsi-ld:City:1"));
        verify_that!(
            &entity,
            not(has_value_at("locatedAt", ["dataset", "urn:ngsi-ld:Dataset:b"], "urn:ngsi-ld:City:1"))
        )
    }

    #[gtest]
    fn an_array_is_counted_in_the_value_or_at_a_path() -> Result<()> {
        let entity = SimplifiedEntity::parse(
            r#"{"id":"urn:ngsi-ld:Building:1","type":"Building","tags":["a","b","c"],"location":{"type":"Point","coordinates":[2.29,48.85]}}"#,
        )?;
        expect_that!(&entity, has_length_at("tags", [], 3));
        expect_that!(&entity, not(has_length_at("tags", [], 2)));
        expect_that!(&entity, has_length_at("location", ["coordinates"], 2));
        verify_that!(&entity, not(has_length_at("location", ["type"], 1)))
    }

    #[gtest]
    fn a_simplified_entity_counts_its_id_type_and_attributes() -> Result<()> {
        verify_that!(simplified_member_count(&building()?), eq(5))
    }
}
