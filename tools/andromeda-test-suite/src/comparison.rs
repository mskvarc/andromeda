//! The exact comparison of a JSON answer with an expected document. Two documents agree when:
//!
//! - an object holds exactly the expected members, except members the comparison hides, which may be present or
//!   absent: below the root for a name given to [`Expectation::hiding`], everywhere for one given to
//!   [`Expectation::hiding_everywhere`] (the `id` a notification check hides);
//! - a list holds as many items as expected, paired regardless of order: by `id` when every item on both sides is an
//!   object with one, otherwise each expected item with a distinct actual item that agrees with it;
//! - `observedAt` is the same instant as expected, to the microsecond; inside a list item, where items pair by their
//!   text, it is the expected text;
//! - `createdAt`, `modifiedAt` and `deletedAt` are well-formed dates, whatever their value;
//! - an `@context` string equals a one-element list holding it, or, under [`ContextRule::AnyCoreContextVersion`], any
//!   lone versioned core `@context` stands for the expected one;
//! - an integer differs from a float of the same value outside any list, and equals it inside a list item, where
//!   items pair by value whatever the number type;
//! - everything else is equal.
//!
//! Every item of a list counts, so a list that repeats an item does not agree with one that holds it once.

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

/// The versioned NGSI-LD core `@context`s, any of which [`ContextRule::AnyCoreContextVersion`] accepts.
static CORE_CONTEXT: LazyLock<Result<Regex, regex::Error>> =
    LazyLock::new(|| Regex::new(r"^https://uri\.etsi\.org/ngsi-ld/v1/ngsi-ld-core-context-v\d\.\d\.jsonld$"));

/// The system-generated temporal properties accepted with any well-formed date.
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
    /// Member names hidden in every object below the compared root.
    hidden_below_root: BTreeSet<String>,
    /// Member names hidden at every depth, the root included.
    hidden_everywhere: BTreeSet<String>,
    /// How `@context` compares.
    context: ContextRule,
}

/// How two values are compared: member by member, or as two items of a list pair by their content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A member by member comparison: `observedAt` as an instant, system timestamps as any date, `@context` under the
    /// expectation's rule, and an integer apart from a float of the same value.
    Diff,
    /// The pairing of list items by content: `observedAt`, system timestamps and `@context` as text, numbers by value
    /// whatever their type, so an item pairs with the one that spells the same number as `1` or `1.0`.
    Hash,
}

/// Matches a JSON answer that agrees with `expected` under the rules of this module.
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

mod list_pairing;

#[cfg(test)]
mod tests;
