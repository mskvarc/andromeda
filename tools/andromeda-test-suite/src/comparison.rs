//! The exact comparison of a JSON answer with an expected document, as the ETSI suite's `Compare Dictionaries
//! Ignoring Keys` makes it.
//!
//! The suite runs DeepDiff with `ignore_order=True` and the suite's own operators. Two documents agree when:
//!
//! - an object holds exactly the expected members, except members the comparison hides, which may be present or
//!   absent: below the root for an exclude pattern such as `root\[.*\]\['instanceId'\]`, everywhere for one naming
//!   the text alone (the `id` a notification check hides);
//! - a list holds as many items as expected, paired regardless of order: by `id` when every item on both sides is an
//!   object with one, otherwise each expected item with a distinct actual item that agrees with it;
//! - `observedAt` is the same instant as expected, to the microsecond; below a list, where DeepDiff pairs items by
//!   their text, it is the expected text;
//! - `createdAt`, `modifiedAt` and `deletedAt` are well-formed dates, whatever their value;
//! - an `@context` string equals a one-element list holding it, or, under [`ContextRule::AnyCoreContextVersion`], any
//!   lone versioned core `@context` stands for the expected one;
//! - an integer differs from a float of the same value outside any list, and equals it below one, where DeepDiff
//!   pairs items by a hash that does not see the type;
//! - everything else is equal.
//!
//! DeepDiff ignores how often an item repeats; the comparison counts every item, which is stricter for an answer
//! that repeats one.

pub mod difference;
pub mod document_path;
mod leaf_value;

use crate::comparison::{
    difference::{Difference, Discrepancy},
    document_path::DocumentPath,
    leaf_value::{compare_numbers, compare_observed_at, compare_system_timestamp},
};
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use regex::Regex;
use serde_json::{Map, Value};
use std::{collections::BTreeSet, sync::LazyLock};

/// The versioned NGSI-LD core `@context`s, any of which the AnyCoreContextVersion operator accepts.
static CORE_CONTEXT: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^https://uri\.etsi\.org/ngsi-ld/v1/ngsi-ld-core-context-v\d\.\d\.jsonld$"));

/// The system-generated temporal properties the suite accepts with any well-formed date.
const SYSTEM_TIMESTAMPS: [&str; 3] = ["createdAt", "modifiedAt", "deletedAt"];

/// How an `@context` member is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ContextRule {
    /// A string equals a one-element list holding it; anything else compares exactly.
    #[default]
    StringOrSingleList,
    /// Any lone versioned core `@context`, as a string or a one-element list, stands for the expected one; anything
    /// else compares exactly, and an expected string is matched only by that string.
    AnyCoreContextVersion,
}

/// An expected document and the rules it is compared under.
#[derive(Debug, Clone, MatcherBase)]
pub struct Expectation {
    /// The document.
    expected: Value,
    /// Member names hidden below the compared root, as an exclude pattern `root\[.*\]\['<name>'\]` hides them.
    hidden_below_root: BTreeSet<String>,
    /// Member names hidden at every depth, the root included, as an exclude pattern naming the text alone hides them.
    hidden_everywhere: BTreeSet<String>,
    /// How `@context` compares.
    context: ContextRule,
}

/// How two values are compared: as DeepDiff diffs two members, or as its hash pairs two items of a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A member by member diff: `observedAt` as an instant, system timestamps as any date, the suite's `@context`
    /// operators, and an integer apart from a float of the same value.
    Diff,
    /// The hash of a list item: `observedAt`, system timestamps and `@context` as text, numbers by value whatever
    /// their type.
    Hash,
}

/// Matches a JSON answer that agrees with `expected` under the suite's rules.
#[must_use]
pub fn matches_document(expected: Value) -> Expectation {
    Expectation {
        expected,
        hidden_below_root: BTreeSet::new(),
        hidden_everywhere: BTreeSet::new(),
        context: ContextRule::default(),
    }
}

