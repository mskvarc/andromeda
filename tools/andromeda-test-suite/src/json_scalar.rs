//! Matchers on the one scalar a body holds at a `JSONPath`, for a test whose subject is a number compared by value or
//! range, a string by its start or its kind, or a node by its JSON type: the comparisons an aggregated answer or a
//! time member need, which an equality on the JSON text cannot state.
//!
//! A body that is not JSON, a path that is not a `JSONPath`, or one that does not select exactly one node, matches
//! nothing and says why.

use crate::ngsild_date::is_ngsild_date;
use bytes::Bytes;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use serde_json::Value;
use serde_json_path::JsonPath;

/// What the scalar must be.
#[derive(Debug, Clone)]
enum Requirement {
    /// A number equal to the value.
    NumberEquals(f64),
    /// A number greater than the value.
    NumberAbove(f64),
    /// A number less than the value.
    NumberBelow(f64),
    /// A string.
    IsString,
    /// A number.
    IsNumber,
    /// An array.
    IsList,
    /// An object.
    IsObject,
    /// A string starting with the prefix.
    StartsWith(String),
    /// A string that is a date in the `NGSILD_DATE` form.
    IsDate,
}

/// The requirement on the one node a body holds at a path.
#[derive(Debug, Clone, MatcherBase)]
pub struct JsonScalar {
    /// The `JSONPath` text.
    path: String,
    /// What the node must be.
    requirement: Requirement,
}

/// A body whose one node at `path` is a number equal to `expected`.
#[must_use]
pub fn json_number_equals(path: &str, expected: f64) -> JsonScalar {
    JsonScalar::new(path, Requirement::NumberEquals(expected))
}

/// A body whose one node at `path` is a number greater than `bound`.
#[must_use]
pub fn json_number_above(path: &str, bound: f64) -> JsonScalar {
    JsonScalar::new(path, Requirement::NumberAbove(bound))
}

/// A body whose one node at `path` is a number less than `bound`.
#[must_use]
pub fn json_number_below(path: &str, bound: f64) -> JsonScalar {
    JsonScalar::new(path, Requirement::NumberBelow(bound))
}

/// A body whose one node at `path` is a string.
#[must_use]
pub fn json_is_string(path: &str) -> JsonScalar {
    JsonScalar::new(path, Requirement::IsString)
}

/// A body whose one node at `path` is a number.
#[must_use]
pub fn json_is_number(path: &str) -> JsonScalar {
    JsonScalar::new(path, Requirement::IsNumber)
}

/// A body whose one node at `path` is an array.
#[must_use]
pub fn json_is_list(path: &str) -> JsonScalar {
    JsonScalar::new(path, Requirement::IsList)
}

/// A body whose one node at `path` is an object.
#[must_use]
pub fn json_is_object(path: &str) -> JsonScalar {
    JsonScalar::new(path, Requirement::IsObject)
}

/// A body whose one node at `path` is a string starting with `prefix`.
#[must_use]
pub fn json_starts_with(path: &str, prefix: &str) -> JsonScalar {
    JsonScalar::new(path, Requirement::StartsWith(prefix.to_owned()))
}

/// A body whose one node at `path` is a date-time string in the `NGSILD_DATE` form.
#[must_use]
pub fn json_is_date(path: &str) -> JsonScalar {
    JsonScalar::new(path, Requirement::IsDate)
}

impl JsonScalar {
    /// The requirement `requirement` on `path`.
    fn new(path: &str, requirement: Requirement) -> JsonScalar {
        JsonScalar {
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
        let [node] = nodes.as_slice() else {
            return Some(format!("selects {} nodes, one is required", nodes.len()));
        };
        let holds = match &self.requirement {
            Requirement::NumberEquals(expected) => node
                .as_f64()
                .is_some_and(|number| (number - expected).abs() <= f64::EPSILON * expected.abs().max(1.0)),
            Requirement::NumberAbove(bound) => node.as_f64().is_some_and(|number| number > *bound),
            Requirement::NumberBelow(bound) => node.as_f64().is_some_and(|number| number < *bound),
            Requirement::IsString => node.is_string(),
            Requirement::IsNumber => node.is_number(),
            Requirement::IsList => node.is_array(),
            Requirement::IsObject => node.is_object(),
            Requirement::StartsWith(prefix) => node.as_str().is_some_and(|text| text.starts_with(prefix.as_str())),
            Requirement::IsDate => node.as_str().is_some_and(is_ngsild_date),
        };
        (!holds).then(|| format!("holds {node}"))
    }

    /// What the requirement says, for a description.
    fn requirement_text(&self) -> String {
        match &self.requirement {
            Requirement::NumberEquals(expected) => format!("a number equal to {expected}"),
            Requirement::NumberAbove(bound) => format!("a number above {bound}"),
            Requirement::NumberBelow(bound) => format!("a number below {bound}"),
            Requirement::IsString => "a string".to_owned(),
            Requirement::IsNumber => "a number".to_owned(),
            Requirement::IsList => "an array".to_owned(),
            Requirement::IsObject => "an object".to_owned(),
            Requirement::StartsWith(prefix) => format!("a string starting with {prefix:?}"),
            Requirement::IsDate => "a date-time string".to_owned(),
        }
    }

    /// The verdict on `body`.
    fn verdict(&self, body: &[u8]) -> MatcherResult {
        self.mismatch(body).is_none().into()
    }

    /// The description of the requirement or its negation.
    fn describe_result(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is a body holding {} at {}", self.requirement_text(), self.path).into(),
            MatcherResult::NoMatch => format!("is not a body holding {} at {}", self.requirement_text(), self.path).into(),
        }
    }
}

impl Matcher<&Bytes> for JsonScalar {
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

impl Matcher<&[u8]> for JsonScalar {
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

#[cfg(test)]
mod tests {
    use crate::json_scalar::{
        json_is_date,
        json_is_list,
        json_is_number,
        json_is_object,
        json_is_string,
        json_number_above,
        json_number_below,
        json_number_equals,
        json_starts_with,
    };
    use bytes::Bytes;
    use googletest::prelude::*;

    fn body() -> Bytes {
        Bytes::from_static(br#"{"n": 15.0, "i": 7, "s": "09:00:00.5", "d": "2024-01-01T00:00:00Z", "l": [1], "o": [1, 2]}"#)
    }

    #[gtest]
    fn numbers_compare_by_value_whatever_their_spelling() {
        let body = body();
        expect_that!(&body, json_number_equals("$.n", 15.0));
        expect_that!(&body, json_number_equals("$.i", 7.0));
        expect_that!(&body, json_number_above("$.n", 14.99));
        expect_that!(&body, json_number_below("$.n", 15.01));
        expect_that!(&body, not(json_number_equals("$.n", 16.0)));
        expect_that!(&body, json_is_number("$.i"));
    }

    #[gtest]
    fn kinds_and_prefixes_are_read_from_the_node() {
        let body = body();
        expect_that!(&body, json_is_string("$.s"));
        expect_that!(&body, json_starts_with("$.s", "09:00:00"));
        expect_that!(&body, json_is_date("$.d"));
        expect_that!(&body, json_is_list("$.l"));
        expect_that!(&body, json_is_object("$"));
        expect_that!(&body, not(json_is_date("$.s")));
    }

    #[gtest]
    fn a_path_selecting_other_than_one_node_matches_nothing() {
        let body = body();
        expect_that!(&body, not(json_is_string("$.missing")));
        expect_that!(&body, not(json_is_number("$.o[*]")));
    }
}
