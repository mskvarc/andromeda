//! The equivalence of typed NGSI-LD documents: equality up to the spellings the specification reads the same.
//!
//! The typed model keeps every value as it was written, so a document round-trips: a `DateTime` keeps its fraction
//! digits and a number its lexical form. A broker that answers `2020-09-09T16:40:00Z` for a request that wrote
//! `2020-09-09T16:40:00.000Z` answers the same instant (clause 4.6.3), and `4.0` is the number `4` (RFC 8259 section
//! 6). Here such spellings are equivalent; every other member is compared exactly. The instances of one attribute are
//! paired by `datasetId` (clause 4.5.5), and attributes by name, so neither order matters.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use indexmap::IndexMap;
use ngsi_ld::model::{
    Attribute,
    AttributeMeta,
    Attributes,
    DateTime,
    Entity,
    GeoProperty,
    Input,
    Instances,
    JsonProperty,
    LanguageProperty,
    ListProperty,
    ListRelationship,
    Number,
    Property,
    Relationship,
    SimplifiedEntity,
    SimplifiedValue,
    TypedValue,
    Value,
    VocabProperty,
};
use std::{fmt::Debug, hash::Hash};

/// A value compared up to the spellings the specification reads the same.
pub trait Equivalent {
    /// Whether `self` and `other` say the same.
    fn equivalent(&self, other: &Self) -> bool;
}

impl Equivalent for DateTime {
    fn equivalent(&self, other: &Self) -> bool {
        self.timestamp() == other.timestamp()
    }
}

impl Equivalent for Number {
    fn equivalent(&self, other: &Self) -> bool {
        match (self.as_i64(), other.as_i64()) {
            (Some(left), Some(right)) => left == right,
            (Some(_) | None, None) | (None, Some(_)) => self.as_f64().total_cmp(&other.as_f64()).is_eq(),
        }
    }
}

impl Equivalent for TypedValue {
    fn equivalent(&self, other: &Self) -> bool {
        if self.datatype() != other.datatype() {
            return false;
        }
        match (self.lexical().parse::<DateTime>(), other.lexical().parse::<DateTime>()) {
            (Ok(left), Ok(right)) => left.equivalent(&right),
            (Ok(_) | Err(_), Err(_)) | (Err(_), Ok(_)) => self.lexical() == other.lexical(),
        }
    }
}

impl<T: Equivalent> Equivalent for Option<T> {
    fn equivalent(&self, other: &Self) -> bool {
        match (self, other) {
            (Some(left), Some(right)) => left.equivalent(right),
            (None, None) => true,
            (Some(_), None) | (None, Some(_)) => false,
        }
    }
}

impl<T: Equivalent> Equivalent for [T] {
    fn equivalent(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().zip(other).all(|(left, right)| left.equivalent(right))
    }
}

impl Equivalent for Value {
    fn equivalent(&self, other: &Self) -> bool {
        match self {
            Value::Boolean(left) => matches!(other, Value::Boolean(right) if left == right),
            Value::Number(left) => matches!(other, Value::Number(right) if left.equivalent(right)),
            Value::Text(left) => matches!(other, Value::Text(right) if left == right),
            Value::Array(left) => matches!(other, Value::Array(right) if left.equivalent(right)),
            Value::Object(left) => matches!(other, Value::Object(right) if left.equivalent(right)),
            Value::Typed(left) => matches!(other, Value::Typed(right) if left.equivalent(right)),
            Value::JsonNull => matches!(other, Value::JsonNull),
        }
    }
}

impl Equivalent for AttributeMeta<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.dataset_id == other.dataset_id
            && self.observed_at.equivalent(&other.observed_at)
            && self.expires_at.equivalent(&other.expires_at)
            && self.proof == other.proof
            && self.attributes.equivalent(&other.attributes)
    }
}

impl Equivalent for Property<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.value().equivalent(other.value())
            && self.unit_code() == other.unit_code()
            && self.value_type() == other.value_type()
            && self.meta().equivalent(other.meta())
    }
}