impl Expectation {
    /// Hides the member `name` in every object below the compared root: there it may be present or absent and is
    /// never compared, while the root's own members are counted exactly.
    #[must_use]
    pub fn hiding(mut self, name: &str) -> Expectation {
        self.hidden_below_root.insert(name.to_owned());
        self
    }

    /// Hides the member `name` in every object, the compared root included.
    #[must_use]
    pub fn hiding_everywhere(mut self, name: &str) -> Expectation {
        self.hidden_everywhere.insert(name.to_owned());
        self
    }

    /// Whether member `name` of the object at `path` is hidden; the root is the object at `$`.
    fn hides(&self, path: &DocumentPath, name: &str) -> bool {
        self.hidden_everywhere.contains(name) || (!path.is_root() && self.hidden_below_root.contains(name))
    }

    /// Compares `@context` under `rule`.
    #[must_use]
    pub const fn context_rule(mut self, rule: ContextRule) -> Expectation {
        self.context = rule;
        self
    }

    /// Every difference between `actual` and the expectation, empty when they agree.
    #[must_use]
    pub fn differences(&self, actual: &Value) -> Vec<Difference> {
        let mut differences = Vec::new();
        self.compare(&DocumentPath::default(), None, actual, &self.expected, Mode::Diff, &mut differences);
        differences
    }

    /// Compares `actual` with `expected` at `path`, the value of member `key` when it is one.
    fn compare(&self, path: &DocumentPath, key: Option<&str>, actual: &Value, expected: &Value, mode: Mode, out: &mut Vec<Difference>) {
        match (key, mode) {
            (Some("observedAt"), Mode::Diff) => compare_observed_at(path, actual, expected, out),
            (Some(name), Mode::Diff) if SYSTEM_TIMESTAMPS.contains(&name) => compare_system_timestamp(path, actual, out),
            (Some("@context"), Mode::Diff) => self.compare_context(path, actual, expected, out),
            (Some(_) | None, Mode::Diff | Mode::Hash) => self.compare_value(path, actual, expected, mode, out),
        }
    }

    /// Compares two values by their JSON kind.
    fn compare_value(&self, path: &DocumentPath, actual: &Value, expected: &Value, mode: Mode, out: &mut Vec<Difference>) {
        match (actual, expected) {
            (Value::Object(actual), Value::Object(expected)) => self.compare_objects(path, actual, expected, mode, out),
            (Value::Array(actual), Value::Array(expected)) => self.compare_lists(path, actual, expected, mode, out),
            (Value::Number(actual), Value::Number(expected)) => compare_numbers(path, actual, expected, mode, out),
            (actual, expected) if actual == expected => {}
            (actual, expected) => out.push(Difference::at(
                path,
                Discrepancy::Unequal {
                    actual: actual.clone(), // clone: the difference owns the values it reports
                    expected: expected.clone(),
                },
            )),
        }
    }

    /// Compares two objects member by member.
    fn compare_objects(&self, path: &DocumentPath, actual: &Map<String, Value>, expected: &Map<String, Value>, mode: Mode, out: &mut Vec<Difference>) {
        for (name, expected) in expected.iter().filter(|(name, _)| !self.hides(path, name)) {
            let member = path.member(name);
            match actual.get(name) {
                Some(actual) => self.compare(&member, Some(name), actual, expected, mode, out),
                None => out.push(Difference::at(&member, Discrepancy::Missing(expected.clone()))), // clone: the difference owns the values it reports
            }
        }
        out.extend(
            actual
                .iter()
                .filter(|(name, _)| !expected.contains_key(*name) && !self.hides(path, name))
                .map(|(name, value)| Difference::at(&path.member(name), Discrepancy::Unexpected(value.clone()))), // clone: the difference owns the values it reports
        );
    }

