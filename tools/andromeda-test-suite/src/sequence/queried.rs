//! What a query of an answer yields, typed the way the predicates and filters read it.
//!
//! A JSON path query yields nothing when no node matches, the node's value when exactly one matches, and the list of
//! values when several do. A header yields nothing when absent, its value when sent once and the list of values when
//! sent several times. JSON numbers keep their kind: `4` is an integer and `4.0` a float, and the two compare equal.

use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde_json::{Map, Number, Value as Json};
use std::fmt::{self, Display};

/// A value a query or a filter yields.
#[derive(Debug, Clone, PartialEq)]
pub enum Queried {
    /// No value: an absent header, a JSON path that matches no node, a regular expression that does not match.
    Nothing,
    /// The JSON `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A JSON number written without fraction or exponent, or a count.
    Integer(i64),
    /// Any other number.
    Float(f64),
    /// A string.
    String(String),
    /// A list: a JSON array, several JSON path nodes or several header values.
    List(Vec<Queried>),
    /// A JSON object.
    Object(Map<String, Json>),
    /// The body as bytes.
    Bytes(Bytes),
    /// An instant a date filter read.
    Date(DateTime<Utc>),
}

impl Queried {
    /// The value of a JSON document.
    #[must_use]
    pub fn of_json(json: &Json) -> Queried {
        match json {
            Json::Null => Queried::Null,
            Json::Bool(flag) => Queried::Bool(*flag),
            Json::Number(number) => Queried::of_number(number),
            Json::String(text) => Queried::String(text.clone()),
            Json::Array(items) => Queried::List(items.iter().map(Queried::of_json).collect()),
            Json::Object(members) => Queried::Object(members.clone()),
        }
    }

    /// The value of a JSON number: an integer when it has no fraction and fits, a float otherwise.
    fn of_number(number: &Number) -> Queried {
        match (number.as_i64(), number.as_f64()) {
            (Some(integer), _) => Queried::Integer(integer),
            (None, Some(float)) => Queried::Float(float),
            (None, None) => Queried::Nothing,
        }
    }

    /// The value of JSON path nodes: nothing for none, the value for one, the list of values for several.
    #[must_use]
    pub fn of_nodes(nodes: &[&Json]) -> Queried {
        match nodes {
            [] => Queried::Nothing,
            [single] => Queried::of_json(single),
            several => Queried::List(several.iter().map(|node| Queried::of_json(node)).collect()),
        }
    }

    /// The value as JSON, when it has one.
    #[must_use]
    pub fn to_json(&self) -> Option<Json> {
        match self {
            Queried::Nothing | Queried::Bytes(_) => None,
            Queried::Null => Some(Json::Null),
            Queried::Bool(flag) => Some(Json::Bool(*flag)),
            Queried::Integer(integer) => Some(Json::from(*integer)),
            Queried::Float(float) => Number::from_f64(*float).map(Json::Number),
            Queried::String(text) => Some(Json::String(text.clone())),
            Queried::List(items) => items.iter().map(Queried::to_json).collect::<Option<Vec<_>>>().map(Json::Array),
            Queried::Object(members) => Some(Json::Object(members.clone())),
            Queried::Date(instant) => Some(Json::String(instant.to_rfc3339())),
        }
    }

    /// The name of the value's kind, for failure messages.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Queried::Nothing => "none",
            Queried::Null => "null",
            Queried::Bool(_) => "boolean",
            Queried::Integer(_) => "integer",
            Queried::Float(_) => "float",
            Queried::String(_) => "string",
            Queried::List(_) => "list",
            Queried::Object(_) => "object",
            Queried::Bytes(_) => "bytes",
            Queried::Date(_) => "date",
        }
    }

    /// Whether the two values are equal: numbers by value whatever their kind, everything else by kind and content.
    #[must_use]
    pub fn equals(&self, other: &Queried) -> bool {
        match (self, other) {
            (Queried::Integer(left), Queried::Integer(right)) => left == right,
            (Queried::Integer(_) | Queried::Float(_), Queried::Integer(_) | Queried::Float(_)) => self
                .as_float()
                .zip(other.as_float())
                .is_some_and(|(left, right)| left.total_cmp(&right).is_eq()),
            (Queried::List(left), Queried::List(right)) => left.len() == right.len() && left.iter().zip(right).all(|(left, right)| left.equals(right)),
            (Queried::Nothing, _) | (_, Queried::Nothing) => false,
            (left, right) => left == right,
        }
    }

    /// The number as a float.
    #[expect(clippy::cast_precision_loss, reason = "an integer compared with a float is compared as a float")]
    pub(crate) const fn as_float(&self) -> Option<f64> {
        match self {
            Queried::Integer(integer) => Some(*integer as f64),
            Queried::Float(float) => Some(*float),
            Queried::Nothing
            | Queried::Null
            | Queried::Bool(_)
            | Queried::String(_)
            | Queried::List(_)
            | Queried::Object(_)
            | Queried::Bytes(_)
            | Queried::Date(_) => None,
        }
    }

    /// The text a template places for the value: a string as it is, anything else as JSON.
    #[must_use]
    pub fn template_text(&self) -> Option<String> {
        match self {
            Queried::String(text) => Some(text.clone()),
            Queried::Nothing | Queried::Bytes(_) => None,
            Queried::Date(instant) => Some(instant.to_rfc3339()),
            other @ (Queried::Null | Queried::Bool(_) | Queried::Integer(_) | Queried::Float(_) | Queried::List(_) | Queried::Object(_)) => {
                other.to_json().map(|json| json.to_string())
            }
        }
    }
}

impl Display for Queried {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Queried::Nothing => write!(formatter, "none"),
            Queried::Bytes(bytes) => write!(formatter, "bytes <{} bytes>", bytes.len()),
            other @ (Queried::Null
            | Queried::Bool(_)
            | Queried::Integer(_)
            | Queried::Float(_)
            | Queried::String(_)
            | Queried::List(_)
            | Queried::Object(_)
            | Queried::Date(_)) => match other.to_json() {
                Some(json) => write!(formatter, "{} <{json}>", other.kind()),
                None => write!(formatter, "{}", other.kind()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sequence::queried::Queried;
    use googletest::prelude::*;
    use serde_json::json;

    #[gtest]
    fn one_node_is_its_value_and_several_are_a_list() {
        let first = json!(7);
        let second = json!(8);
        expect_that!(Queried::of_nodes(&[]), eq(&Queried::Nothing));
        expect_that!(Queried::of_nodes(&[&first]), eq(&Queried::Integer(7)));
        expect_that!(
            Queried::of_nodes(&[&first, &second]),
            eq(&Queried::List(vec![Queried::Integer(7), Queried::Integer(8)]))
        );
    }

    #[gtest]
    fn an_integer_equals_the_float_of_the_same_value() {
        expect_that!(Queried::of_json(&json!(4)).equals(&Queried::of_json(&json!(4.0))), eq(true));
        expect_that!(Queried::of_json(&json!(4.0)), eq(&Queried::Float(4.0)));
    }

    #[gtest]
    fn nothing_equals_nothing_else() {
        expect_that!(Queried::Nothing.equals(&Queried::Null), eq(false));
        expect_that!(Queried::Nothing.equals(&Queried::Nothing), eq(false));
    }

    #[gtest]
    fn a_string_fills_a_template_as_written_and_a_number_as_json() {
        expect_that!(Queried::String("a b".to_owned()).template_text(), some(eq("a b")));
        expect_that!(Queried::Integer(12).template_text(), some(eq("12")));
        expect_that!(Queried::Nothing.template_text(), none());
    }
}
