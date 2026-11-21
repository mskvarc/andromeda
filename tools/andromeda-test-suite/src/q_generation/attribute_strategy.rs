//! The strategies that draw the Attributes of an Entity, weighted so that every attribute family and every Property
//! value turns up in the cases the suite draws.

use crate::q_generation::{
    drawn_attribute::{Companion, DrawnAttribute, DrawnFamily, DrawnGeometry, DrawnJson, Member, MemberValue, PropertyValue},
    drawn_pool::{DATE_TIMES, DATES, DOUBLES, JSON_MEMBERS, NAMES, OBJECTS, POSITIONS, STRINGS, STRUCTURE_MEMBERS, TERMS, TIMES},
};
use proptest::{
    bool::ANY,
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};
use std::fmt::Debug;

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

/// The geometry of a `GeoProperty`.
fn drawn_geometry() -> BoxedStrategy<DrawnGeometry> {
    let count = POSITIONS.len();
    prop_oneof![
        (0..count).prop_map(DrawnGeometry::Point),
        (0..count, 1..count).prop_map(move |(from, step)| DrawnGeometry::Line(from, (from + step) % count)),
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
        select(&DOUBLES[..]).prop_map(PropertyValue::Double),
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

/// The objects `drawn`, each once, in the order first drawn.
fn distinct(drawn: Vec<&'static str>) -> Vec<&'static str> {
    let mut objects = Vec::with_capacity(drawn.len());
    for object in drawn {
        if !objects.contains(&object) {
            objects.push(object);
        }
    }
    objects
}

/// The family of an Attribute, weighted so that each Property value is about as frequent as each other family.
fn drawn_family() -> BoxedStrategy<DrawnFamily> {
    prop_oneof![
        10 => (property_value(), companion()).prop_map(|(value, companion)| DrawnFamily::Property(value, companion)),
        1 => drawn_geometry().prop_map(DrawnFamily::Geometry),
        1 => drawn_json().prop_map(DrawnFamily::Json),
        1 => vec(select(&OBJECTS[..]), 1..4).prop_map(|objects| DrawnFamily::Relationship(distinct(objects))),
        1 => vec(select(&OBJECTS[..]), 1..3).prop_map(DrawnFamily::ListRelationship),
        1 => vec(0..10_u8, 1..3).prop_map(DrawnFamily::ListProperty),
        1 => select(&TERMS[..]).prop_map(DrawnFamily::Vocab),
        1 => (select(&STRINGS[..]), select(&STRINGS[..])).prop_map(|(english, french)| DrawnFamily::Language(english, french)),
    ]
    .boxed()
}

/// The Attributes of a drawn Entity, one to four, each name once.
pub fn drawn_entity() -> BoxedStrategy<Vec<DrawnAttribute>> {
    vec((select(&NAMES[..]), drawn_family()), 1..5)
        .prop_map(|drawn| {
            let mut attributes: Vec<DrawnAttribute> = Vec::with_capacity(drawn.len());
            for (name, family) in drawn {
                if attributes.iter().all(|kept| kept.name != name) {
                    attributes.push(DrawnAttribute { name, family });
                }
            }
            attributes
        })
        .boxed()
}
