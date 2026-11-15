//! Serialising a parsed `q` filter and parsing the text again gives back the
//! same filter, for filters generated over the whole clause 4.9 grammar.
//!
//! A filter is stored, forwarded and read back through its serialisation, so
//! a filter the round trip changes is answered differently once it has
//! travelled. The generator writes filters as a client does, covering every
//! operator, every operand type, ranges and value lists, dotted, bracketed and
//! wildcard paths, hops up to three deep with and without a type guard, and
//! groups nested up to four deep. Every URI operand is in its normalised
//! spelling, because a lone operand in another spelling is rewritten into a
//! value list of both spellings when it is parsed.

use crate::filter::{expression::Expr, parser::parse, serialize::serialize};
use proptest::{
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_assert_eq, prop_oneof, proptest},
    sample::select,
    test_runner::Config,
};

/// Attribute names, a prefixed name, a quoted IRI and a quoted prefixed name
/// among them. A bare prefixed name at the head of a hop is the type of a
/// guard and the attribute after it.
const NAMES: [&str; 8] = [
    "a",
    "speed",
    "brandName",
    "x_1",
    "observedAt",
    "ex:name",
    "\"https://example.com/q\"",
    "\"ex:q\"",
];

/// The names of [`NAMES`] a hop's path may open with after a type guard: a
/// bare prefixed name there leaves the parser two colons, and it refuses it.
const AFTER_GUARD_NAMES: [&str; 7] = ["a", "speed", "brandName", "x_1", "observedAt", "\"https://example.com/q\"", "\"ex:q\""];

/// Entity types of a hop guard.
const TYPES: [&str; 3] = ["T1", "Vehicle", "Building"];

/// Numbers in the forms clause 4.9 takes from RFC 8259.
const NUMBERS: [&str; 8] = ["0", "-1", "10", "2.5", "1e3", "-0.25", "9007199254740993", "1.0"];

/// Quoted strings, escapes and the separators of the language among them.
const STRINGS: [&str; 6] = ["\"abc\"", "\"\"", "\"with \\\"quote\\\"\"", "\"a,b\"", "\"semi;colon|pipe\"", "\"ünï\""];

/// Date-times, dates and times in the forms clause 4.6.3 defines.
const TIMES: [&str; 6] = [
    "2024-01-15T12:00:00Z",
    "2024-01-15T12:00:00.5Z",
    "1999-12-31T23:59:59.123456Z",
    "2024-01-15",
    "12:00:00Z",
    "09:30:00.5Z",
];

/// URIs in their normalised spelling.
const URIS: [&str; 3] = ["urn:x:1", "http://example.com/a", "urn:ngsi-ld:Vehicle:A4567"];

/// Regular expressions as quoted strings.
const PATTERNS: [&str; 4] = ["\"a.*\"", "\"^x\"", "\"[0-9]+\"", "\"\\\\d\""];

/// A `ComparableValue`.
fn comparable() -> BoxedStrategy<String> {
    prop_oneof![select(&NUMBERS[..]), select(&STRINGS[..]), select(&TIMES[..])]
        .prop_map(str::to_owned)
        .boxed()
}

/// A `Value`: a `ComparableValue` or a Boolean.
fn value() -> BoxedStrategy<String> {
    prop_oneof![comparable(), select(&["true", "false"][..]).prop_map(str::to_owned)].boxed()
}

/// A `CompEqualityValue`: a single value, a URI, a range or a value list.
fn equality_value() -> BoxedStrategy<String> {
    prop_oneof![
        value(),
        select(&URIS[..]).prop_map(str::to_owned),
        (select(&NUMBERS[..]), select(&NUMBERS[..])).prop_map(|(low, high)| format!("{low}..{high}")),
        (select(&STRINGS[..]), select(&STRINGS[..])).prop_map(|(low, high)| format!("{low}..{high}")),
        Just("2024-01-01..2024-12-31".to_owned()),
        vec(value(), 2..4).prop_map(|members| members.join(",")),
    ]
    .boxed()
}

