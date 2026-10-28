//! What a test requires of one answer: its status, its checks, and the values it captures for later requests.
//!
//! Captures are taken first, then every check runs, so one failing answer reports every unmet requirement at once.

use crate::sequence::{
    answer::Answer,
    predicate::{Expected, Predicate},
    queried::Queried,
    query::Query,
    variables::Variables,
};
use http::Version;

/// One requirement: a query, a predicate, and whether the predicate is negated.
#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    /// What is read.
    query: Query,
    /// Whether the predicate must not hold.
    negated: Negation,
    /// What must hold.
    predicate: Predicate,
}

/// Whether a check requires its predicate to hold or not to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Negation {
    /// The predicate must hold.
    Holds,
    /// The predicate must not hold.
    DoesNotHold,
}

/// A query whose next predicate must not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Not(Query);

/// Defines the predicate methods of [`Query`] and [`Not`].
macro_rules! predicates {
    ($($(#[$doc:meta])* $name:ident($($argument:ident: $kind:ty),*) => $predicate:expr;)*) => {
        #[expect(clippy::missing_const_for_fn, reason = "the predicate methods share one generated shape; those without argument could be const")]
        impl Query {
            $($(#[$doc])* #[must_use] pub fn $name(self, $($argument: $kind),*) -> Check {
                Check { query: self, negated: Negation::Holds, predicate: $predicate }
            })*

            /// The query, with the next predicate negated.
            #[must_use]
            pub const fn not(self) -> Not {
                Not(self)
            }
        }

        impl Not {
            $($(#[$doc])* #[must_use] pub fn $name(self, $($argument: $kind),*) -> Check {
                Check { query: self.0, negated: Negation::DoesNotHold, predicate: $predicate }
            })*
        }
    };
}

predicates! {
    /// The value equals `expected`.
    eq(expected: impl Into<Expected>) => Predicate::Equal(expected.into());
    /// The value differs from `expected`.
    ne(expected: impl Into<Expected>) => Predicate::NotEqual(expected.into());
    /// The value is greater than `expected`.
    gt(expected: impl Into<Expected>) => Predicate::Greater(expected.into());
    /// The value is at least `expected`.
    ge(expected: impl Into<Expected>) => Predicate::GreaterOrEqual(expected.into());
    /// The value is less than `expected`.
    lt(expected: impl Into<Expected>) => Predicate::Less(expected.into());
    /// The value is at most `expected`.
    le(expected: impl Into<Expected>) => Predicate::LessOrEqual(expected.into());
    /// The string holds `expected`, or the list an item equal to it.
    contains(expected: impl Into<Expected>) => Predicate::Contains(expected.into());
    /// The list holds an item equal to `expected`.
    includes(expected: impl Into<Expected>) => Predicate::Includes(expected.into());
    /// The string matches the regular expression `pattern`.
    matches(pattern: &str) => Predicate::Matches(pattern.to_owned());
    /// The string starts with `prefix`.
    starts_with(prefix: impl Into<Expected>) => Predicate::StartsWith(prefix.into());
    /// The string ends with `suffix`.
    ends_with(suffix: impl Into<Expected>) => Predicate::EndsWith(suffix.into());
    /// There is a value.
    exists() => Predicate::Exists;
    /// The value is a string.
    is_string() => Predicate::IsString;
    /// The value is an integer.
    is_integer() => Predicate::IsInteger;
    /// The value is a float.
    is_float() => Predicate::IsFloat;
    /// The value is a number.
    is_number() => Predicate::IsNumber;
    /// The value is a boolean.
    is_boolean() => Predicate::IsBoolean;
    /// The value is a list.
    is_list() => Predicate::IsList;
    /// The value is an object.
    is_object() => Predicate::IsObject;
    /// The value is a list, an object or bytes.
    is_collection() => Predicate::IsCollection;
    /// The value is empty.
    is_empty() => Predicate::IsEmpty;
    /// The value is an RFC 3339 date and time.
    is_iso_date() => Predicate::IsIsoDate;
}

/// Every requirement on one answer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Expect {
    /// The status the answer must have; none accepts any.
    status: Option<u16>,
    /// The HTTP version the answer must come in; none accepts any.
    version: Option<Version>,
    /// The checks, in order.
    checks: Vec<Check>,
    /// The values taken for later requests, by name.
    captures: Vec<(String, Query)>,
}

/// An answer with status `code`.
#[must_use]
pub fn status(code: u16) -> Expect {
    Expect {
        status: Some(code),
        ..Expect::default()
    }
}

/// An answer with any status.
#[must_use]
pub fn any_status() -> Expect {
    Expect::default()
}

/// What one answer met and what it captured.
#[derive(Debug, Default)]
pub struct Verdict {
    /// Each requirement the answer did not meet.
    pub failures: Vec<String>,
    /// The captured values, by name.
    pub captured: Vec<(String, Queried)>,
}

impl Expect {
    /// The requirements, with the answer coming in HTTP `version`.
    #[must_use]
    pub const fn version(mut self, version: Version) -> Expect {
        self.version = Some(version);
        self
    }

    /// The requirements, with `check` added.
    #[must_use]
    pub fn that(mut self, check: Check) -> Expect {
        self.checks.push(check);
        self
    }

    /// The requirements, with `query` captured as `name` for later requests.
    #[must_use]
    pub fn capture(mut self, name: &str, query: Query) -> Expect {
        self.captures.push((name.to_owned(), query));
        self
    }

    /// What `answer` meets of the requirements. A capture is placed in `variables` as soon as it is taken, so a
    /// later capture or check of the same answer reads it.
    #[must_use]
    pub fn verdict(&self, answer: &Answer, variables: &mut Variables) -> Verdict {
        let mut verdict = Verdict::default();
        for (name, query) in &self.captures {
            match query.evaluate(answer, variables) {
                Ok(Queried::Nothing) => verdict.failures.push(format!("capture {name}: {query} yields nothing")),
                Ok(value) => {
                    variables.set(name, value.clone());
                    verdict.captured.push((name.clone(), value));
                }
                Err(error) => verdict.failures.push(format!("capture {name}: {query}: {error}")),
            }
        }
        if let Some(code) = self.status
            && answer.status != code
        {
            verdict.failures.push(format!("status: expected {code}, got {}", answer.status));
        }
        if let Some(version) = self.version
            && answer.version != version
        {
            verdict.failures.push(format!("version: expected {version:?}, got {:?}", answer.version));
        }
        verdict.failures.extend(self.checks.iter().filter_map(|check| check.failure(answer, variables)));
        verdict
    }
}

impl Check {
    /// Why `answer` fails the check, if it does.
    fn failure(&self, answer: &Answer, variables: &Variables) -> Option<String> {
        let negation = match self.negated {
            Negation::Holds => "",
            Negation::DoesNotHold => "not ",
        };
        let label = format!("{} {negation}{}", self.query, self.predicate);
        let value = match self.query.evaluate(answer, variables) {
            Ok(value) => value,
            Err(error) => return Some(format!("{label}: {error}")),
        };
        match (self.predicate.holds(&value, variables), self.negated) {
            (Ok(true), Negation::Holds) | (Ok(false), Negation::DoesNotHold) => None,
            (Ok(_), _) => Some(format!("{label}: actual {value}")),
            (Err(error), _) => Some(format!("{label}: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sequence::{
        answer::Answer,
        expect::status,
        query::{header, json},
        variables::Variables,
    };
    use googletest::prelude::*;

    #[gtest]
    fn every_unmet_requirement_is_reported() {
        let answer = Answer::json(200, r#"{"a": 1}"#.to_owned(), "http://broker/".to_owned());
        let verdict = status(201)
            .that(json("$.a").eq(2))
            .that(json("$.a").eq(1))
            .that(header("Location").exists())
            .verdict(&answer, &mut Variables::default());
        expect_that!(verdict.failures.len(), eq(3));
    }

    #[gtest]
    fn a_negated_check_over_nothing_holds() {
        let answer = Answer::json(200, "{}".to_owned(), "http://broker/".to_owned());
        let verdict = status(200).that(json("$.missing").not().exists()).verdict(&answer, &mut Variables::default());
        expect_that!(verdict.failures, len(eq(0)));
    }

    #[gtest]
    fn a_capture_is_readable_by_a_later_check() {
        let answer = Answer::json(200, r#"{"id": "x", "same": "x"}"#.to_owned(), "http://broker/".to_owned());
        let mut variables = Variables::default();
        let verdict = status(200)
            .capture("id", json("$.id"))
            .that(json("$.same").eq("{{id}}"))
            .verdict(&answer, &mut variables);
        expect_that!(verdict.failures, len(eq(0)));
        expect_that!(variables.get("id"), some(anything()));
    }
}
