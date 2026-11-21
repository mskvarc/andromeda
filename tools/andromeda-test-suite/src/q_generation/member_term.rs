//! The terms into a member of a drawn value that the value satisfies: a member of the JSON object of a Property or a
//! `JsonProperty`, the `type` or `coordinates` of a geometry, or a language of a `LanguageProperty`.
//!
//! A Property's value goes through JSON-LD compaction, so an array of one object reads back as the object and an
//! array of one element as the element, while an array of several objects resolves no trailing member path. A
//! `JsonProperty` keeps its JSON as written (clause 4.5.24.2), so a member holding an array is unordered whatever its
//! length, a `DateTime` in it is a plain string, and a member holding the JSON `null` exists.

use crate::{
    geo_generation::pooled_geometry::{Outline, PooledGeometry},
    q_generation::{
        drawn_attribute::{DrawnJson, Member, MemberValue},
        held_term::{HeldTerm, Operator, compared, ending, existing},
        scalar_comparison::{Temporal, elements, number, temporal, text},
    },
};
use proptest::{
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
    strategy::Union,
};
use strum::IntoEnumIterator;

/// How the members of a JSON object are read.
#[derive(Debug, Clone, Copy)]
enum Members {
    /// Compacted, as the value of a Property.
    Compacted,
    /// As written, as a `JsonProperty` that `jsonKeys` names.
    Literal,
}

/// A term into the member `name` holding `value`, None for the JSON `null`, read as `members` says.
fn member(name: &'static str, value: Option<&MemberValue>, members: Members) -> BoxedStrategy<HeldTerm> {
    let path = format!("[{name}]");
    match (value, members) {
        (None, Members::Compacted | Members::Literal) => existing(path),
        (Some(MemberValue::Number(held)), Members::Compacted | Members::Literal) => ending(path, number(*held)),
        (Some(MemberValue::Text(held)), Members::Compacted | Members::Literal) | (Some(MemberValue::DateTime(held)), Members::Literal) => {
            ending(path, text(held))
        }
        (Some(MemberValue::Numbers(held)), Members::Compacted) => match held.as_slice() {
            [] => prop_oneof![existing(path.clone()), ending(path, elements(&[]))].boxed(),
            [single] => ending(path, number(*single)),
            several => ending(path, elements(several)),
        },
        (Some(MemberValue::Numbers(held)), Members::Literal) if held.is_empty() => existing(path),
        (Some(MemberValue::Numbers(held)), Members::Literal) => ending(path, elements(held)),
        (Some(MemberValue::Nested(inner, held)), Members::Compacted | Members::Literal) => ending(format!("[{name}.{inner}]"), number(*held)),
        (Some(MemberValue::DateTime(held)), Members::Compacted) => ending(path, temporal(held, Temporal::DateTime)),
    }
}

/// A term the whole Attribute satisfies by existing, or one into a member of `members`, each about as often.
fn one_of<'m, V: 'm>(drawn: impl IntoIterator<Item = &'m Member<V>>, term: impl Fn(&'m Member<V>) -> BoxedStrategy<HeldTerm>) -> BoxedStrategy<HeldTerm> {
    let mut options = vec![(1, existing(""))];
    options.extend(drawn.into_iter().map(|held| (3, term(held))));
    Union::new_weighted(options).boxed()
}

/// A term into the JSON object of a Property holding `members`.
pub fn structured(members: &[Member<MemberValue>]) -> BoxedStrategy<HeldTerm> {
    one_of(members, |held| member(held.name, Some(&held.value), Members::Compacted))
}

/// A term into an array of the JSON objects `objects`: into the one object an array of one compacts to, or the test
/// that an array of several exists, since it resolves no member.
pub fn structures(objects: &[Vec<Member<MemberValue>>]) -> BoxedStrategy<HeldTerm> {
    match objects {
        [single] => structured(single),
        [] | [_, _, ..] => existing(""),
    }
}

/// A term into the JSON of a `JsonProperty`, read as written, into a member of its object or of one of its objects.
pub fn json(held: &DrawnJson) -> BoxedStrategy<HeldTerm> {
    let members: Vec<&Member<Option<MemberValue>>> = match held {
        DrawnJson::Object(members) => members.iter().collect(),
        DrawnJson::Objects(objects) => objects.iter().flatten().collect(),
    };
    one_of(members, |held| member(held.name, held.value.as_ref(), Members::Literal))
}

/// A term into the geometry `held`: its `type`, or the longitude or latitude among the `coordinates` of a Point.
pub fn geometry(held: PooledGeometry) -> BoxedStrategy<HeldTerm> {
    let outline = held.outline();
    let type_name = outline.type_name();
    let mut other_types: Vec<&'static str> = Vec::new();
    for other in PooledGeometry::iter().map(|geometry| geometry.outline().type_name()) {
        if other != type_name && !other_types.contains(&other) {
            other_types.push(other);
        }
    }
    let typed = prop_oneof![
        2 => Just(compared(Operator::Equal, &format!("\"{type_name}\""))),
        1 => select(other_types).prop_map(|other| compared(Operator::Unequal, &format!("\"{other}\""))),
    ];
    match outline {
        Outline::Point(longitude, latitude) => prop_oneof![
            1 => ending("[type]", typed.boxed()),
            1 => ending("[coordinates]", select(vec![longitude, latitude]).prop_map(|ordinate| compared(Operator::Equal, &ordinate)).boxed()),
        ]
        .boxed(),
        Outline::LineString(_) | Outline::Polygon(_) => ending("[type]", typed.boxed()),
    }
}

/// A term into the `LanguageProperty` holding `english` and `french`: a string of one language, or of either with no
/// language named or every language named by `*`.
pub fn language(english: &'static str, french: &'static str) -> BoxedStrategy<HeldTerm> {
    let either = |path: &'static str| select(vec![english, french]).prop_flat_map(move |held| ending(path, text(held))).boxed();
    prop_oneof![
        2 => ending("[en]", text(english)),
        1 => ending("[fr]", text(french)),
        2 => either(""),
        1 => either("[*]"),
    ]
    .boxed()
}
