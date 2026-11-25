//! `q` filters generated over the whole clause 4.9 grammar, written as a
//! client writes them.
//!
//! The generator covers every operator, every operand type, ranges and value
//! lists, dotted, bracketed and wildcard paths, names unquoted and quoted at
//! every position, hops up to three deep with no type list or a list of one
//! type or several, with or without JSON whitespace between its marks, each
//! before a value path or a further hop, and groups nested up to four deep.
//! Every URI operand
//! is in its normalised spelling, because a lone operand in another spelling
//! is rewritten into a value list of both spellings when it is parsed.

use proptest::{
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};

/// Names of every position a path names: unquoted `AttrName`s, and a quoted
/// absolute IRI and a quoted prefixed name, the two spellings no `AttrName`
/// holds.
const NAMES: [&str; 7] = ["a", "speed", "brandName", "x_1", "observedAt", "\"https://example.com/q\"", "\"ex:q\""];

/// Entity types of a hop's type list: unquoted `EntityType`s, and a quoted
/// prefixed name and a quoted absolute IRI.
const TYPES: [&str; 5] = ["T1", "Vehicle", "Building", "\"ex:T\"", "\"https://example.com/T\""];

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

/// Regular expressions as a `q` filter writes them, quoted or bare (clause 4.9 `RegExp`), a bare one holding a `|` and a `)` only inside its group and its bracket expression.
const PATTERNS: [&str; 7] = ["\"a.*\"", "\"^x\"", "\"[0-9]+\"", "\"\\\\d\"", "a.*", "^x(y|z)$", "[;|)]+"];

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
    let trailing_group = prop_oneof![
        Just(String::new()),
        vec(select(&["city", "en", "zip", "b"][..]), 1..3).prop_map(|members| format!("[{}]", members.join("."))),
        Just("[*]".to_owned()),
    ];
    (dotted(names), trailing_group)
        .prop_map(|(path, trailing_group)| format!("{path}{trailing_group}"))
        .boxed()
}

/// A `ValuePath` over every name of [`NAMES`], as it stands on its own in an
/// `orderBy` term or an `aggrGroupBy` key.
pub(crate) fn written_value_path() -> BoxedStrategy<String> {
    value_path(&NAMES)
}

/// An `Attribute`: a value path, or hops into it up to `depth` deep.
fn attribute(depth: u32) -> BoxedStrategy<String> {
    prop_oneof![value_path(&NAMES), hop(depth)].boxed()
}

/// A hop, and up to `depth - 1` hops within it, into a value path, with no
/// type list, a list of one type or a list of several, so every hop of a chain
/// takes a list of its own.
fn hop(depth: u32) -> BoxedStrategy<String> {
    let inner = || {
        if depth > 1 {
            prop_oneof![value_path(&NAMES), hop(depth - 1)].boxed()
        } else {
            value_path(&NAMES)
        }
    };
    let guard = (vec(select(&TYPES[..]), 1..4), select(&["", " "][..]))
        .prop_map(|(guards, space)| format!("{space}{}{space}:{space}", guards.join(&format!("{space},{space}"))));
    let head = prop_oneof![(Just(String::new()), inner()), (guard, inner())];
    (select(&["r", "isParked", "link"][..]), head)
        .prop_map(|(relationship, (guard, inner))| format!("{relationship}{{{guard}{inner}}}"))
        .boxed()
}

/// A `QueryTerm`.
fn term() -> BoxedStrategy<String> {
    prop_oneof![
        attribute(3),
        (attribute(3), select(&["==", "!="][..]), equality_value()).prop_map(|(path, operator, operand)| format!("{path}{operator}{operand}")),
        (attribute(3), select(&[">", ">=", "<", "<="][..]), comparable()).prop_map(|(path, operator, operand)| format!("{path}{operator}{operand}")),
        (attribute(3), select(&["~=", "!~="][..]), select(&PATTERNS[..])).prop_map(|(path, operator, pattern)| format!("{path}{operator}{pattern}")),
    ]
    .boxed()
}

/// A `Filter`, the clause 4.9 `Query`: terms joined by `;` and `|`, with groups nested up to four deep.
pub(crate) fn query() -> BoxedStrategy<String> {
    term()
        .prop_recursive(4, 32, 4, |inner| {
            (vec(inner, 2..4), select(&[";", "|"][..])).prop_map(|(members, operator)| format!("({})", members.join(operator)))
        })
        .boxed()
}
