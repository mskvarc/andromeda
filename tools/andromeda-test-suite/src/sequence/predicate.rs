//! What a check requires of the value its query yields.
//!
//! A predicate over nothing (an absent header, a JSON path without node) is false, so its negation holds. A predicate
//! that cannot apply to the value it is given (`includes` on a string, `matches` on a number, an order between a
//! string and a number) fails the check whether negated or not.

use crate::sequence::{
    placeholder_name::PlaceholderName,
    queried::Queried,
    variables::{TemplateError, Variables},
};
use chrono::DateTime;
use regex::Regex;
use std::{
    cmp::Ordering,
    fmt::{self, Display},
};

/// A value a predicate compares with. A string is a template, filled when the check runs.
#[derive(Debug, Clone, PartialEq)]
pub enum Expected {
    /// The JSON `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// An integer.
    Integer(i64),
    /// A float.
    Float(f64),
    /// A string template.
    Text(String),
    /// The value of a variable, of whatever kind it holds.
    Variable(String),
}

/// The value of the variable `name`, as an expected value of its own kind.
#[must_use]
pub fn var(name: &str) -> Expected {
    Expected::Variable(name.to_owned())
}

/// The JSON `null`, as an expected value.
pub const NULL: Expected = Expected::Null;

impl From<bool> for Expected {
    fn from(flag: bool) -> Expected {
        Expected::Bool(flag)
    }
}

impl From<i64> for Expected {
    fn from(integer: i64) -> Expected {
        Expected::Integer(integer)
    }
}

impl From<i32> for Expected {
    fn from(integer: i32) -> Expected {
        Expected::Integer(i64::from(integer))
    }
}

impl From<f64> for Expected {
    fn from(float: f64) -> Expected {
        Expected::Float(float)
    }
}

impl From<&str> for Expected {
    fn from(template: &str) -> Expected {
        Expected::Text(template.to_owned())
    }
}

impl Expected {
    /// The value compared with, its template filled.
    fn resolve(&self, variables: &Variables) -> Result<Queried, TemplateError> {
        Ok(match self {
            Expected::Null => Queried::Null,
            Expected::Bool(flag) => Queried::Bool(*flag),
            Expected::Integer(integer) => Queried::Integer(*integer),
            Expected::Float(float) => Queried::Float(*float),
            Expected::Text(template) => Queried::String(variables.fill(template)?),
            Expected::Variable(name) => variables
                .get(name)
                .cloned()
                .ok_or_else(|| TemplateError::Unset(PlaceholderName::new(name.as_str())))?,
        })
    }
}

/// A requirement on a value.
#[derive(Debug, Clone, PartialEq)]
pub enum Predicate {
    /// Equal.
    Equal(Expected),
    /// Not equal.
    NotEqual(Expected),
    /// Greater.
    Greater(Expected),
    /// Greater or equal.
    GreaterOrEqual(Expected),
    /// Less.
    Less(Expected),
    /// Less or equal.
    LessOrEqual(Expected),
    /// A string holds the text, or a list holds an equal item.
    Contains(Expected),
    /// A list holds an equal item.
    Includes(Expected),
    /// A string matches the regular expression, a template.
    Matches(String),
    /// A string starts with the text.
    StartsWith(Expected),
    /// A string ends with the text.
    EndsWith(Expected),
    /// There is a value.
    Exists,
    /// A string.
    IsString,
    /// An integer.
    IsInteger,
    /// A float.
    IsFloat,
    /// A number.
    IsNumber,
    /// A boolean.
    IsBoolean,
    /// A list.
    IsList,
    /// An object.
    IsObject,
    /// A list, an object or bytes.
    IsCollection,
    /// An empty list, object, string or body.
    IsEmpty,
    /// A string holding an RFC 3339 date and time.
    IsIsoDate,
}

