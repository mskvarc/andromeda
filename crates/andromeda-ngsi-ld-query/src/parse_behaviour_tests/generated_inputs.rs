//! Texts generated over the grammars of the entry points other than `q`,
//! written as a client writes them, with whitespace where each grammar skips
//! it.

use crate::filter::generated_filters::written_value_path;
use proptest::{
    collection::vec,
    prelude::{BoxedStrategy, Just, Strategy, prop_oneof},
    sample::select,
};
use std::ops::Range;

/// No whitespace, and the whitespace characters every language skips around
/// a name.
const SPACES: [&str; 3] = ["", " ", "\t"];

/// Entity type names: plain, prefixed, an IRI, and one holding a digit and an
/// underscore.
const TYPE_NAMES: [&str; 5] = ["A", "Vehicle", "ex:Building", "https://example.com/T", "T_1"];

/// Scope levels, the single-level wildcard among them.
const SCOPE_LEVELS: [&str; 5] = ["A", "Madrid", "b_1", "x-y", "+"];

/// Attribute names of a projection list, a prefixed one among them.
const PROJECTED_NAMES: [&str; 5] = ["a", "speed", "brandName", "ex:name", "r"];

/// Names an `orderBy` term may give in place of a value path.
const ORDERING_KEYWORDS: [&str; 5] = ["id", "type", "scope", "createdAt", "modifiedAt"];

/// The structural members an `aggrGroupBy` key names.
const ENTITY_MEMBERS: [&str; 5] = ["id", "type", "scope", "datasetId", "unitCode"];

/// The temporal properties a bucket applies to.
const TEMPORAL_PROPERTIES: [&str; 4] = ["observedAt", "createdAt", "modifiedAt", "deletedAt"];

/// The bucket tokens of an `aggrGroupBy` term.
const BUCKETS: [&str; 8] = ["year", "month", "week", "day", "hour", "dayOfWeek", "hourOfDay", "monthOfYear"];

/// One of `texts`.
fn one_of(texts: &'static [&'static str]) -> BoxedStrategy<String> {
    select(texts).prop_map(str::to_owned).boxed()
}

/// `text` with whitespace drawn before and after it.
fn spaced(text: BoxedStrategy<String>) -> BoxedStrategy<String> {
    (select(&SPACES[..]), text, select(&SPACES[..]))
        .prop_map(|(before, text, after)| format!("{before}{text}{after}"))
        .boxed()
}

/// One `item` followed by `further` more, each after one of `separators`.
fn separated(item: BoxedStrategy<String>, separators: &'static [&'static str], further: Range<usize>) -> BoxedStrategy<String> {
    (item.clone(), vec((select(separators), item), further))
        .prop_map(|(first, rest)| rest.into_iter().fold(first, |text, (separator, item)| format!("{text}{separator}{item}")))
        .boxed()
}

/// An entity type selection of clause 4.17: the wildcard, or alternatives
/// joined by `|` or `,`, each a type or a parenthesised conjunction.
pub(super) fn type_selection() -> BoxedStrategy<String> {
    let name = spaced(one_of(&TYPE_NAMES));
    let group = separated(name.clone(), &[";"], 0..3).prop_map(|types| format!("({types})")).boxed();
    prop_oneof![1 => Just("*".to_owned()), 9 => separated(prop_oneof![name, group].boxed(), &["|", ","], 0..4)].boxed()
}

/// One scope pattern of clause 4.19: levels after a `/`, or one written
/// without it, optionally closed by the multi-level wildcard, or the
/// multi-level wildcard alone.
pub(super) fn scope_pattern() -> BoxedStrategy<String> {
    let levels = (select(&["/", ""][..]), vec(select(&SCOPE_LEVELS[..]), 1..4), select(&["", "/#"][..]))
        .prop_map(|(opening, levels, closing)| format!("{opening}{}{closing}", levels.join("/")));
    spaced(prop_oneof![9 => levels, 1 => Just("/#".to_owned())].boxed())
}

/// A `scopeQ` expression of clause 4.19: alternatives joined by `|` or `,`,
/// each a pattern or a parenthesised conjunction.
pub(super) fn scope_query() -> BoxedStrategy<String> {
    let pattern = scope_pattern();
    let group = separated(pattern.clone(), &[";"], 0..3).prop_map(|patterns| format!("({patterns})")).boxed();
    separated(prop_oneof![pattern, group].boxed(), &["|", ","], 0..4)
}

/// A `pick` or `omit` list of clause 4.21: terms joined by `,` or `|`, a term
/// opening a braced list of linked-entity terms up to three deep.
pub(super) fn projection_list() -> BoxedStrategy<String> {
    let term = spaced(one_of(&PROJECTED_NAMES)).prop_recursive(3, 16, 3, |inner| {
        (select(&PROJECTED_NAMES[..]), separated(inner, &[",", "|"], 0..3)).prop_map(|(relationship, terms)| format!("{relationship}{{{terms}}}"))
    });
    separated(term.boxed(), &[",", "|"], 0..4)
}

/// An `orderBy` term of clause 4.23: a value path or a keyword, optionally
/// followed by `;` and a direction.
pub(super) fn ordering_term() -> BoxedStrategy<String> {
    let target = prop_oneof![8 => written_value_path(), 2 => one_of(&ORDERING_KEYWORDS)];
    let direction = prop_oneof![
        Just(String::new()),
        (select(&SPACES[..]), select(&["asc", "desc"][..])).prop_map(|(space, direction)| format!(";{space}{direction}")),
    ];
    (spaced(target.boxed()), direction)
        .prop_map(|(target, direction)| format!("{target}{direction}"))
        .boxed()
}

/// An `aggrGroupBy` key: an entity member, a temporal property or a value
/// path.
pub(super) fn group_key() -> BoxedStrategy<String> {
    prop_oneof![one_of(&ENTITY_MEMBERS), one_of(&TEMPORAL_PROPERTIES), written_value_path()].boxed()
}

/// An `aggrGroupBy` value: terms joined by `,`, each a key, or a temporal
/// property with a bucket after `;`.
pub(super) fn aggregation_grouping() -> BoxedStrategy<String> {
    let bucketed = (select(&TEMPORAL_PROPERTIES[..]), select(&SPACES[..]), select(&BUCKETS[..]))
        .prop_map(|(property, space, bucket)| format!("{property};{space}{bucket}"));
    separated(spaced(prop_oneof![group_key(), bucketed].boxed()), &[","], 0..3)
}
