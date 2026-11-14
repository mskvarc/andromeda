//! A `q` filter compiled for the executions that run it (clause 4.9).
//!
//! An execution reads a filter in two forms. The expanded expression is the
//! text a filter is stored, forwarded and written back as, and what the SQL
//! rendering and the Linked Entity retrieval read. Its reading is what the
//! in-memory execution runs, with every clause 4.9 decision taken. A compiled
//! filter holds the two together, lowered once where a query or a Subscription
//! is compiled, so no matching path lowers a filter and the two forms cannot
//! disagree.

use crate::filter::{
    expression::Expr,
    language_matching::LanguageMatching,
    predicate::Predicate,
    reading::{filter_reading::FilterReading, term_reading::TermReading},
};

/// An expanded `q` filter and its reading, lowered under one language
/// matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFilter {
    /// The expanded expression.
    expr: Expr,
    /// How the filter's target languages select languageMap keys, the level
    /// the reading was lowered under.
    matching: LanguageMatching,
    /// The reading of `expr`, which mirrors its boolean structure term for
    /// term.
    reading: FilterReading,
}

impl CompiledFilter {
    /// The expanded `expr`, its target languages selecting languageMap keys by
    /// `matching`.
    #[must_use]
    pub fn new(expr: Expr, matching: LanguageMatching) -> CompiledFilter {
        let reading = FilterReading::of(&expr, matching);
        CompiledFilter { expr, matching, reading }
    }

    /// The expanded expression.
    #[must_use]
    pub const fn expr(&self) -> &Expr {
        &self.expr
    }

    /// How the filter's target languages select languageMap keys.
    #[must_use]
    pub const fn matching(&self) -> LanguageMatching {
        self.matching
    }

    /// The reading the in-memory execution runs.
    #[must_use]
    pub const fn reading(&self) -> &FilterReading {
        &self.reading
    }

    /// Every Query Term in pre-order, each beside its reading.
    ///
    /// Lowering maps each conjunction, disjunction and term of the expression
    /// to one of the reading, in order, so the two pre-orders pair up.
    pub fn terms(&self) -> impl Iterator<Item = (&Predicate, &TermReading)> {
        self.expr.predicates().into_iter().zip(self.reading.terms())
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading};

    /// The terms pair each predicate with the reading lowered from it, in the
    /// pre-order of the expression.
    #[test]
    fn every_term_is_paired_with_its_own_reading() {
        let compiled = CompiledFilter::new(parse("a==1;(b>2|c~=\"x\");d").expect("the filter parses"), LanguageMatching::TagEquality);
        let attributes: Vec<(&str, &str)> = compiled
            .terms()
            .map(|(predicate, reading)| (predicate.path().terminal().attr.as_str(), reading.attribute().as_str()))
            .collect();
        pretty_assertions::assert_eq!(attributes, [("a", "a"), ("b", "b"), ("c", "c"), ("d", "d")]);
    }

    /// The reading is the expression's, lowered under the matching given.
    #[test]
    fn the_reading_is_lowered_under_the_matching_given() {
        let expr = parse("name[en]==\"x\"").expect("the filter parses");
        let compiled = CompiledFilter::new(expr.clone(), LanguageMatching::ExtendedFiltering);
        pretty_assertions::assert_eq!(compiled.reading(), &FilterReading::of(&expr, LanguageMatching::ExtendedFiltering));
        pretty_assertions::assert_eq!(compiled.matching(), LanguageMatching::ExtendedFiltering);
        pretty_assertions::assert_eq!(compiled.expr(), &expr);
    }
}
