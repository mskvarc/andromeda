//! The strategies that draw the Attributes of an Entity, weighted so that every attribute family and every Property
//! value turns up in the cases the suite draws.
//!
//! Each name has a home family it is drawn with three times as often as with any family, so that several Entities of
//! a case hold a name with values one ordering, range or pattern compares alike, and a filter can select some of them
//! and leave the others.

use crate::{
    geo_generation::pooled,
    q_generation::{
        drawn_attribute::{Companion, DrawnAttribute, DrawnDouble, DrawnFamily, DrawnJson, Member, MemberValue, PropertyValue},
        drawn_entity::DrawnEntity,
        drawn_object::{DrawnObject, drawn_object},
        drawn_pool::{DATE_TIMES, DATES, JSON_MEMBERS, NAMES, STRINGS, STRUCTURE_MEMBERS, TERMS, TIMES},
    },
};
use proptest::{
    bool::ANY,
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};
use std::fmt::Debug;
use strum::IntoEnumIterator;

/// The family a name is drawn with three times as often as with any family.
#[derive(Debug, Clone, Copy)]
enum Home {
    /// A Property holding a number.
    Number,
    /// A Property holding a string.
    Text,
    /// A Property holding a declared `DateTime`.
    DateTime,
    /// A Property holding a declared Time.
    Time,
    /// A Property holding a JSON object.
    Structured,
    /// A Relationship.
    Relationship,
    /// A `LanguageProperty`.
    Language,
}

/// The home of the name at each position of [`NAMES`].
const HOMES: [Home; 8] = [
    Home::Number,
    Home::Text,
    Home::DateTime,
    Home::Relationship,
    Home::Number,
    Home::Language,
    Home::Structured,
    Home::Time,
];

/// Each name of [`NAMES`] with its home.
fn named() -> Vec<(&'static str, Home)> {
    NAMES.into_iter().zip(HOMES).collect()
}

/// What a member holds.
fn member_value() -> BoxedStrategy<MemberValue> {
    prop_oneof![
        (0..10_u8).prop_map(MemberValue::Number),
        select(&STRINGS[..]).prop_map(MemberValue::Text),
        vec(0..10_u8, 0..3).prop_map(MemberValue::Numbers),
        (select(&STRUCTURE_MEMBERS[..]), 0..10_u8).prop_map(|(name, number)| MemberValue::Nested(name, number)),
        select(&DATE_TIMES[..]).prop_map(MemberValue::DateTime),
    ]
    .boxed()
}

/// The members of an object, named from `names`, each name once, holding what `value` draws.
fn members<V: Clone + Debug + 'static>(names: &'static [&'static str], value: BoxedStrategy<V>) -> BoxedStrategy<Vec<Member<V>>> {
    vec((select(names), value), 1..4)
        .prop_map(|drawn| {
            let mut members: Vec<Member<V>> = Vec::with_capacity(drawn.len());
            for (name, value) in drawn {
                if members.iter().all(|kept| kept.name != name) {
                    members.push(Member { name, value });
                }
            }
            members
        })
        .boxed()
}

/// What a member of a `JsonProperty` object holds, the JSON `null` among it.
fn json_member_value() -> BoxedStrategy<Option<MemberValue>> {
    prop_oneof![1 => Just(None), 4 => member_value().prop_map(Some)].boxed()
}

/// The JSON of a `JsonProperty`.
fn drawn_json() -> BoxedStrategy<DrawnJson> {
    prop_oneof![
        3 => members(&JSON_MEMBERS, json_member_value()).prop_map(DrawnJson::Object),
        1 => vec(members(&JSON_MEMBERS, json_member_value()), 1..3).prop_map(DrawnJson::Objects),
    ]
    .boxed()
}

/// The value of a Property.
fn property_value() -> BoxedStrategy<PropertyValue> {
    prop_oneof![
        (0..10_u8).prop_map(PropertyValue::Number),
        select(&STRINGS[..]).prop_map(PropertyValue::Text),
        ANY.prop_map(PropertyValue::Boolean),
        vec(0..10_u8, 1..3).prop_map(PropertyValue::Numbers),
        select(&DATES[..]).prop_map(PropertyValue::Date),
        select(&DATE_TIMES[..]).prop_map(PropertyValue::DateTime),
        select(&TIMES[..]).prop_map(PropertyValue::Time),
        select(DrawnDouble::iter().collect::<Vec<_>>()).prop_map(PropertyValue::Double),
        members(&STRUCTURE_MEMBERS, member_value()).prop_map(PropertyValue::Structured),
        vec(members(&STRUCTURE_MEMBERS, member_value()), 1..3).prop_map(PropertyValue::Structures),
    ]
    .boxed()
}

/// What a Property carries beside its value.
fn companion() -> BoxedStrategy<Companion> {
    prop_oneof![
        3 => Just(Companion::Alone),
        1 => (0..10_u8).prop_map(Companion::DatasetInstance),
        1 => (0..10_u8).prop_map(Companion::SubProperty),
        1 => select(&DATE_TIMES[..]).prop_map(Companion::ObservedAt),
    ]
    .boxed()
}

