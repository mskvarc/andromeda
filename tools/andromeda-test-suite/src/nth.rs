//! A matcher on the item at one position of a list: the entities of a page in their order, the instances of an
//! attribute's evolution.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use std::fmt::Debug;

/// Matches a list whose item at `index` meets a matcher.
#[derive(Debug, Clone, MatcherBase)]
pub struct Nth<M> {
    /// The position, from 0.
    index: usize,
    /// The requirements on the item.
    matcher: M,
}

/// A list whose item at `index` (from 0) meets `matcher`.
#[must_use]
pub const fn nth<M>(index: usize, matcher: M) -> Nth<M> {
    Nth { index, matcher }
}

impl<'a, T: Debug, M: Matcher<&'a T>> Matcher<&'a [T]> for Nth<M> {
    fn matches(&self, actual: &'a [T]) -> MatcherResult {
        actual.get(self.index).map_or(MatcherResult::NoMatch, |item| self.matcher.matches(item))
    }

    fn describe(&self, result: MatcherResult) -> Description {
        format!("has at {} an item which {}", self.index, self.matcher.describe(result)).into()
    }

    fn explain_match(&self, actual: &'a [T]) -> Description {
        actual
            .get(self.index)
            .map_or_else(|| format!("which holds {} items", actual.len()).into(), |item| self.matcher.explain_match(item))
    }
}

impl<'a, T: Debug, M: Matcher<&'a T>> Matcher<&'a Vec<T>> for Nth<M> {
    fn matches(&self, actual: &'a Vec<T>) -> MatcherResult {
        Matcher::<&'a [T]>::matches(self, actual.as_slice())
    }

    fn describe(&self, result: MatcherResult) -> Description {
        Matcher::<&'a [T]>::describe(self, result)
    }

    fn explain_match(&self, actual: &'a Vec<T>) -> Description {
        Matcher::<&'a [T]>::explain_match(self, actual.as_slice())
    }
}