    /// Compares two lists regardless of order.
    fn compare_lists(&self, path: &DocumentPath, actual: &[Value], expected: &[Value], mode: Mode, out: &mut Vec<Difference>) {
        if actual.len() != expected.len() {
            out.push(Difference::at(
                path,
                Discrepancy::Length {
                    actual: actual.len(),
                    expected: expected.len(),
                },
            ));
            return;
        }
        match (ids(actual), ids(expected), mode) {
            (Some(actual_ids), Some(expected_ids), _) => self.compare_by_id(path, (actual, &actual_ids), (expected, &expected_ids), mode, out),
            (Some(_) | None, Some(_) | None, Mode::Hash) => self.compare_by_hash(path, actual, expected, out),
            (Some(_) | None, Some(_) | None, Mode::Diff) => self.compare_by_pairing(path, actual, expected, out),
        }
    }

    /// Pairs the items of two lists by their `id` and compares each pair: a keyword grouping by `id` diffs each
    /// element as a member.
    fn compare_by_id(
        &self,
        path: &DocumentPath,
        (actual, actual_ids): (&[Value], &[&str]),
        (expected, expected_ids): (&[Value], &[&str]),
        mode: Mode,
        out: &mut Vec<Difference>,
    ) {
        for (expected, id) in expected.iter().zip(expected_ids) {
            let item = path.item_with_id(id);
            match actual_ids.iter().position(|actual_id| actual_id == id) {
                Some(index) => self.compare(&item, None, &actual[index], expected, mode, out),
                None => out.push(Difference::at(&item, Discrepancy::MissingItem)),
            }
        }
    }

    /// Pairs the items of two lists inside a hashed item: every expected item with a distinct hash-equal one.
    fn compare_by_hash(&self, path: &DocumentPath, actual: &[Value], expected: &[Value], out: &mut Vec<Difference>) {
        let item = path.any_item();
        let partner = self.pairing(&item, &actual.iter().collect::<Vec<_>>(), &expected.iter().collect::<Vec<_>>(), Mode::Hash);
        out.extend(
            expected
                .iter()
                .zip(&partner)
                .filter(|(_, paired)| paired.is_none())
                .map(|(value, _)| Difference::at(&item, Discrepancy::Unpaired((*value).clone()))), // clone: the difference owns the values it reports
        );
    }

    /// Pairs the items of two lists as DeepDiff does with `ignore_order`: items with equal hashes first; then, while
    /// few items are left (`cutoff_intersection_for_pairs`, 0.7 of both lists), each left expected item with a
    /// distinct left actual item it agrees with member by member.
    fn compare_by_pairing(&self, path: &DocumentPath, actual: &[Value], expected: &[Value], out: &mut Vec<Difference>) {
        let item = path.any_item();
        let hashed = self.pairing(&item, &actual.iter().collect::<Vec<_>>(), &expected.iter().collect::<Vec<_>>(), Mode::Hash);
        let left_expected = hashed
            .iter()
            .enumerate()
            .filter(|(_, paired)| paired.is_none())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let taken = hashed.iter().flatten().copied().collect::<BTreeSet<_>>();
        let left_actual = (0..actual.len()).filter(|index| !taken.contains(index)).collect::<Vec<_>>();
        if left_expected.is_empty() {
            return;
        }
        if 20 * left_expected.len() > 7 * (2 * expected.len() + 1) {
            out.push(Difference::at(path, Discrepancy::BeyondPairingCutoff(left_expected.len())));
            out.extend(
                left_expected
                    .iter()
                    .flat_map(|index| self.closest_differences(&item, &left_actual, actual, &expected[*index])),
            );
            return;
        }
        let actual_left = left_actual.iter().map(|index| &actual[*index]).collect::<Vec<_>>();
        let expected_left = left_expected.iter().map(|index| &expected[*index]).collect::<Vec<_>>();
        let diffed = self.pairing(&item, &actual_left, &expected_left, Mode::Diff);
        out.extend(
            expected_left
                .iter()
                .zip(&diffed)
                .filter(|(_, paired)| paired.is_none())
                .map(|(value, _)| Difference::at(&item, Discrepancy::Unpaired((*value).clone()))), // clone: the difference owns the values it reports
        );
    }

