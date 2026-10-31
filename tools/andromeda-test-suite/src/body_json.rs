//! Matchers on a JSON body read as received, the body of an answer or of a request a broker forwarded to a context
//! source, for a test whose subject is a member the typed model has no place for (an odd `Accept-Language` answer, a
//! registration property that is a `LanguageProperty`, an `@context` a forwarded item must not carry, a member whose
//! absence is the subject).
//!
//! A matcher names a `JSONPath` (IETF RFC 9535) and what the body must hold there. A body that is not JSON, or a path
//! that is not a `JSONPath`, matches nothing and says why, so a requirement of absence never passes on a body that is
//! no document.

use bytes::Bytes;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use regex::Regex;
use serde_json::Value;
use serde_json_path::JsonPath;
use wiremock::Request;

/// What the body is required to hold at the path.
#[derive(Debug, Clone)]
enum Requirement {
    /// At least one node.
    Exists,
    /// No node.
    Absent,
    /// Exactly one node, equal to the JSON.
    Equals(Value),
    /// Exactly one node, a string that matches the pattern anywhere.
    Matches(Regex),
    /// Exactly one node, a string that holds the text.
    ContainsText(String),
    /// Exactly one node, an array that holds the JSON as an item.
    Includes(Value),
    /// One node that is an array or an object with this many items or members, or else this many nodes.
    Count(usize),
}

/// The requirement on the JSON a body holds at one path.
#[derive(Debug, Clone, MatcherBase)]
pub struct BodyJson {
    /// The `JSONPath` text.
    path: String,
    /// What the body must hold there.
    requirement: Requirement,
}

/// A body that selects at least one node at `path`.
#[must_use]
pub fn json_exists(path: &str) -> BodyJson {
    BodyJson::new(path, Requirement::Exists)
}

/// A body that selects no node at `path`.
#[must_use]
pub fn json_absent(path: &str) -> BodyJson {
    BodyJson::new(path, Requirement::Absent)
}

/// A body whose one node at `path` equals `expected`.
#[must_use]
pub fn json_equals(path: &str, expected: impl Into<Value>) -> BodyJson {
    BodyJson::new(path, Requirement::Equals(expected.into()))
}

/// A body whose one node at `path` is a string matching `pattern` anywhere.
///
/// # Errors
///
/// Returns the error of the pattern when it is not a regular expression.
pub fn json_matches(path: &str, pattern: &str) -> Result<BodyJson, regex::Error> {
    Ok(BodyJson::new(path, Requirement::Matches(Regex::new(pattern)?)))
}

/// A body whose one node at `path` is a string holding `text`.
#[must_use]
pub fn json_contains_text(path: &str, text: &str) -> BodyJson {
    BodyJson::new(path, Requirement::ContainsText(text.to_owned()))
}

/// A body whose one node at `path` is an array holding `item`.
#[must_use]
pub fn json_includes(path: &str, item: impl Into<Value>) -> BodyJson {
    BodyJson::new(path, Requirement::Includes(item.into()))
}

/// A body whose node at `path` is an array or object of `count` items or members, or which selects `count` nodes.
#[must_use]
pub fn json_count(path: &str, count: usize) -> BodyJson {
    BodyJson::new(path, Requirement::Count(count))
}

impl BodyJson {
    /// The requirement `requirement` on `path`.
    fn new(path: &str, requirement: Requirement) -> BodyJson {
        BodyJson {
            path: path.to_owned(),
            requirement,
        }
    }

    /// Why `body` breaks the requirement, or `None` when it meets it.
    fn mismatch(&self, body: &[u8]) -> Option<String> {
        let document = match serde_json::from_slice::<Value>(body) {
            Ok(document) => document,
            Err(error) => return Some(format!("is not JSON: {error}")),
        };
        let path = match self.path.parse::<JsonPath>() {
            Ok(path) => path,
            Err(error) => return Some(format!("cannot be queried, {} is not a JSONPath: {error}", self.path)),
        };
        let nodes = path.query(&document).all();
        match &self.requirement {
            Requirement::Exists => nodes.is_empty().then(|| "selects nothing".to_owned()),
            Requirement::Absent => (!nodes.is_empty()).then(|| format!("selects {} node(s): {nodes:?}", nodes.len())),
            Requirement::Equals(expected) => single(&nodes, |node| node == expected),
            Requirement::Matches(pattern) => single(&nodes, |node| node.as_str().is_some_and(|text| pattern.is_match(text))),
            Requirement::ContainsText(text) => single(&nodes, |node| node.as_str().is_some_and(|held| held.contains(text.as_str()))),
            Requirement::Includes(item) => single(&nodes, |node| node.as_array().is_some_and(|items| items.contains(item))),
            Requirement::Count(count) => {
                let held = match nodes.as_slice() {
                    [Value::Array(items)] => items.len(),
                    [Value::Object(members)] => members.len(),
                    [] | [_] | [_, _, ..] => nodes.len(),
                };
                (held != *count).then(|| format!("counts {held}"))
            }
        }
    }

