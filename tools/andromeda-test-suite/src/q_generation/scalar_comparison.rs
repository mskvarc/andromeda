//! The comparisons a drawn scalar value, or a list of them, satisfies under the plain reading of clause 4.9.
//!
//! Each value draws comparisons that hold for it alone (`==` the value, a value list holding it), and comparisons
//! that hold for the values of the pool a range, an ordering or a pattern lets through, a bound never letting through
//! every value of the pool. An array, a `ListProperty` and the objects of a Relationship are unordered and include an
//! operand they hold, so they draw only equalities with one of their elements and value lists.

use crate::q_generation::{
    drawn_attribute::DrawnDouble,
    drawn_object::DrawnObject,
    drawn_pool::{DATE_TIMES, DATES, OBJECTS, STRINGS, TERMS, TIMES},
    held_term::{Comparison, Operator, compared},
};
use proptest::{
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};
use std::iter;

/// The temporal type a declared value has.
#[derive(Debug, Clone, Copy)]
pub enum Temporal {
    /// An NGSI-LD Date.
    Date,
    /// An NGSI-LD `DateTime`.
    DateTime,
    /// An NGSI-LD Time.
    Time,
}

impl Temporal {
    /// The values of the pool of this type, every one in a lexical form that orders as the instant does.
    const fn candidates(self) -> &'static [&'static str] {
        match self {
            Temporal::Date => &DATES,
            Temporal::DateTime => &DATE_TIMES,
            Temporal::Time => &TIMES,
        }
    }
}

/// The values of `candidates` that are not `held`, or `held` alone when every candidate is it.
fn others<T: PartialEq + Copy>(candidates: &[T], held: T) -> Vec<T> {
    let others: Vec<T> = candidates.iter().copied().filter(|candidate| *candidate != held).collect();
    if others.is_empty() { vec![held] } else { others }
}

/// The values of `pool` a lower bound of `held` is drawn from: `held`, and the values below it but the least of the
/// pool, which would let every value of the pool through.
fn lower_bounds<T: Ord + Copy>(pool: &[T], held: T) -> Vec<T> {
    let least = pool.iter().copied().min();
    iter::once(held)
        .chain(pool.iter().copied().filter(|bound| *bound < held && Some(*bound) != least))
        .collect()
}

/// The values of `pool` an upper bound of `held` is drawn from: `held`, and the values above it but the greatest of
/// the pool.
fn upper_bounds<T: Ord + Copy>(pool: &[T], held: T) -> Vec<T> {
    let greatest = pool.iter().copied().max();
    iter::once(held)
        .chain(pool.iter().copied().filter(|bound| *bound > held && Some(*bound) != greatest))
        .collect()
}

/// A comparison the number `held` satisfies. A bound is at most two from it and lets through some of the numbers of
/// the pool and leaves others, and the ordering that leaves out more of them is drawn more often.
pub fn number(held: u8) -> BoxedStrategy<Comparison> {
    let pool: Vec<u8> = (0..10).collect();
    let lower = lower_bounds(&pool, held).into_iter().filter(|low| held - low <= 2).collect::<Vec<_>>();
    let upper = upper_bounds(&pool, held).into_iter().filter(|high| high - held <= 2).collect::<Vec<_>>();
    let (at_least, at_most) = if held >= 5 { (3, 1) } else { (1, 3) };
    prop_oneof![
        1 => Just(compared(Operator::Equal, &held)),
        1 => (0..10_u8).prop_map(move |other| compared(Operator::Equal, &format!("{other},{held}"))),
        at_least => select(lower.clone()).prop_map(|low| compared(Operator::GreaterOrEqual, &low)),
        at_most => select(upper.clone()).prop_map(|high| compared(Operator::LessOrEqual, &high)),
        3 => (select(lower), select(upper)).prop_map(|(low, high)| compared(Operator::Equal, &format!("{low}..{high}"))),
    ]
    .boxed()
}

/// A comparison the string `held` satisfies: equalities, patterns built from it, and orderings and ranges over the
/// strings of the pool.
pub fn text(held: &'static str) -> BoxedStrategy<Comparison> {
    let prefix = held.char_indices().nth(1).and_then(|(end, _)| held.get(..end)).unwrap_or(held);
    let lower = lower_bounds(&STRINGS, held);
    let upper = upper_bounds(&STRINGS, held);
    prop_oneof![
        1 => Just(compared(Operator::Equal, &format!("\"{held}\""))),
        1 => Just(compared(Operator::Matches, &format!("\"^{held}$\""))),
        2 => Just(compared(Operator::Matches, &format!("\"^{prefix}\""))),
        1 => Just(compared(Operator::Matches, &format!("\"(?i)^{held}$\""))),
        2 => select(lower.clone()).prop_map(|low| compared(Operator::GreaterOrEqual, &format!("\"{low}\""))),
        2 => select(upper.clone()).prop_map(|high| compared(Operator::LessOrEqual, &format!("\"{high}\""))),
        1 => (select(lower), select(upper)).prop_map(|(low, high)| compared(Operator::Equal, &format!("\"{low}\"..\"{high}\""))),
        1 => select(&STRINGS[..]).prop_map(move |other| compared(Operator::Equal, &format!("\"{held}\",\"{other}\""))),
    ]
    .boxed()
}

