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

use crate::{
    error::QueryError,
    filter::{expression::Expr, predicate::Predicate, scalar::CompareValue},
};
use std::num::NonZeroUsize;

/// The deepest a `q` filter may nest its parentheses, and the highest nesting
/// limit a broker may be configured with.
pub const WRITTEN_NESTING_CEILING: NonZeroUsize = NonZeroUsize::MIN.saturating_add(63);

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
    /// Returns [`QueryError::QTooManyComparisons`] for a filter making more
    /// comparisons than `comparisons`, and [`QueryError::QNestedTooDeep`] for
    /// one nesting deeper than `nesting`; clause 5.5.6 answers both with
    /// `TooComplexQuery`.
    pub fn admit(self, expr: &Expr) -> Result<(), QueryError> {
        let comparisons = comparison_count(expr);
        if comparisons > self.comparisons.get() {
            return Err(QueryError::QTooManyComparisons {
                comparisons,
                limit: self.comparisons,
            });
        }
        let depth = nesting_depth(expr);
        if depth > self.nesting.get() {
            return Err(QueryError::QNestedTooDeep { depth, limit: self.nesting });
        }
        Ok(())
    }
}

/// The comparisons `expr` makes: one per Query Term, and one per value of a
/// value list.
#[must_use]
pub fn comparison_count(expr: &Expr) -> usize {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => terms.iter().map(comparison_count).sum(),
        Expr::Pred(predicate) => predicate_comparisons(predicate),
    }
}

/// The levels `expr`'s `;` and `|` combinations nest; a lone Query Term nests
/// none, however many parentheses enclose it.
#[must_use]
pub fn nesting_depth(expr: &Expr) -> usize {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => 1 + terms.iter().map(nesting_depth).max().unwrap_or(0),
        Expr::Pred(_) => 0,
    }
}

/// The comparisons one Query Term makes.
const fn predicate_comparisons(predicate: &Predicate) -> usize {
    match predicate {
        Predicate::Equality {
            rhs: CompareValue::ValueList(values),
            ..
        } => values.len(),
        Predicate::Exists(_)
        | Predicate::Equality {
            rhs: CompareValue::Scalar(_) | CompareValue::Range { .. },
            ..
        }
        | Predicate::Ordering { .. }
        | Predicate::MatchPattern { .. }
        | Predicate::NotMatchPattern { .. } => 1,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        filter::{
            complexity::{FilterComplexityLimits, comparison_count, nesting_depth},
            expression::Expr,
        },
    };
    use std::num::NonZeroUsize;

    fn filter(q: &str) -> Expr {
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
            matches!(refusal, QueryError::QTooManyComparisons { comparisons: 101, limit } if limit.get() == 100),
            "{refusal:?}"
        );
    }

    #[test]
    fn a_value_list_at_the_comparison_limit_is_admitted_and_one_value_more_is_too_complex() {
        let limits = limits(1000, 16);
        assert!(limits.admit(&filter(&value_list(1000))).is_ok());
        let refusal = limits.admit(&filter(&value_list(1001))).expect_err("one value too many");
        assert!(matches!(refusal, QueryError::QTooManyComparisons { comparisons: 1001, .. }), "{refusal:?}");
    }

    #[test]
    fn a_value_list_and_other_terms_are_counted_together() {
        let limits = limits(10, 16);
        assert!(limits.admit(&filter(&format!("{};{}", value_list(8), conjunction(2)))).is_ok());
        let refusal = limits
            .admit(&filter(&format!("{};{}", value_list(8), conjunction(3))))
            .expect_err("the list and the terms pass the limit together");
        assert!(matches!(refusal, QueryError::QTooManyComparisons { comparisons: 11, .. }), "{refusal:?}");
    }

    #[test]
    fn a_filter_at_the_nesting_limit_is_admitted_and_one_level_deeper_is_too_complex() {
        let limits = limits(1000, 16);
        assert!(limits.admit(&filter(&nested(16))).is_ok());
        let refusal = limits.admit(&filter(&nested(17))).expect_err("one level too deep");
        assert!(
            matches!(refusal, QueryError::QNestedTooDeep { depth: 17, limit } if limit.get() == 16),
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