/// Why a predicate cannot be evaluated.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PredicateError {
    /// The predicate does not apply to the kind of value.
    #[error("{predicate} does not apply to {value}")]
    Inapplicable {
        /// The predicate.
        predicate: Predicate,
        /// The value.
        value: Queried,
    },
    /// The regular expression is not valid.
    #[error("the filled pattern is not a regular expression")]
    Pattern(#[source] regex::Error),
    /// A template cannot be filled.
    #[error(transparent)]
    Template(#[from] TemplateError),
}

impl Predicate {
    /// Whether `value` satisfies the predicate.
    ///
    /// # Errors
    ///
    /// Returns [`PredicateError`] when the predicate does not apply to the kind of `value`.
    pub fn holds(&self, value: &Queried, variables: &Variables) -> Result<bool, PredicateError> {
        // clone: the error names the predicate and value the caller keeps
        let inapplicable = || PredicateError::Inapplicable {
            predicate: self.clone(),
            value: value.clone(),
        };
        if matches!(value, Queried::Nothing) {
            return Ok(false);
        }
        match self {
            Predicate::Equal(expected) => Ok(value.equals(&expected.resolve(variables)?)),
            Predicate::NotEqual(expected) => Ok(!value.equals(&expected.resolve(variables)?)),
            Predicate::Greater(expected) => order(value, &expected.resolve(variables)?).map(Ordering::is_gt).ok_or_else(inapplicable),
            Predicate::GreaterOrEqual(expected) => order(value, &expected.resolve(variables)?).map(Ordering::is_ge).ok_or_else(inapplicable),
            Predicate::Less(expected) => order(value, &expected.resolve(variables)?).map(Ordering::is_lt).ok_or_else(inapplicable),
            Predicate::LessOrEqual(expected) => order(value, &expected.resolve(variables)?).map(Ordering::is_le).ok_or_else(inapplicable),
            Predicate::Contains(expected) => match (value, expected.resolve(variables)?) {
                (Queried::String(subject), Queried::String(part)) => Ok(subject.contains(&part)),
                (Queried::List(items), item) => Ok(items.iter().any(|candidate| candidate.equals(&item))),
                (Queried::Bytes(bytes), Queried::String(part)) => Ok(bytes.windows(part.len().max(1)).any(|window| window == part.as_bytes())),
                _ => Err(inapplicable()),
            },
            Predicate::Includes(expected) => match value {
                Queried::List(items) => {
                    let item = expected.resolve(variables)?;
                    Ok(items.iter().any(|candidate| candidate.equals(&item)))
                }
                Queried::Nothing
                | Queried::Null
                | Queried::Bool(_)
                | Queried::Integer(_)
                | Queried::Float(_)
                | Queried::String(_)
                | Queried::Object(_)
                | Queried::Bytes(_)
                | Queried::Date(_) => Err(inapplicable()),
            },
            Predicate::Matches(template) => match value {
                Queried::String(subject) => {
                    let expression = variables.fill(template)?;
                    let pattern = Regex::new(&expression).map_err(PredicateError::Pattern)?;
                    Ok(pattern.is_match(subject))
                }
                Queried::Nothing
                | Queried::Null
                | Queried::Bool(_)
                | Queried::Integer(_)
                | Queried::Float(_)
                | Queried::List(_)
                | Queried::Object(_)
                | Queried::Bytes(_)
                | Queried::Date(_) => Err(inapplicable()),
            },
            Predicate::StartsWith(expected) => match (value, expected.resolve(variables)?) {
                (Queried::String(subject), Queried::String(prefix)) => Ok(subject.starts_with(&prefix)),
                _ => Err(inapplicable()),
            },
            Predicate::EndsWith(expected) => match (value, expected.resolve(variables)?) {
                (Queried::String(subject), Queried::String(suffix)) => Ok(subject.ends_with(&suffix)),
                _ => Err(inapplicable()),
            },
            Predicate::Exists => Ok(true),
            Predicate::IsString => Ok(matches!(value, Queried::String(_))),
            Predicate::IsInteger => Ok(matches!(value, Queried::Integer(_))),
            Predicate::IsFloat => Ok(matches!(value, Queried::Float(_))),
            Predicate::IsNumber => Ok(matches!(value, Queried::Integer(_) | Queried::Float(_))),
            Predicate::IsBoolean => Ok(matches!(value, Queried::Bool(_))),
            Predicate::IsList => Ok(matches!(value, Queried::List(_))),
            Predicate::IsObject => Ok(matches!(value, Queried::Object(_))),
            Predicate::IsCollection => Ok(matches!(value, Queried::List(_) | Queried::Object(_) | Queried::Bytes(_))),
            Predicate::IsEmpty => match value {
                Queried::List(items) => Ok(items.is_empty()),
                Queried::Object(members) => Ok(members.is_empty()),
                Queried::String(text) => Ok(text.is_empty()),
                Queried::Bytes(bytes) => Ok(bytes.is_empty()),
                Queried::Nothing | Queried::Null | Queried::Bool(_) | Queried::Integer(_) | Queried::Float(_) | Queried::Date(_) => Err(inapplicable()),
            },
            Predicate::IsIsoDate => Ok(match value {
                Queried::String(text) => DateTime::parse_from_rfc3339(text).is_ok(),
                Queried::Date(_) => true,
                Queried::Nothing
                | Queried::Null
                | Queried::Bool(_)
                | Queried::Integer(_)
                | Queried::Float(_)
                | Queried::List(_)
                | Queried::Object(_)
                | Queried::Bytes(_) => false,
            }),
        }
    }
}

/// The order of two numbers, two strings or two instants; none for any other pair.
fn order(value: &Queried, expected: &Queried) -> Option<Ordering> {
    match (value, expected) {
        (Queried::Integer(left), Queried::Integer(right)) => Some(left.cmp(right)),
        (Queried::String(left), Queried::String(right)) => Some(left.cmp(right)),
        (Queried::Date(left), Queried::Date(right)) => Some(left.cmp(right)),
        (left, right) => left.as_float().zip(right.as_float()).and_then(|(left, right)| left.partial_cmp(&right)),
    }
}

impl Display for Expected {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Expected::Null => write!(formatter, "null"),
            Expected::Bool(flag) => write!(formatter, "{flag}"),
            Expected::Integer(integer) => write!(formatter, "{integer}"),
            Expected::Float(float) => write!(formatter, "{float:?}"),
            Expected::Text(template) => write!(formatter, "{template:?}"),
            Expected::Variable(name) => write!(formatter, "{{{{{name}}}}}"),
        }
    }
}