    /// The member differences between `expected` and the item among `candidates` (indexes into `actual`) it differs
    /// from least, to show what a list that DeepDiff refuses to pair differs in.
    fn closest_differences(&self, item: &DocumentPath, candidates: &[usize], actual: &[Value], expected: &Value) -> Vec<Difference> {
        candidates
            .iter()
            .map(|index| {
                let mut differences = Vec::new();
                self.compare(item, None, &actual[*index], expected, Mode::Diff, &mut differences);
                differences
            })
            .min_by_key(Vec::len)
            .unwrap_or_default()
    }

    /// The actual item paired with each expected item, under `mode`, along augmenting paths (Kuhn's algorithm).
    fn pairing(&self, item: &DocumentPath, actual: &[&Value], expected: &[&Value], mode: Mode) -> Vec<Option<usize>> {
        let agrees = expected
            .iter()
            .map(|expected| actual.iter().map(|actual| self.agrees(item, actual, expected, mode)).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let mut partner = vec![None; actual.len()];
        for index in 0..expected.len() {
            let mut visited = vec![false; actual.len()];
            augment(index, &agrees, &mut partner, &mut visited);
        }
        let mut paired = vec![None; expected.len()];
        for (actual_index, expected_index) in partner.iter().enumerate() {
            if let Some(expected_index) = expected_index {
                paired[*expected_index] = Some(actual_index);
            }
        }
        paired
    }

    /// Whether `actual` agrees with `expected` under `mode`, as an item of a list at `item`.
    fn agrees(&self, item: &DocumentPath, actual: &Value, expected: &Value, mode: Mode) -> bool {
        let mut differences = Vec::new();
        self.compare(item, None, actual, expected, mode, &mut differences);
        differences.is_empty()
    }

    /// Compares an `@context` under the expectation's rule.
    fn compare_context(&self, path: &DocumentPath, actual: &Value, expected: &Value, out: &mut Vec<Difference>) {
        let accepted = match self.context {
            ContextRule::StringOrSingleList => single(actual).zip(single(expected)).is_some_and(|(actual, expected)| actual == expected),
            ContextRule::AnyCoreContextVersion => single(actual).is_some_and(|actual| CORE_CONTEXT.as_ref().is_ok_and(|core| core.is_match(actual))),
        };
        if !accepted {
            self.compare_value(path, actual, expected, Mode::Diff, out);
        }
    }
}

/// The string an `@context` names alone, as a string or a one-element list.
fn single(value: &Value) -> Option<&str> {
    match value {
        Value::String(text) => Some(text),
        Value::Array(items) => match items.as_slice() {
            [Value::String(text)] => Some(text),
            [] | [_] | [_, _, ..] => None,
        },
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::Object(_) => None,
    }
}

/// The `id` of every item, when each is an object with a string `id`.
fn ids(items: &[Value]) -> Option<Vec<&str>> {
    items.iter().map(|item| item.get("id").and_then(Value::as_str)).collect()
}

/// Finds a partner for expected item `item` along an augmenting path (Kuhn's algorithm).
fn augment(item: usize, agrees: &[Vec<bool>], partner: &mut [Option<usize>], visited: &mut [bool]) -> bool {
    for candidate in 0..partner.len() {
        if agrees[item][candidate] && !visited[candidate] {
            visited[candidate] = true;
            if partner[candidate].is_none_or(|other| augment(other, agrees, partner, visited)) {
                partner[candidate] = Some(item);
                return true;
            }
        }
    }
    false
}

impl Matcher<&Value> for Expectation {
    fn matches(&self, actual: &Value) -> MatcherResult {
        self.differences(actual).is_empty().into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("agrees with {}", self.expected).into(),
            MatcherResult::NoMatch => format!("doesn't agree with {}", self.expected).into(),
        }
    }

    fn explain_match(&self, actual: &Value) -> Description {
        let differences = self.differences(actual);
        let listed = differences.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
        format!("which differs in {} place(s):\n{listed}", differences.len()).into()
    }
}

#[cfg(test)]
mod tests;
