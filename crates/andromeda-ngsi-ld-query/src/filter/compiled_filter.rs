//! A `q` filter compiled for the executions that run it (clause 4.9).
//!
//! An execution reads a filter in two forms. The expanded expression is the
//! text a filter is stored, forwarded and written back as, and what the Linked
//! Entity retrieval reads. Its reading, with every clause 4.9 decision taken
//! and in normal form, is what the in-memory execution runs, what the SQL
//! renderings render and what a split query's filters are leaves of, so every
//! spelling of one filter runs, renders and splits alike. A compiled filter
//! holds the two together, lowered once where a query or a Subscription is
//! compiled, so no matching path lowers a filter and the forms cannot
//! disagree.

use crate::filter::{expression::Expr, language_matching::LanguageMatching, reading::filter_reading::FilterReading};

/// An expanded `q` filter and its reading, lowered under one language
/// matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFilter {
    /// The expanded expression.
    expr: Expr,
    /// The reading of `expr` in normal form.
    reading: FilterReading,
}

impl CompiledFilter {
    /// The expanded `expr`, its target languages selecting languageMap keys by
    /// `matching`.
    #[must_use]
    pub fn new(expr: Expr, matching: LanguageMatching) -> CompiledFilter {
        CompiledFilter {
            reading: FilterReading::of(&expr, matching),
            expr,
        }
    }

    /// The expanded expression.
    #[must_use]
    pub const fn expr(&self) -> &Expr {
        &self.expr
    }

    /// The reading every execution runs, in normal form.
    #[must_use]
    pub const fn reading(&self) -> &FilterReading {
        &self.reading
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading};

    /// The reading is the normal form, which reorders and joins the terms
    /// as written.
    #[test]
    fn the_reading_is_the_normal_form_of_the_filter_as_written() {
        let compiled = CompiledFilter::new(parse("d==1;(c==2|c==3|b~=\"x\");a").expect("the filter parses"), LanguageMatching::TagEquality);
        pretty_assertions::assert_eq!(compiled.reading(), &FilterReading::of(compiled.expr(), LanguageMatching::TagEquality));
        pretty_assertions::assert_eq!(compiled.reading().terms().len(), 4, "the normal form joins c==2 and c==3");
    }

    /// The reading is the expression's, lowered under the matching given.
    #[test]
    fn the_reading_is_lowered_under_the_matching_given() {
        let expr = parse("name[en]==\"y\"|name[en]==\"x\"").expect("the filter parses");
        let compiled = CompiledFilter::new(expr.clone(), LanguageMatching::ExtendedFiltering);
        pretty_assertions::assert_eq!(compiled.reading(), &FilterReading::of(&expr, LanguageMatching::ExtendedFiltering));
        pretty_assertions::assert_eq!(
            compiled.reading(),
            &FilterReading::lowered(&parse("name[en]==\"x\",\"y\"").expect("the filter parses"), LanguageMatching::ExtendedFiltering)
        );
        pretty_assertions::assert_eq!(compiled.expr(), &expr);
    }
}
