//! Entities and `q` filters drawn from a seed, for a differential of Query Entities against Subscriptions.
//!
//! The fixed corpus states what each of its filters selects. A generated case states nothing: it draws Entities and
//! filters from a seed and expects Query Entities and a Subscription with each filter to select the same Entities,
//! whatever those are, so a filter no corpus case thought of can still find the two executions disagreeing. Every
//! seed draws the same case on every run.
//!
//! An Entity holds one to four Attributes named from a pool of eight, each a Property holding a number, a string, a
//! Boolean, an array of one or two numbers, a declared Date or `DateTime`, or a declared `xsd:double` (not-a-number
//! and the infinities among them), or a Relationship, a `ListRelationship`, a `ListProperty`, a `VocabProperty` or a
//! `LanguageProperty`, a Property with a second instance in a dataset or a sub-property or neither. A filter is up to
//! four Query Terms over the same names, joined by `;` and `|` and grouped up to two deep, every operator and operand
//! type among them.

use crate::value::xsd_typed;
use googletest::prelude::*;
use ngsi_ld::model::{
    DatasetId,
    Entity,
    EntityId,
    EntityTypeName,
    Input,
    LanguageMap,
    ListProperty,
    ListRelationship,
    Number,
    Property,
    Relationship,
    Value,
    VocabProperty,
    VocabTerm,
};
use proptest::{
    bool::ANY,
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
    strategy::ValueTree,
    test_runner::{Config, RngSeed, TestRunner},
};

/// The Attribute names an Entity and a filter are drawn over.
pub const NAMES: [&str; 8] = ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"];

/// The strings a value and an operand are drawn from.
const STRINGS: [&str; 4] = ["a", "b", "ab", "ba"];

/// The objects a Relationship is drawn from.
const OBJECTS: [&str; 3] = ["urn:x:1", "urn:x:2", "urn:x:3"];

/// The Dates a value and an operand are drawn from.
const DATES: [&str; 2] = ["2024-01-15", "2024-06-30"];

/// The `DateTime`s a value and an operand are drawn from.
const DATE_TIMES: [&str; 2] = ["2024-01-15T12:00:00Z", "2024-06-30T00:00:00Z"];

/// The `xsd:double` lexical forms a value is drawn from.
const DOUBLES: [&str; 4] = ["1.5", "NaN", "INF", "-INF"];

/// The vocabulary terms a `VocabProperty` holds.
const TERMS: [&str; 2] = ["Male", "Female"];

/// The patterns a filter matches strings with.
const PATTERNS: [&str; 3] = ["^a", "b$", "a.*"];

