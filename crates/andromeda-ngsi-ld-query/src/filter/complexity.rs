//! How much work a `q` filter asks of the broker, and the limits it is held to.
//!
//! Clause 5.5.6 lets a broker refuse a query "so complex that cannot be
//! resolved" with `TooComplexQuery`. A filter costs in two ways. Every
//! comparison it makes is its own test of the stored values, and a value list
//! makes one per value, because each value is compared on its own. Every level
//! its `;` and `|` combinations nest adds a level to the condition the store
//! evaluates and to every walk over the filter.
//!
//! Parentheses are bounded once more while the filter is parsed, before any
//! configured limit is known: [`WRITTEN_NESTING_CEILING`] keeps the parser's
//! recursion inside a fixed depth however a filter reaches it, a request, a
//! stored subscription or a forwarded query.

use crate::filter::{Filter, equality_value::EqualityValue, filter_error::FilterError, query_term::QueryTerm};
use andromeda_ngsi_ld_representation::attribute_nesting::AttributeNestingLimit;
use std::num::NonZeroUsize;

/// The deepest a `q` filter may nest its parentheses, and the highest nesting
/// limit a broker may be configured with.
pub const WRITTEN_NESTING_CEILING: NonZeroUsize = NonZeroUsize::MIN.saturating_add(63);

/// The most segments an attribute path may nest, counting sub-attributes and
/// linked-entity hops alike.
///
/// Held to the depth a written attribute is admitted to,
/// [`AttributeNestingLimit::DEFAULT`], so every sub-attribute a write can
/// store stays addressable by `q` and `orderBy`. The linked-entity hops that
/// share this budget stay bounded by it: a hop compiles to a correlated
/// subquery per level, so `a{a{a{…}}}` is as expensive as it is deep. An
/// `orderBy` path draws on the same budget, for the same reason: each segment
/// is another hop inside the jsonpath its lateral evaluates.
pub const MAX_SUB_ATTRIBUTE_DEPTH: usize = AttributeNestingLimit::DEFAULT.get();

/// The most a `q` filter a request writes may ask of the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterComplexityLimits {
    /// The most comparisons one filter may make, a value list making one per
    /// value.
    pub comparisons: NonZeroUsize,
    /// The most levels its `;` and `|` combinations may nest: `a;b` nests one
    /// level, `a;(b|c)` and `a|b;c` two.
    pub nesting: NonZeroUsize,
}

impl FilterComplexityLimits {
    /// No limit past the parser's own: what a filter admitted when it was
    /// written, such as a stored subscription's, is read under, so a limit
    /// lowered since refuses none of them.
    pub const PARSED: FilterComplexityLimits = FilterComplexityLimits {
        comparisons: NonZeroUsize::MAX,
        nesting: NonZeroUsize::MAX,
    };

    /// The limits of `comparisons` and `nesting`.
    ///
    /// Written out because `derive_more::Constructor` spells its fields
    /// redundantly, which clippy refuses.
    #[must_use]
    pub const fn new(comparisons: NonZeroUsize, nesting: NonZeroUsize) -> FilterComplexityLimits {
        FilterComplexityLimits { comparisons, nesting }
    }

    /// Admit a filter a request carries under these limits.
    ///
    /// # Errors
    ///
    /// Returns [`FilterError::TooManyComparisons`] for a filter making more
    /// comparisons than `comparisons`, and [`FilterError::NestedTooDeep`] for
    /// one nesting deeper than `nesting`; clause 5.5.6 answers both with
    /// `TooComplexQuery`.
    pub fn admit(self, filter: &Filter) -> Result<(), FilterError> {
        let comparisons = comparison_count(filter);
        if comparisons > self.comparisons.get() {
            return Err(FilterError::TooManyComparisons {
                comparisons,
                limit: self.comparisons,
            });
        }
        let depth = nesting_depth(filter);
        if depth > self.nesting.get() {
            return Err(FilterError::NestedTooDeep { depth, limit: self.nesting });
        }
        Ok(())
    }
}

/// The comparisons `filter` makes: one per Query Term, and one per value of a
/// value list.
#[must_use]
pub fn comparison_count(filter: &Filter) -> usize {
    match filter {
        Filter::And(filters) | Filter::Or(filters) => filters.iter().map(comparison_count).sum(),
        Filter::Term(term) => term_comparisons(term),
    }
}

/// The levels `filter`'s `;` and `|` combinations nest; a lone Query Term nests
/// none, however many parentheses enclose it.
#[must_use]
pub fn nesting_depth(filter: &Filter) -> usize {
    match filter {
        Filter::And(filters) | Filter::Or(filters) => 1 + filters.iter().map(nesting_depth).max().unwrap_or(0),
        Filter::Term(_) => 0,
    }
}