    /// What the requirement says, for a description.
    fn requirement_text(&self) -> String {
        match &self.requirement {
            Requirement::Exists => format!("selects a node at {}", self.path),
            Requirement::Absent => format!("selects no node at {}", self.path),
            Requirement::Equals(expected) => format!("holds {expected} at {}", self.path),
            Requirement::Matches(pattern) => format!("holds a string matching {pattern} at {}", self.path),
            Requirement::ContainsText(text) => format!("holds a string containing {text:?} at {}", self.path),
            Requirement::Includes(item) => format!("holds an array including {item} at {}", self.path),
            Requirement::Count(count) => format!("counts {count} at {}", self.path),
        }
    }
}

/// Why the one node of `nodes` is not accepted by `accepts`, or why there is not exactly one.
fn single(nodes: &[&Value], accepts: impl Fn(&Value) -> bool) -> Option<String> {
    match nodes {
        [node] => (!accepts(node)).then(|| format!("holds {node}")),
        [] => Some("selects nothing".to_owned()),
        [_, _, ..] => Some(format!("selects {} nodes", nodes.len())),
    }
}

impl BodyJson {
    /// The verdict on `body`.
    fn verdict(&self, body: &[u8]) -> MatcherResult {
        if self.mismatch(body).is_none() {
            MatcherResult::Match
        } else {
            MatcherResult::NoMatch
        }
    }

    /// The description of the requirement or its negation.
    fn describe_result(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is a body that {}", self.requirement_text()).into(),
            MatcherResult::NoMatch => format!("is not a body that {}", self.requirement_text()).into(),
        }
    }
}

impl Matcher<&Bytes> for BodyJson {
    fn matches(&self, actual: &Bytes) -> MatcherResult {
        self.verdict(actual)
    }

    fn describe(&self, result: MatcherResult) -> Description {
        self.describe_result(result)
    }

    fn explain_match(&self, actual: &Bytes) -> Description {
        self.mismatch(actual).unwrap_or_default().into()
    }
}

impl Matcher<&[u8]> for BodyJson {
    fn matches(&self, actual: &[u8]) -> MatcherResult {
        self.verdict(actual)
    }

    fn describe(&self, result: MatcherResult) -> Description {
        self.describe_result(result)
    }

    fn explain_match(&self, actual: &[u8]) -> Description {
        self.mismatch(actual).unwrap_or_default().into()
    }
}

impl Matcher<&Request> for BodyJson {
    fn matches(&self, actual: &Request) -> MatcherResult {
        self.verdict(&actual.body)
    }

    fn describe(&self, result: MatcherResult) -> Description {
        self.describe_result(result)
    }

    fn explain_match(&self, actual: &Request) -> Description {
        format!(
            "whose body {}: {}",
            self.mismatch(&actual.body).unwrap_or_default(),
            String::from_utf8_lossy(&actual.body)
        )
        .into()
    }
}

#[cfg(test)]
mod tests {
    use crate::body_json::{json_absent, json_contains_text, json_count, json_equals, json_exists, json_includes, json_matches};
    use bytes::Bytes;
    use googletest::prelude::*;
    use serde_json::json;

    fn body() -> Bytes {
        Bytes::from_static(br#"[{"id":"a","label":{"type":"Property","value":"Hoofdbron","tags":["x","y"]}}]"#)
    }

    #[gtest]
    fn a_member_is_found_equal_and_counted() {
        let body = body();
        expect_that!(&body, json_exists("$[0].label.value"));
        expect_that!(&body, json_equals("$[0].label.value", "Hoofdbron"));
        expect_that!(&body, json_count("$", 1));
        expect_that!(&body, json_count("$[0].label.tags", 2));
        expect_that!(&body, json_includes("$[0].label.tags", json!("y")));
        expect_that!(&body, json_contains_text("$[0].label.value", "bron"));
    }

    #[gtest]
    fn a_missing_member_or_a_wrong_value_does_not_match() -> Result<()> {
        let body = body();
        expect_that!(&body, json_absent("$[0].label.languageMap"));
        expect_that!(&body, not(json_exists("$[0].label.languageMap")));
        expect_that!(&body, not(json_equals("$[0].label.value", "Source")));
        expect_that!(&body, json_matches("$[0].id", "^(a|b)$")?);
        expect_that!(&body, not(json_matches("$[0].id", "^c$")?));
        Ok(())
    }

    #[gtest]
    fn a_body_that_is_not_json_matches_nothing() {
        let body = Bytes::from_static(b"not json");
        expect_that!(&body, not(json_exists("$")));
        expect_that!(&body, not(json_absent("$")));
    }

    #[gtest]
    fn a_value_is_met_by_one_equal_node_only() {
        let body = Bytes::from_static(br#"[{"speed":{"value":42}},{"speed":{"value":42}}]"#);
        expect_that!(&body, json_equals("$[0].speed.value", 42));
        expect_that!(&body, not(json_equals("$[*].speed.value", 42)));
        expect_that!(&body, json_count("$", 2));
    }
}