impl Display for Predicate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Predicate::Equal(expected) => write!(formatter, "== {expected}"),
            Predicate::NotEqual(expected) => write!(formatter, "!= {expected}"),
            Predicate::Greater(expected) => write!(formatter, "> {expected}"),
            Predicate::GreaterOrEqual(expected) => write!(formatter, ">= {expected}"),
            Predicate::Less(expected) => write!(formatter, "< {expected}"),
            Predicate::LessOrEqual(expected) => write!(formatter, "<= {expected}"),
            Predicate::Contains(expected) => write!(formatter, "contains {expected}"),
            Predicate::Includes(expected) => write!(formatter, "includes {expected}"),
            Predicate::Matches(template) => write!(formatter, "matches {template:?}"),
            Predicate::StartsWith(expected) => write!(formatter, "startsWith {expected}"),
            Predicate::EndsWith(expected) => write!(formatter, "endsWith {expected}"),
            Predicate::Exists => write!(formatter, "exists"),
            Predicate::IsString => write!(formatter, "isString"),
            Predicate::IsInteger => write!(formatter, "isInteger"),
            Predicate::IsFloat => write!(formatter, "isFloat"),
            Predicate::IsNumber => write!(formatter, "isNumber"),
            Predicate::IsBoolean => write!(formatter, "isBoolean"),
            Predicate::IsList => write!(formatter, "isList"),
            Predicate::IsObject => write!(formatter, "isObject"),
            Predicate::IsCollection => write!(formatter, "isCollection"),
            Predicate::IsEmpty => write!(formatter, "isEmpty"),
            Predicate::IsIsoDate => write!(formatter, "isIsoDate"),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sequence::{
        predicate::{Expected, Predicate},
        queried::Queried,
        variables::Variables,
    };
    use googletest::prelude::*;

    fn holds(predicate: &Predicate, value: &Queried) -> Result<bool, String> {
        predicate.holds(value, &Variables::default()).map_err(|error| error.to_string())
    }

    #[gtest]
    fn every_predicate_over_nothing_is_false() {
        let nothing = Queried::Nothing;
        expect_that!(holds(&Predicate::NotEqual(Expected::Integer(3)), &nothing), ok(eq(&false)));
        expect_that!(holds(&Predicate::Less(Expected::Integer(3)), &nothing), ok(eq(&false)));
        expect_that!(holds(&Predicate::IsEmpty, &nothing), ok(eq(&false)));
        expect_that!(holds(&Predicate::Exists, &nothing), ok(eq(&false)));
    }

    #[gtest]
    fn includes_does_not_apply_to_a_string() {
        expect_that!(
            holds(&Predicate::Includes(Expected::from("b")), &Queried::String("abc".to_owned())),
            err(anything())
        );
    }

    #[gtest]
    fn contains_reads_a_list_by_membership_and_a_string_by_substring() {
        let list = Queried::List(vec![Queried::String("a".to_owned())]);
        expect_that!(holds(&Predicate::Contains(Expected::from("a")), &list), ok(eq(&true)));
        expect_that!(
            holds(&Predicate::Contains(Expected::from("b")), &Queried::String("abc".to_owned())),
            ok(eq(&true))
        );
        expect_that!(holds(&Predicate::Contains(Expected::from("x")), &Queried::Null), err(anything()));
    }

    #[gtest]
    fn an_order_between_a_string_and_a_number_does_not_apply() {
        expect_that!(
            holds(&Predicate::GreaterOrEqual(Expected::Integer(3)), &Queried::String("abc".to_owned())),
            err(anything())
        );
        expect_that!(holds(&Predicate::Greater(Expected::Float(3.5)), &Queried::Integer(4)), ok(eq(&true)));
    }

    #[gtest]
    fn a_float_equals_the_integer_of_the_same_value() {
        expect_that!(holds(&Predicate::NotEqual(Expected::Integer(4)), &Queried::Float(4.0)), ok(eq(&false)));
        expect_that!(holds(&Predicate::Equal(Expected::from("4")), &Queried::Integer(4)), ok(eq(&false)));
    }

    #[gtest]
    fn null_exists_and_is_empty_does_not_apply_to_it() {
        expect_that!(holds(&Predicate::Exists, &Queried::Null), ok(eq(&true)));
        expect_that!(holds(&Predicate::IsEmpty, &Queried::Null), err(anything()));
        expect_that!(holds(&Predicate::Equal(Expected::Null), &Queried::Null), ok(eq(&true)));
    }
}