/// The comparisons one Query Term makes.
const fn term_comparisons(term: &QueryTerm) -> usize {
    match term {
        QueryTerm::Equality {
            value: EqualityValue::ValueList(values),
            ..
        } => values.len(),
        QueryTerm::Exists(_)
        | QueryTerm::Equality {
            value: EqualityValue::Scalar(_) | EqualityValue::Range { .. },
            ..
        }
        | QueryTerm::Ordering { .. }
        | QueryTerm::MatchPattern { .. }
        | QueryTerm::NotMatchPattern { .. } => 1,
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        Filter,
        complexity::{FilterComplexityLimits, comparison_count, nesting_depth},
        filter_error::FilterError,
    };
    use std::num::NonZeroUsize;

    fn filter(q: &str) -> Filter {
        q.parse().expect("a valid filter")
    }

    fn limits(comparisons: usize, nesting: usize) -> FilterComplexityLimits {
        FilterComplexityLimits::new(
            NonZeroUsize::new(comparisons).expect("a positive comparison limit"),
            NonZeroUsize::new(nesting).expect("a positive nesting limit"),
        )
    }

    /// `n` Query Terms joined by `;`.
    fn conjunction(terms: usize) -> String {
        (0..terms).map(|index| format!("a{index}>1")).collect::<Vec<_>>().join(";")
    }

    /// A Query Term comparing with a list of `values` numbers.
    fn value_list(values: usize) -> String {
        format!("a=={}", (0..values).map(|value| value.to_string()).collect::<Vec<_>>().join(","))
    }

    /// `levels` combinations, each nested inside the one before it.
    fn nested(levels: usize) -> String {
        (1..levels).fold("a;b".to_owned(), |inner, level| format!("c{level}|({inner})"))
    }

    #[test]
    fn every_query_term_makes_one_comparison_and_a_value_list_one_per_value() {
        let cases = [
            ("speed", 1),
            ("speed>50", 1),
            ("speed==10..20", 1),
            ("name~=\"A.*\"", 1),
            ("name!~=\"A.*\"", 1),
            ("color==\"red\",\"blue\",\"green\"", 3),
            ("color!=\"red\",\"blue\"", 2),
            ("speed>50;color==\"red\",\"blue\"|(rpm<10;isParkedIn{category}==\"x\")", 5),
        ];
        for (q, expected) in cases {
            pretty_assertions::assert_eq!(comparison_count(&filter(q)), expected, "{q}");
        }
    }

    #[test]
    fn each_level_of_combination_nests_once_and_parentheses_alone_add_none() {
        let cases = [
            ("speed>50", 0),
            ("((((speed>50))))", 0),
            ("a;b", 1),
            ("a|b|c", 1),
            ("a;(b|c)", 2),
            ("a|b;c", 2),
            ("a;(b|(c;d))", 3),
        ];
        for (q, expected) in cases {
            pretty_assertions::assert_eq!(nesting_depth(&filter(q)), expected, "{q}");
        }
        pretty_assertions::assert_eq!(nesting_depth(&filter(&nested(16))), 16);
    }

    #[test]
    fn a_filter_at_the_comparison_limit_is_admitted_and_one_past_it_is_too_complex() {
        let limits = limits(100, 16);
        assert!(limits.admit(&filter(&conjunction(100))).is_ok());
        let refusal = limits.admit(&filter(&conjunction(101))).expect_err("one comparison too many");
        assert!(
            matches!(refusal, FilterError::TooManyComparisons { comparisons: 101, limit } if limit.get() == 100),
            "{refusal:?}"
        );
    }

    #[test]
    fn a_value_list_at_the_comparison_limit_is_admitted_and_one_value_more_is_too_complex() {
        let limits = limits(1000, 16);
        assert!(limits.admit(&filter(&value_list(1000))).is_ok());
        let refusal = limits.admit(&filter(&value_list(1001))).expect_err("one value too many");
        assert!(matches!(refusal, FilterError::TooManyComparisons { comparisons: 1001, .. }), "{refusal:?}");
    }

    #[test]
    fn a_value_list_and_other_terms_are_counted_together() {
        let limits = limits(10, 16);
        assert!(limits.admit(&filter(&format!("{};{}", value_list(8), conjunction(2)))).is_ok());
        let refusal = limits
            .admit(&filter(&format!("{};{}", value_list(8), conjunction(3))))
            .expect_err("the list and the terms pass the limit together");
        assert!(matches!(refusal, FilterError::TooManyComparisons { comparisons: 11, .. }), "{refusal:?}");
    }

    #[test]
    fn a_filter_at_the_nesting_limit_is_admitted_and_one_level_deeper_is_too_complex() {
        let limits = limits(1000, 16);
        assert!(limits.admit(&filter(&nested(16))).is_ok());
        let refusal = limits.admit(&filter(&nested(17))).expect_err("one level too deep");
        assert!(
            matches!(refusal, FilterError::NestedTooDeep { depth: 17, limit } if limit.get() == 16),
            "{refusal:?}"
        );
    }

    #[test]
    fn a_refusal_names_what_was_counted_and_the_limit() {
        let comparisons = limits(2, 16).admit(&filter(&value_list(3))).expect_err("too many comparisons");
        pretty_assertions::assert_eq!(
            comparisons.to_string(),
            "q expression: 3 comparisons, more than the 2 this broker resolves in one query; a value list counts one comparison per value"
        );
        let nesting = limits(1000, 1).admit(&filter("a;(b|c)")).expect_err("nested too deep");
        pretty_assertions::assert_eq!(
            nesting.to_string(),
            "q expression: nested 2 levels deep, more than the 1 this broker resolves in one query"
        );
    }
}