/// A `DottedPath` of one to three of `names`.
fn dotted(names: &'static [&'static str]) -> BoxedStrategy<String> {
    vec(select(names), 1..4).prop_map(|names| names.join(".")).boxed()
}

/// A `ValuePath` over `names`: a dotted path, optionally followed by one
/// bracketed dotted path of plain names or by the wildcard.
fn value_path(names: &'static [&'static str]) -> BoxedStrategy<String> {
    let trailing = prop_oneof![
        Just(String::new()),
        vec(select(&["city", "en", "zip", "b"][..]), 1..3).prop_map(|members| format!("[{}]", members.join("."))),
        Just("[*]".to_owned()),
    ];
    (dotted(names), trailing).prop_map(|(path, trailing)| format!("{path}{trailing}")).boxed()
}

/// An `Attribute`: a value path, or hops into it up to `depth` deep.
fn attribute(depth: u32) -> BoxedStrategy<String> {
    prop_oneof![value_path(&NAMES), hop(depth)].boxed()
}

/// A hop, and up to `depth - 1` hops within it, into a value path, with no
/// type guard, a guard of one type or a guard of several.
fn hop(depth: u32) -> BoxedStrategy<String> {
    let inner = |names: &'static [&'static str]| {
        if depth > 1 {
            prop_oneof![value_path(names), hop(depth - 1)].boxed()
        } else {
            value_path(names)
        }
    };
    let guard = prop_oneof![
        select(&TYPES[..]).prop_map(|guard| format!("{guard}:")),
        vec(select(&TYPES[..]), 2..4).prop_map(|guards| format!("{}:", guards.join(","))),
    ];
    let head = prop_oneof![(Just(String::new()), inner(&NAMES)), (guard, inner(&AFTER_GUARD_NAMES))];
    (select(&["r", "isParked", "link"][..]), head)
        .prop_map(|(relationship, (guard, inner))| format!("{relationship}{{{guard}{inner}}}"))
        .boxed()
}

/// A `QueryTerm`.
fn term() -> BoxedStrategy<String> {
    prop_oneof![
        attribute(3),
        (attribute(3), select(&["==", "!="][..]), equality_value()).prop_map(|(path, op, operand)| format!("{path}{op}{operand}")),
        (attribute(3), select(&[">", ">=", "<", "<="][..]), comparable()).prop_map(|(path, op, operand)| format!("{path}{op}{operand}")),
        (attribute(3), select(&["~=", "!~="][..]), select(&PATTERNS[..])).prop_map(|(path, op, pattern)| format!("{path}{op}{pattern}")),
    ]
    .boxed()
}

/// A `Query`: terms joined by `;` and `|`, with groups nested up to four deep.
fn query() -> BoxedStrategy<String> {
    term()
        .prop_recursive(4, 32, 4, |inner| {
            (vec(inner, 2..4), select(&[";", "|"][..])).prop_map(|(members, op)| format!("({})", members.join(op)))
        })
        .boxed()
}

/// The filter a client wrote, parsed.
fn parsed(written: &str) -> Expr {
    parse(written).unwrap_or_else(|error| panic!("the generator writes only grammatical filters, and {written} is refused: {error}"))
}

proptest! {
    #![proptest_config(Config::with_cases(2048))]

    #[test]
    fn serialising_a_parsed_filter_and_parsing_it_again_gives_the_same_filter(written in query()) {
        let expr = parsed(&written);
        let serialised = serialize(&expr);
        prop_assert_eq!(parse(&serialised).ok(), Some(expr), "{} serialised as {}", written, serialised);
    }

    #[test]
    fn serialising_is_stable_after_one_round_trip(written in query()) {
        let serialised = serialize(&parsed(&written));
        prop_assert_eq!(serialize(&parsed(&serialised)), serialised);
    }
}
