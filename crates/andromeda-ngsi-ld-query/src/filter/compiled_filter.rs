//! A `q` filter compiled for the executions that run it (clause 4.9).
//!
//! An execution reads a filter in two forms. The expanded expression is the
//! text a filter is stored, forwarded and written back as, and what the SQL
//! rendering and the Linked Entity retrieval read. Its reading is what the
//! in-memory execution runs, with every clause 4.9 decision taken and in
//! normal form. A compiled filter holds the two together, lowered once where a
//! query or a Subscription is compiled, so no matching path lowers a filter
//! and the two forms cannot disagree.
//!
//! A split query's filters are judged term by term over each part of an
//! Entity (clause 5.7.4.4), where a store renders each term from its
//! expression and memory runs its reading. The normal form joins and orders
//! terms, so it pairs with no term of the expression, and the compiled filter
//! also holds each term of the expression lowered alone.

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
    /// The reading of `expr` in normal form.
    reading: FilterReading,
    /// Each Query Term of `expr` in pre-order, lowered alone.
    term_readings: Vec<TermReading>,
}

impl CompiledFilter {
    /// The expanded `expr`, its target languages selecting languageMap keys by
    /// `matching`.
    #[must_use]
    pub fn new(expr: Expr, matching: LanguageMatching) -> CompiledFilter {
        let lowered = FilterReading::lowered(&expr, matching);
        // clone: the terms as written stay beside the normal form, which joins and reorders them.
        let term_readings = lowered.terms().into_iter().cloned().collect();
        CompiledFilter {
            expr,
            matching,
            reading: lowered.normal_form(),
            term_readings,
        }
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

    /// The reading the in-memory execution runs, in normal form.
    #[must_use]
    pub const fn reading(&self) -> &FilterReading {
        &self.reading
    }

    /// Every Query Term of the expression in pre-order, each beside its
    /// reading lowered alone: the leaves a split query's filters are judged
    /// by.
    pub fn terms(&self) -> impl Iterator<Item = (&Predicate, &TermReading)> {
        self.expr.predicates().into_iter().zip(&self.term_readings)
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{
        compiled_filter::CompiledFilter,
        language_matching::LanguageMatching,
        parser::parse,
        reading::{filter_reading::FilterReading, term_reading::TermReading},
    };

    /// The terms pair each predicate with the reading lowered from it, in the
    /// pre-order of the expression, while the normal form reorders and joins
    /// them.
    #[test]
    fn every_term_is_paired_with_its_own_reading() {
        let compiled = CompiledFilter::new(parse("d==1;(c==2|c==3|b~=\"x\");a").expect("the filter parses"), LanguageMatching::TagEquality);
        let attributes: Vec<(&str, &str)> = compiled
            .terms()
            .map(|(predicate, reading)| (predicate.path().terminal().attr.as_str(), reading.attribute().as_str()))
            .collect();
        pretty_assertions::assert_eq!(attributes, [("d", "d"), ("c", "c"), ("c", "c"), ("b", "b"), ("a", "a")]);
        let paired: Vec<&TermReading> = compiled.terms().map(|(_, reading)| reading).collect();
        let lowered = FilterReading::lowered(compiled.expr(), LanguageMatching::TagEquality);
        pretty_assertions::assert_eq!(paired, lowered.terms());
        pretty_assertions::assert_eq!(compiled.reading().terms().len(), 4, "the normal form joins c==2 and c==3");
    }

    /// The reading is the expression's normal form, lowered under the matching
    /// given.
    #[test]
    fn the_reading_is_lowered_under_the_matching_given() {
        let expr = parse("name[en]==\"y\"|name[en]==\"x\"").expect("the filter parses");
        let compiled = CompiledFilter::new(expr.clone(), LanguageMatching::ExtendedFiltering);
        pretty_assertions::assert_eq!(compiled.reading(), &FilterReading::of(&expr, LanguageMatching::ExtendedFiltering));
        pretty_assertions::assert_eq!(
            compiled.reading(),
            &FilterReading::lowered(&parse("name[en]==\"x\",\"y\"").expect("the filter parses"), LanguageMatching::ExtendedFiltering)
        );
        pretty_assertions::assert_eq!(compiled.matching(), LanguageMatching::ExtendedFiltering);
        pretty_assertions::assert_eq!(compiled.expr(), &expr);
    }
}
