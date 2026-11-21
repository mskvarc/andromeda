//! The filters a drawn case asks, each a `q` over the drawn names with the `jsonKeys` and `expandValues` it is asked with.

use crate::q_generation::drawn_pool::{
    CORE_MEMBERS,
    DATE_TIMES,
    DATES,
    GEOMETRY_OPERANDS,
    NAMES,
    OBJECTS,
    PATH_MEMBERS,
    PATTERNS,
    STRINGS,
    STRUCTURE_MEMBERS,
    TERMS,
    TIMES,
};
use proptest::{
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};

/// The ranges between the two Dates, the two `DateTime`s and the two Times of the pools.
const TEMPORAL_RANGES: [&str; 3] = ["2024-01-15..2024-06-30", "2024-01-15T12:00:00Z..2024-06-30T00:00:00Z", "08:30:00Z..12:00:00Z"];

/// Value lists of URIs and of Booleans.
const OTHER_LISTS: [&str; 2] = ["\"urn:x:1\",\"urn:x:2\"", "true,false"];

/// One drawn filter.
#[derive(Debug, Clone)]
pub struct DrawnFilter {
    /// The `q`, as a client writes it.
    pub q: String,
    /// The Attributes `jsonKeys` names.
    pub json_keys: Vec<&'static str>,
    /// The Attributes `expandValues` names.
    pub expand_values: Vec<&'static str>,
}

/// An operand of `==` or `!=`: a single value of every type, a range or a value list.
fn equality_operand() -> BoxedStrategy<String> {
    prop_oneof![
        (0..10_u8).prop_map(|number| number.to_string()),
        select(&["-1", "2.5"][..]).prop_map(str::to_owned),
        select(&STRINGS[..]).prop_map(|text| format!("\"{text}\"")),
        select(&["true", "false"][..]).prop_map(str::to_owned),
        select(&DATES[..]).prop_map(str::to_owned),
        select(&DATE_TIMES[..]).prop_map(str::to_owned),
        select(&TIMES[..]).prop_map(str::to_owned),
        select(&OBJECTS[..]).prop_map(|object| format!("\"{object}\"")),
        select(&OBJECTS[..]).prop_map(str::to_owned),
        select(&GEOMETRY_OPERANDS[..]).prop_map(str::to_owned),
        select(&TERMS[..]).prop_map(|term| format!("\"{term}\"")),
        (0..10_u8, 0..10_u8).prop_map(|(low, high)| format!("{}..{}", low.min(high), low.max(high))),
        Just("\"a\"..\"b\"".to_owned()),
        select(&TEMPORAL_RANGES[..]).prop_map(str::to_owned),
        (0..10_u8, 0..10_u8).prop_map(|(first, second)| format!("{first},{second}")),
        (select(&STRINGS[..]), select(&STRINGS[..])).prop_map(|(first, second)| format!("\"{first}\",\"{second}\"")),
        (0..10_u8, select(&STRINGS[..])).prop_map(|(number, text)| format!("{number},\"{text}\"")),
        select(&OTHER_LISTS[..]).prop_map(str::to_owned),
    ]
    .boxed()
}

/// An operand of an ordering.
fn ordering_operand() -> BoxedStrategy<String> {
    prop_oneof![
        (0..10_u8).prop_map(|number| number.to_string()),
        select(&["-1", "2.5", "13.4", "52.5"][..]).prop_map(str::to_owned),
        select(&STRINGS[..]).prop_map(|text| format!("\"{text}\"")),
        select(&DATES[..]).prop_map(str::to_owned),
        select(&DATE_TIMES[..]).prop_map(str::to_owned),
        select(&TIMES[..]).prop_map(str::to_owned),
    ]
    .boxed()
}

/// An attribute path over a name of [`NAMES`]: the name, its sub-property or observation time, a trailing path into
/// its value, or a member of its own representation named as if it were a sub-Attribute.
fn drawn_path() -> BoxedStrategy<String> {
    prop_oneof![
        3 => select(&NAMES[..]).prop_map(str::to_owned),
        1 => select(&NAMES[..]).prop_map(|name| format!("{name}.sub")),
        1 => select(&NAMES[..]).prop_map(|name| format!("{name}.observedAt")),
        2 => (select(&NAMES[..]), select(&PATH_MEMBERS[..])).prop_map(|(name, member)| format!("{name}[{member}]")),
        1 => (select(&NAMES[..]), select(&STRUCTURE_MEMBERS[..]), select(&STRUCTURE_MEMBERS[..]))
            .prop_map(|(name, outer, inner)| format!("{name}[{outer}.{inner}]")),
        1 => (select(&NAMES[..]), select(&CORE_MEMBERS[..])).prop_map(|(name, member)| format!("{name}.{member}")),
    ]
    .boxed()
}

/// A Query Term: an existence test, an equality, an ordering or a pattern.
fn drawn_term() -> BoxedStrategy<String> {
    prop_oneof![
        drawn_path(),
        (drawn_path(), select(&["==", "!="][..]), equality_operand()).prop_map(|(path, op, operand)| format!("{path}{op}{operand}")),
        (drawn_path(), select(&[">", ">=", "<", "<="][..]), ordering_operand()).prop_map(|(path, op, operand)| format!("{path}{op}{operand}")),
        (drawn_path(), select(&["~=", "!~="][..]), select(&PATTERNS[..])).prop_map(|(path, op, pattern)| format!("{path}{op}\"{pattern}\"")),
    ]
    .boxed()
}

/// A `q` of up to four Query Terms, grouped up to two deep.
fn drawn_q() -> BoxedStrategy<String> {
    drawn_term()
        .prop_recursive(2, 4, 2, |inner| {
            (vec(inner, 2..3), select(&[";", "|"][..])).prop_map(|(members, op)| format!("({})", members.join(op)))
        })
        .boxed()
}

/// Up to `most` names of [`NAMES`], each once.
fn some_names(most: usize) -> BoxedStrategy<Vec<&'static str>> {
    vec(select(&NAMES[..]), 0..=most)
        .prop_map(|drawn| {
            let mut names = Vec::with_capacity(drawn.len());
            for name in drawn {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
            names
        })
        .boxed()
}

/// A filter, a `q` with the names `jsonKeys` and `expandValues` list beside it.
pub fn drawn_filter() -> BoxedStrategy<DrawnFilter> {
    (drawn_q(), some_names(2), some_names(1))
        .prop_map(|(q, json_keys, expand_values)| DrawnFilter { q, json_keys, expand_values })
        .boxed()
}