/// The value of one drawn Attribute.
#[derive(Debug, Clone)]
enum DrawnValue {
    /// A Property holding a number.
    Number(u8),
    /// A Property holding a string.
    Text(&'static str),
    /// A Property holding a Boolean.
    Boolean(bool),
    /// A Property holding an array of numbers.
    Numbers(Vec<u8>),
    /// A Property holding a declared Date.
    Date(&'static str),
    /// A Property holding a declared `DateTime`.
    DateTime(&'static str),
    /// A Property holding a declared `xsd:double`.
    Double(&'static str),
    /// A Relationship.
    Relationship(&'static str),
    /// A `ListRelationship`.
    ListRelationship(Vec<&'static str>),
    /// A `ListProperty` of numbers.
    ListProperty(Vec<u8>),
    /// A `VocabProperty` holding a term.
    Vocab(&'static str),
    /// A `LanguageProperty` with an English and a French string.
    Language(&'static str, &'static str),
}

/// What a drawn Property carries beside its value.
#[derive(Debug, Clone, Copy)]
enum Companion {
    /// Nothing.
    Alone,
    /// A second instance in a dataset, holding this number.
    DatasetInstance(u8),
    /// A sub-property `sub` holding this number.
    SubProperty(u8),
}

/// One drawn Attribute.
#[derive(Debug, Clone)]
struct DrawnAttribute {
    /// Its name, from [`NAMES`].
    name: &'static str,
    /// Its value.
    value: DrawnValue,
    /// What a Property carries beside its value.
    companion: Companion,
}

/// A drawn case: Entities and the filters asked of them.
#[derive(Debug, Clone)]
pub struct GeneratedCase {
    /// The Attributes of each Entity.
    entities: Vec<Vec<DrawnAttribute>>,
    /// The filters, as a client writes them.
    pub filters: Vec<String>,
}

/// The value of a drawn Attribute.
fn drawn_value() -> BoxedStrategy<DrawnValue> {
    prop_oneof![
        (0..10_u8).prop_map(DrawnValue::Number),
        select(&STRINGS[..]).prop_map(DrawnValue::Text),
        ANY.prop_map(DrawnValue::Boolean),
        vec(0..10_u8, 1..3).prop_map(DrawnValue::Numbers),
        select(&DATES[..]).prop_map(DrawnValue::Date),
        select(&DATE_TIMES[..]).prop_map(DrawnValue::DateTime),
        select(&DOUBLES[..]).prop_map(DrawnValue::Double),
        select(&OBJECTS[..]).prop_map(DrawnValue::Relationship),
        vec(select(&OBJECTS[..]), 1..3).prop_map(DrawnValue::ListRelationship),
        vec(0..10_u8, 1..3).prop_map(DrawnValue::ListProperty),
        select(&TERMS[..]).prop_map(DrawnValue::Vocab),
        (select(&STRINGS[..]), select(&STRINGS[..])).prop_map(|(english, french)| DrawnValue::Language(english, french)),
    ]
    .boxed()
}

/// A drawn Attribute.
fn drawn_attribute() -> BoxedStrategy<DrawnAttribute> {
    let companion = prop_oneof![
        Just(Companion::Alone),
        (0..10_u8).prop_map(Companion::DatasetInstance),
        (0..10_u8).prop_map(Companion::SubProperty),
    ];
    (select(&NAMES[..]), drawn_value(), companion)
        .prop_map(|(name, value, companion)| DrawnAttribute { name, value, companion })
        .boxed()
}

/// The Attributes of a drawn Entity, each name once.
fn drawn_entity() -> BoxedStrategy<Vec<DrawnAttribute>> {
    vec(drawn_attribute(), 1..5)
        .prop_map(|attributes| {
            let mut named: Vec<DrawnAttribute> = Vec::with_capacity(attributes.len());
            for attribute in attributes {
                if named.iter().all(|kept| kept.name != attribute.name) {
                    named.push(attribute);
                }
            }
            named
        })
        .boxed()
}

/// An operand of an equality: a single value, a range or a value list.
fn equality_operand() -> BoxedStrategy<String> {
    prop_oneof![
        (0..10_u8).prop_map(|number| number.to_string()),
        select(&STRINGS[..]).prop_map(|text| format!("\"{text}\"")),
        select(&["true", "false"][..]).prop_map(str::to_owned),
        select(&DATES[..]).prop_map(str::to_owned),
        select(&DATE_TIMES[..]).prop_map(str::to_owned),
        select(&OBJECTS[..]).prop_map(|object| format!("\"{object}\"")),
        (0..10_u8, 0..10_u8).prop_map(|(low, high)| format!("{}..{}", low.min(high), low.max(high))),
        Just("\"a\"..\"b\"".to_owned()),
        (0..10_u8, 0..10_u8).prop_map(|(first, second)| format!("{first},{second}")),
        (select(&STRINGS[..]), select(&STRINGS[..])).prop_map(|(first, second)| format!("\"{first}\",\"{second}\"")),
    ]
    .boxed()
}

/// An operand of an ordering.
fn ordering_operand() -> BoxedStrategy<String> {
    prop_oneof![
        (0..10_u8).prop_map(|number| number.to_string()),
        select(&STRINGS[..]).prop_map(|text| format!("\"{text}\"")),
        select(&DATES[..]).prop_map(str::to_owned),
        select(&DATE_TIMES[..]).prop_map(str::to_owned),
    ]
    .boxed()
}

/// A Query Term over a name of [`NAMES`], or over its sub-property.
fn drawn_term() -> BoxedStrategy<String> {
    let path = prop_oneof![
        select(&NAMES[..]).prop_map(str::to_owned),
        select(&NAMES[..]).prop_map(|name| format!("{name}.sub")),
    ];
    prop_oneof![
        path.clone(),
        (path.clone(), select(&["==", "!="][..]), equality_operand()).prop_map(|(path, op, operand)| format!("{path}{op}{operand}")),
        (path.clone(), select(&[">", ">=", "<", "<="][..]), ordering_operand()).prop_map(|(path, op, operand)| format!("{path}{op}{operand}")),
        (path, select(&["~=", "!~="][..]), select(&PATTERNS[..])).prop_map(|(path, op, pattern)| format!("{path}{op}\"{pattern}\"")),
    ]
    .boxed()
}

/// A filter of up to four Query Terms, grouped up to two deep.
fn drawn_filter() -> BoxedStrategy<String> {
    drawn_term()
        .prop_recursive(2, 4, 2, |inner| {
            (vec(inner, 2..3), select(&[";", "|"][..])).prop_map(|(members, op)| format!("({})", members.join(op)))
        })
        .boxed()
}

/// Adds the drawn Property `$attribute` holding `$value` to `$builder`, with its companion, and evaluates to the
/// builder. A macro keeps the builder's typestate unnamed.
macro_rules! with_property {
    ($builder:expr, $attribute:expr, $value:expr, $dataset:expr) => {{
        let value: Value = $value;
        match $attribute.companion {
            Companion::Alone => $builder.property($attribute.name, value),
            Companion::DatasetInstance(second) => $builder
                .property($attribute.name, value)
                .property($attribute.name, Property::builder(number(second)?).dataset_id($dataset.clone())),
            Companion::SubProperty(sub) => $builder.property($attribute.name, Property::builder(value).property("sub", number(sub)?)),
        }
    }};
}

impl GeneratedCase {
    /// The case `seed` draws: `entities` Entities and `filters` filters.
    ///
    /// # Errors
    ///
    /// Fails when the generator rejects its draw.
    pub fn drawn(seed: u64, entities: usize, filters: usize) -> Result<GeneratedCase> {
        let config = Config {
            rng_seed: RngSeed::Fixed(seed),
            failure_persistence: None,
            ..Config::default()
        };
        let mut runner = TestRunner::new(config);
        let strategy = (vec(drawn_entity(), entities), vec(drawn_filter(), filters));
        let (entities, filters) = strategy.new_tree(&mut runner).or_fail()?.current();
        Ok(GeneratedCase { entities, filters })
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
                builder = match &attribute.value {
                    DrawnValue::Number(value) => with_property!(builder, attribute, number(*value)?, dataset),
                    DrawnValue::Text(text) => with_property!(builder, attribute, Value::from(*text), dataset),
                    DrawnValue::Boolean(flag) => with_property!(builder, attribute, Value::from(*flag), dataset),
                    DrawnValue::Numbers(values) => with_property!(builder, attribute, numbers(values)?, dataset),
                    DrawnValue::Double(lexical) => with_property!(builder, attribute, xsd_typed("double", lexical)?, dataset),
                    DrawnValue::Date(date) => builder.property(attribute.name, Property::builder(*date).value_type("Date".parse::<VocabTerm>()?)),
                    DrawnValue::DateTime(instant) => builder.property(attribute.name, Property::builder(*instant).value_type("DateTime".parse::<VocabTerm>()?)),
                    DrawnValue::Relationship(object) => builder.relationship(attribute.name, Relationship::builder(object.parse::<EntityId>()?)),
                    DrawnValue::ListRelationship(objects) => builder.list_relationship(attribute.name, ListRelationship::builder(object_list(objects)?)),
                    DrawnValue::ListProperty(values) => builder.list_property(attribute.name, ListProperty::builder(number_list(values)?)),
                    DrawnValue::Vocab(term) => builder.vocab_property(attribute.name, VocabProperty::builder(term.parse::<VocabTerm>()?)),
                    DrawnValue::Language(english, french) => {
                        builder.language_property(attribute.name, LanguageMap::try_from_pairs([("en", *english), ("fr", *french)])?)
                    }
                };
            }
            documents.push(builder.build()?);
        }
        Ok(documents)
    }
}

/// The objects `objects` as Entity identifiers.
fn object_list(objects: &[&str]) -> Result<Vec<EntityId>> {
    let mut identifiers = Vec::with_capacity(objects.len());
    for object in objects {
        identifiers.push(object.parse()?);
    }
    Ok(identifiers)
}

/// The number `value` as a Value.
fn number(value: u8) -> Result<Value> {
    Ok(Value::Number(value.to_string().parse::<Number>()?))
}

/// The numbers `values` as Values.
fn number_list(values: &[u8]) -> Result<Vec<Value>> {
    values.iter().map(|value| number(*value)).collect()
}

/// The array of the numbers `values`.
fn numbers(values: &[u8]) -> Result<Value> {
    Ok(Value::Array(number_list(values)?))
}
