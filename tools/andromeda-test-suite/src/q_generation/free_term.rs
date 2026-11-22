//! The Query Terms a filter draws with no Entity in view: every operator over every path form and every operand type
//! the pools hold, whether or not any drawn Entity satisfies them.

use crate::q_generation::{
    drawn_pool::{CORE_MEMBERS, DATE_TIMES, DATES, GEOMETRY_OPERANDS, NAMES, OBJECTS, PATH_MEMBERS, PATTERNS, STRINGS, STRUCTURE_MEMBERS, TERMS, TIMES},
    drawn_query::DrawnQuery,
};
use proptest::{
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};

/// Ranges between Dates, `DateTime`s and Times of the pools.
const TEMPORAL_RANGES: [&str; 3] = ["2024-01-15..2024-06-30", "2024-01-15T12:00:00Z..2024-06-30T00:00:00Z", "08:30:00Z..12:00:00Z"];

/// Value lists of URIs and of Booleans.
const OTHER_LISTS: [&str; 2] = ["\"urn:x:1\",\"urn:x:2\"", "true,false"];

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
        select(&["-1", "2.5", "63", "175"][..]).prop_map(str::to_owned),
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

/// What a drawn pattern is written between: nothing, as clause 4.9 writes a `RegExp`, or the quotes a client may add.
/// Every pattern of the pool reads back whole written bare.
const PATTERN_QUOTES: [&str; 2] = ["", "\""];

/// A Query Term over any name: an existence test, an equality, an ordering or a pattern.
pub fn free_term() -> BoxedStrategy<DrawnQuery> {
    prop_oneof![
        1 => drawn_path(),
        6 => (drawn_path(), select(&["==", "!="][..]), equality_operand()).prop_map(|(path, op, operand)| format!("{path}{op}{operand}")),
        6 => (drawn_path(), select(&[">", ">=", "<", "<="][..]), ordering_operand()).prop_map(|(path, op, operand)| format!("{path}{op}{operand}")),
        6 => (drawn_path(), select(&["~=", "!~="][..]), select(&PATTERNS[..]), select(&PATTERN_QUOTES[..]))
            .prop_map(|(path, op, pattern, quote)| format!("{path}{op}{quote}{pattern}{quote}")),
    ]
    .prop_map(DrawnQuery::local)
    .boxed()
}