/// A Property holding what `value` draws, with a companion.
fn property(value: BoxedStrategy<PropertyValue>) -> BoxedStrategy<DrawnFamily> {
    (value, companion())
        .prop_map(|(value, companion)| DrawnFamily::Property(value, companion))
        .boxed()
}

/// The objects `drawn`, each once, in the order first drawn.
fn distinct(drawn: Vec<DrawnObject>) -> Vec<DrawnObject> {
    let mut objects = Vec::with_capacity(drawn.len());
    for object in drawn {
        if !objects.contains(&object) {
            objects.push(object);
        }
    }
    objects
}

/// A Relationship of the Entity at `position`, with one object or several.
fn relationship(position: usize) -> BoxedStrategy<DrawnFamily> {
    vec(drawn_object(position), 1..4)
        .prop_map(|objects| DrawnFamily::Relationship(distinct(objects)))
        .boxed()
}

/// A `LanguageProperty`, with an English and a French string.
fn language() -> BoxedStrategy<DrawnFamily> {
    (select(&STRINGS[..]), select(&STRINGS[..]))
        .prop_map(|(english, french)| DrawnFamily::Language(english, french))
        .boxed()
}

/// An Attribute of the Entity at `position` of any family, weighted so that each Property value is about as frequent
/// as each other family.
fn any_family(position: usize) -> BoxedStrategy<DrawnFamily> {
    prop_oneof![
        10 => property(property_value()),
        1 => pooled().prop_map(DrawnFamily::Geometry),
        1 => drawn_json().prop_map(DrawnFamily::Json),
        1 => relationship(position),
        1 => vec(drawn_object(position), 1..3).prop_map(DrawnFamily::ListRelationship),
        1 => vec(0..10_u8, 1..3).prop_map(DrawnFamily::ListProperty),
        1 => select(&TERMS[..]).prop_map(DrawnFamily::Vocab),
        1 => language(),
    ]
    .boxed()
}

/// An Attribute of the Entity at `position` of the family `home`.
fn home_family(home: Home, position: usize) -> BoxedStrategy<DrawnFamily> {
    match home {
        Home::Number => property((0..10_u8).prop_map(PropertyValue::Number).boxed()),
        Home::Text => property(select(&STRINGS[..]).prop_map(PropertyValue::Text).boxed()),
        Home::DateTime => property(select(&DATE_TIMES[..]).prop_map(PropertyValue::DateTime).boxed()),
        Home::Time => property(select(&TIMES[..]).prop_map(PropertyValue::Time).boxed()),
        Home::Structured => property(members(&STRUCTURE_MEMBERS, member_value()).prop_map(PropertyValue::Structured).boxed()),
        Home::Relationship => relationship(position),
        Home::Language => language(),
    }
}

/// An Attribute of the Entity at `position` under one of `names`, drawn with the name's home three times as often as
/// with any family.
fn named_attribute(names: Vec<(&'static str, Home)>, position: usize) -> BoxedStrategy<DrawnAttribute> {
    select(names)
        .prop_flat_map(move |(name, home)| {
            prop_oneof![3 => home_family(home, position), 1 => any_family(position)].prop_map(move |family| DrawnAttribute { name, family })
        })
        .boxed()
}

/// What a second write appends to the Entity at `position` holding `attributes`: an Attribute under a name the
/// Entity does not hold, which raises `attributeCreated`, a default notification trigger (Table 5.2.12-1), and maybe
/// an Attribute under a name it holds, which replaces that one. An Entity holds at most seven of the eight names, so a
/// name is always free.
fn drawn_change(attributes: &[DrawnAttribute], position: usize) -> BoxedStrategy<Vec<DrawnAttribute>> {
    let (held, free): (Vec<_>, Vec<_>) = named()
        .into_iter()
        .partition(|(name, _)| attributes.iter().any(|attribute| attribute.name == *name));
    let replaced = prop_oneof![Just(None), named_attribute(held, position).prop_map(Some)];
    (named_attribute(free, position), replaced)
        .prop_map(|(added, replaced)| [Some(added), replaced].into_iter().flatten().collect())
        .boxed()
}

/// The Attributes of the Entity at `position`, one to seven, each name once, and what its second write appends.
pub fn drawn_entity(position: usize) -> BoxedStrategy<DrawnEntity> {
    vec(named_attribute(named(), position), 4..8)
        .prop_map(|drawn| {
            let mut attributes: Vec<DrawnAttribute> = Vec::with_capacity(drawn.len());
            for attribute in drawn {
                if attributes.iter().all(|kept| kept.name != attribute.name) {
                    attributes.push(attribute);
                }
            }
            attributes
        })
        .prop_flat_map(move |attributes| {
            let change = drawn_change(&attributes, position);
            (Just(attributes), change).prop_map(|(attributes, change)| DrawnEntity { attributes, change })
        })
        .boxed()
}
