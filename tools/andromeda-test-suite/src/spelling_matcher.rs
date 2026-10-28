//! Matchers on a member the specification lets a broker write as one value or as an array of values (`OneOrMany`),
//! such as the `scope` of an entity (clause 4.18): one value written alone, a value at a position of the array, a
//! value held either way, and the length of the array. A value written alone and an array holding only it are
//! different members.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::OneOrMany;
use std::fmt::Debug;

/// What the values of the member must be.
#[derive(Debug, Clone)]
enum Requirement<T> {
    /// This value written alone.
    Alone(T),
    /// This value, written alone or in the array.
    Holding(T),
    /// This value in the array.
    Listed(T),
    /// This value at this position of the array.
    At(usize, T),
    /// An array of this many values.
    Counted(usize),
}

/// Matches a member written as one value or as an array of values.
#[derive(Debug, Clone, MatcherBase)]
pub struct SpellingMatcher<T> {
    /// The requirement on the values.
    requirement: Requirement<T>,
}

/// A member that is `value` written alone.
#[must_use]
pub fn alone<T: Clone>(value: &T) -> SpellingMatcher<T> {
    SpellingMatcher {
        requirement: Requirement::Alone(value.clone()),
    }
}

/// A member holding `value`, written alone or in the array.
#[must_use]
pub fn holding<T: Clone>(value: &T) -> SpellingMatcher<T> {
    SpellingMatcher {
        requirement: Requirement::Holding(value.clone()),
    }
}

/// A member written as an array holding `value`.
#[must_use]
pub fn listing<T: Clone>(value: &T) -> SpellingMatcher<T> {
    SpellingMatcher {
        requirement: Requirement::Listed(value.clone()),
    }
}

/// A member written as an array holding `value` at `index`.
#[must_use]
pub fn listing_at<T: Clone>(index: usize, value: &T) -> SpellingMatcher<T> {
    SpellingMatcher {
        requirement: Requirement::At(index, value.clone()),
    }
}

/// A member written as an array of `count` values.
#[must_use]
pub const fn listing_count<T>(count: usize) -> SpellingMatcher<T> {
    SpellingMatcher {
        requirement: Requirement::Counted(count),
    }
}

impl<T: PartialEq + Debug> Matcher<&OneOrMany<T>> for SpellingMatcher<T> {
    fn matches(&self, actual: &OneOrMany<T>) -> MatcherResult {
        match &self.requirement {
            Requirement::Alone(expected) => actual.as_one() == Some(expected),
            Requirement::Holding(expected) => actual.as_slice().contains(expected),
            Requirement::Listed(expected) => actual.is_many() && actual.as_slice().contains(expected),
            Requirement::At(index, expected) => actual.is_many() && actual.as_slice().get(*index) == Some(expected),
            Requirement::Counted(count) => actual.is_many() && actual.as_slice().len() == *count,
        }
        .into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        let requirement = match &self.requirement {
            Requirement::Alone(value) => format!("{value:?} written alone"),
            Requirement::Holding(value) => format!("a member holding {value:?}"),
            Requirement::Listed(value) => format!("an array holding {value:?}"),
            Requirement::At(index, value) => format!("an array holding {value:?} at {index}"),
            Requirement::Counted(count) => format!("an array of {count} values"),
        };
        match result {
            MatcherResult::Match => format!("is {requirement}").into(),
            MatcherResult::NoMatch => format!("isn't {requirement}").into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::spelling_matcher::{alone, holding, listing, listing_at, listing_count};
    use googletest::prelude::*;
    use ngsi_ld::model::{OneOrMany, Scope};

    #[gtest]
    fn a_value_written_alone_is_not_an_array_of_it() -> Result<()> {
        let scope: Scope = "/a/b".parse()?;
        let written_alone = OneOrMany::one(scope.clone());
        let listed = OneOrMany::try_from(vec![scope.clone()])?;
        expect_that!(&written_alone, alone(&scope));
        expect_that!(&written_alone, holding(&scope));
        expect_that!(&written_alone, not(listing_count(1)));
        expect_that!(&written_alone, not(listing(&scope)));
        expect_that!(&listed, listing(&scope));
        expect_that!(&listed, not(alone(&scope)));
        expect_that!(&listed, holding(&scope));
        verify_that!(&listed, listing_count(1))
    }

    #[gtest]
    fn an_array_is_read_by_position() -> Result<()> {
        let first: Scope = "/a".parse()?;
        let second: Scope = "/b".parse()?;
        let listed = OneOrMany::try_from(vec![first.clone(), second.clone()])?;
        expect_that!(&listed, listing_at(1, &second));
        expect_that!(&listed, not(listing_at(0, &second)));
        verify_that!(&OneOrMany::one(first.clone()), not(listing_at(0, &first)))
    }
}