/// A comparison the Boolean `held` satisfies.
pub fn boolean(held: bool) -> BoxedStrategy<Comparison> {
    prop_oneof![
        4 => Just(compared(Operator::Equal, &held)),
        1 => Just(compared(Operator::Equal, &"true,false")),
    ]
    .boxed()
}

/// A comparison the declared temporal value `held` of type `temporal` satisfies, written as a bare literal.
pub fn temporal(held: &'static str, temporal: Temporal) -> BoxedStrategy<Comparison> {
    let candidates = temporal.candidates();
    let lower = lower_bounds(candidates, held);
    let upper = upper_bounds(candidates, held);
    let spelled = match temporal {
        Temporal::DateTime => held.strip_suffix('Z').map_or_else(|| held.to_owned(), |stem| format!("{stem}.000Z")),
        Temporal::Date | Temporal::Time => held.to_owned(),
    };
    prop_oneof![
        1 => Just(compared(Operator::Equal, &held)),
        1 => Just(compared(Operator::Equal, &spelled)),
        2 => select(lower.clone()).prop_map(|low| compared(Operator::GreaterOrEqual, &low)),
        2 => select(upper.clone()).prop_map(|high| compared(Operator::LessOrEqual, &high)),
        2 => (select(lower), select(upper)).prop_map(|(low, high)| compared(Operator::Equal, &format!("{low}..{high}"))),
    ]
    .boxed()
}

/// A comparison the `xsd:double` `held` satisfies: not-a-number only ever unequal, an infinity ordered past every
/// finite number.
pub fn double(held: DrawnDouble) -> BoxedStrategy<Comparison> {
    let satisfied: &[(Operator, &str)] = match held {
        DrawnDouble::Finite => &[
            (Operator::Equal, "1.5"),
            (Operator::GreaterOrEqual, "1"),
            (Operator::LessOrEqual, "2"),
            (Operator::Equal, "1..2"),
            (Operator::Unequal, "2"),
        ],
        DrawnDouble::NotANumber => &[(Operator::Unequal, "1"), (Operator::Unequal, "1.5")],
        DrawnDouble::Infinity => &[(Operator::Greater, "1"), (Operator::GreaterOrEqual, "1.5"), (Operator::Unequal, "1")],
        DrawnDouble::NegativeInfinity => &[(Operator::Less, "1"), (Operator::LessOrEqual, "-1"), (Operator::Unequal, "1")],
    };
    select(satisfied.to_vec()).prop_map(|(operator, operand)| compared(operator, &operand)).boxed()
}

/// A comparison an unordered list holding `held` satisfies: `==` one of its elements, or a value list holding one.
/// An empty list includes no operand, so it satisfies `!=` alone.
pub fn elements(held: &[u8]) -> BoxedStrategy<Comparison> {
    if held.is_empty() {
        return Just(compared(Operator::Unequal, &"\"x\"")).boxed();
    }
    let held = held.to_vec();
    prop_oneof![
        3 => select(held.clone()).prop_map(|element| compared(Operator::Equal, &element)),
        1 => (select(held), 0..10_u8).prop_map(|(element, other)| compared(Operator::Equal, &format!("{element},{other}"))),
    ]
    .boxed()
}

/// A comparison the objects `held` of a Relationship satisfy: `==` one of them, quoted or bare, a value list holding
/// one, or `!=` a URI of the pool none of them is.
pub fn objects(held: &[DrawnObject]) -> BoxedStrategy<Comparison> {
    let absent: Vec<&'static str> = OBJECTS.into_iter().filter(|uri| !held.contains(&DrawnObject::Fixed(uri))).collect();
    if held.is_empty() {
        return select(absent).prop_map(|other| compared(Operator::Unequal, &format!("\"{other}\""))).boxed();
    }
    let operands: Vec<String> = held.iter().map(|object| object.operand()).collect();
    let equal = prop_oneof![
        2 => select(operands.clone()).prop_map(|object| compared(Operator::Equal, &format!("\"{object}\""))),
        1 => select(operands.clone()).prop_map(|object| compared(Operator::Equal, &object)),
        1 => (select(operands), select(&OBJECTS[..])).prop_map(|(object, other)| compared(Operator::Equal, &format!("\"{object}\",\"{other}\""))),
    ];
    if absent.is_empty() {
        return equal.boxed();
    }
    prop_oneof![
        4 => equal,
        1 => select(absent).prop_map(|other| compared(Operator::Unequal, &format!("\"{other}\""))),
    ]
    .boxed()
}

/// A comparison the vocabulary term `held` satisfies, with `expandValues` naming the `VocabProperty`.
pub fn vocabulary(held: &'static str) -> BoxedStrategy<Comparison> {
    prop_oneof![
        3 => Just(compared(Operator::Equal, &format!("\"{held}\""))),
        1 => Just(compared(Operator::Equal, &TERMS.map(|term| format!("\"{term}\"")).join(","))),
        1 => select(others(&TERMS, held)).prop_map(|other| compared(Operator::Unequal, &format!("\"{other}\""))),
    ]
    .boxed()
}