impl Equivalent for GeoProperty<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.value() == other.value()
            && self.unit_code() == other.unit_code()
            && self.value_type() == other.value_type()
            && self.meta().equivalent(other.meta())
    }
}

impl Equivalent for LanguageProperty<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.language_map() == other.language_map() && self.value_type() == other.value_type() && self.meta().equivalent(other.meta())
    }
}

impl Equivalent for VocabProperty<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.vocab() == other.vocab() && self.value_type() == other.value_type() && self.meta().equivalent(other.meta())
    }
}

impl Equivalent for ListProperty<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.value_list().equivalent(other.value_list())
            && self.unit_code() == other.unit_code()
            && self.value_type() == other.value_type()
            && self.meta().equivalent(other.meta())
    }
}

impl Equivalent for JsonProperty<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.json() == other.json() && self.value_type() == other.value_type() && self.meta().equivalent(other.meta())
    }
}

impl Equivalent for Relationship<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.object() == other.object() && self.object_type() == other.object_type() && self.meta().equivalent(other.meta())
    }
}

impl Equivalent for ListRelationship<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.object_list() == other.object_list() && self.object_type() == other.object_type() && self.meta().equivalent(other.meta())
    }
}

impl Equivalent for Attribute<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        match self {
            Attribute::Property(left) => matches!(other, Attribute::Property(right) if left.equivalent(right)),
            Attribute::GeoProperty(left) => matches!(other, Attribute::GeoProperty(right) if left.equivalent(right)),
            Attribute::LanguageProperty(left) => matches!(other, Attribute::LanguageProperty(right) if left.equivalent(right)),
            Attribute::VocabProperty(left) => matches!(other, Attribute::VocabProperty(right) if left.equivalent(right)),
            Attribute::ListProperty(left) => matches!(other, Attribute::ListProperty(right) if left.equivalent(right)),
            Attribute::JsonProperty(left) => matches!(other, Attribute::JsonProperty(right) if left.equivalent(right)),
            Attribute::Relationship(left) => matches!(other, Attribute::Relationship(right) if left.equivalent(right)),
            Attribute::ListRelationship(left) => matches!(other, Attribute::ListRelationship(right) if left.equivalent(right)),
        }
    }
}

impl Equivalent for Instances<Attribute<Input>> {
    fn equivalent(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self.iter().all(|instance| {
                other
                    .iter()
                    .find(|candidate| candidate.meta().dataset_id == instance.meta().dataset_id)
                    .is_some_and(|candidate| instance.equivalent(candidate))
            })
    }
}

impl Equivalent for Attributes<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.len() == other.len()
            && self
                .iter()
                .all(|(name, instances)| other.get(name).is_some_and(|candidate| instances.equivalent(candidate)))
    }
}

impl Equivalent for Entity<Input> {
    fn equivalent(&self, other: &Self) -> bool {
        self.id == other.id
            && self.entity_type == other.entity_type
            && self.scope == other.scope
            && self.expires_at.equivalent(&other.expires_at)
            && self.attributes.equivalent(&other.attributes)
            && self.context == other.context
    }
}

impl<K: Hash + Eq, V: Equivalent> Equivalent for IndexMap<K, V> {
    fn equivalent(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|(key, value)| other.get(key).is_some_and(|member| value.equivalent(member)))
    }
}

impl Equivalent for SimplifiedValue {
    fn equivalent(&self, other: &Self) -> bool {
        match self {
            SimplifiedValue::Value(left) => matches!(other, SimplifiedValue::Value(right) if left.equivalent(right)),
            SimplifiedValue::Geometry(left) => matches!(other, SimplifiedValue::Geometry(right) if left == right),
            SimplifiedValue::LanguageMap(left) => matches!(other, SimplifiedValue::LanguageMap(right) if left == right),
            SimplifiedValue::Vocab(left) => matches!(other, SimplifiedValue::Vocab(right) if left == right),
            SimplifiedValue::Json(left) => matches!(other, SimplifiedValue::Json(right) if left == right),
            SimplifiedValue::Dataset(left) => matches!(other, SimplifiedValue::Dataset(right) if left.equivalent(right)),
        }
    }
}

impl Equivalent for SimplifiedEntity {
    fn equivalent(&self, other: &Self) -> bool {
        self.id == other.id
            && self.entity_type == other.entity_type
            && self.scope == other.scope
            && self.attributes.equivalent(&other.attributes)
            && self.context == other.context
    }
}

/// Matches a value equivalent to the expected one.
#[derive(MatcherBase)]
pub struct EquivalentMatcher<'e, T> {
    /// The value the actual one must be equivalent to.
    expected: &'e T,
}

/// A value equivalent to `expected`: equal up to the spelling of instants and numbers.
#[must_use]
pub const fn equivalent_to<T>(expected: &T) -> EquivalentMatcher<'_, T> {
    EquivalentMatcher { expected }
}

impl<T: Equivalent + Debug> Matcher<&T> for EquivalentMatcher<'_, T> {
    fn matches(&self, actual: &T) -> MatcherResult {
        actual.equivalent(self.expected).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is equivalent to {:#?}", self.expected).into(),
            MatcherResult::NoMatch => format!("isn't equivalent to {:#?}", self.expected).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::equivalence::{Equivalent, equivalent_to};
    use cefact_units::UnitCode;
    use googletest::prelude::*;
    use ngsi_ld::model::{DateTime, Entity, Input, Number, Property, Value};

    /// A Building whose `airQualityLevel` is `level`, observed at `observed_at`.
    fn building(level: &str, observed_at: &str) -> Result<Entity<Input>> {
        let level: Number = level.parse()?;
        let air_quality_level = Property::builder(Value::Number(level))
            .unit_code(UnitCode::C62)
            .observed_at(observed_at.parse()?);
        Ok(Entity::builder()
            .id("urn:ngsi-ld:Building:1")
            .entity_type("Building")
            .property("airQualityLevel", air_quality_level)
            .build()?)
    }

    #[gtest]
    fn an_instant_written_with_other_fraction_digits_is_equivalent() -> Result<()> {
        let written: DateTime = "2020-09-09T16:40:00.000Z".parse()?;
        let answered: DateTime = "2020-09-09T16:40:00Z".parse()?;
        expect_that!(written, not(eq(answered)));
        verify_that!(&answered, equivalent_to(&written))
    }

    #[gtest]
    fn another_instant_is_not_equivalent() -> Result<()> {
        let written: DateTime = "2020-09-09T16:40:00.000Z".parse()?;
        let later: DateTime = "2020-09-09T16:40:00.001Z".parse()?;
        verify_that!(&later, not(equivalent_to(&written)))
    }

    #[gtest]
    fn a_number_written_as_a_float_is_equivalent_to_the_integer() -> Result<()> {
        let integer: Number = "4".parse()?;
        let float: Number = "4.0".parse()?;
        expect_that!(integer.equivalent(&float), eq(true));
        verify_that!(integer.equivalent(&"5".parse()?), eq(false))
    }

    #[gtest]
    fn entities_differing_only_in_spellings_are_equivalent() -> Result<()> {
        let written = building("4", "2020-09-09T16:40:00.000Z")?;
        let answered = building("4.0", "2020-09-09T16:40:00Z")?;
        verify_that!(&answered, equivalent_to(&written))
    }

    #[gtest]
    fn an_entity_with_another_value_is_not_equivalent() -> Result<()> {
        let written = building("4", "2020-09-09T16:40:00.000Z")?;
        let answered = building("5", "2020-09-09T16:40:00.000Z")?;
        verify_that!(&answered, not(equivalent_to(&written)))
    }

    #[gtest]
    fn an_entity_missing_an_attribute_is_not_equivalent() -> Result<()> {
        let written = building("4", "2020-09-09T16:40:00.000Z")?;
        let bare = Entity::builder().id("urn:ngsi-ld:Building:1").entity_type("Building").build()?;
        verify_that!(&bare, not(equivalent_to(&written)))
    }
}
