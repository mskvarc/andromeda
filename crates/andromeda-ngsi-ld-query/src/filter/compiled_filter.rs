//! A `q` filter compiled for the executions that run it (clause 4.9).
//!
//! An execution reads a filter in three forms. The expanded expression is the
//! text a filter is stored, forwarded and written back as, and what the Linked
//! Entity retrieval reads. Its reading, with every clause 4.9 decision taken,
//! is held twice: as lowered, term for term as the client wrote the filter,
//! which the SQL renderings and a split query's filters read, so a statement
//! keeps the shape of the filter it answers, and in normal form, which the
//! in-memory execution runs. A compiled filter holds the three together,
//! lowered once where a query or a Subscription is compiled, so no matching
//! path lowers a filter and the forms cannot disagree.

use crate::filter::{expression::Expr, language_matching::LanguageMatching, reading::filter_reading::FilterReading};

/// An expanded `q` filter and its reading, lowered under one language
/// matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFilter {
    /// The expanded expression.
    expr: Expr,
    /// The reading of `expr` in normal form.
    reading: FilterReading,
    /// The reading of `expr` as lowered, term for term as written.
    lowered: FilterReading,
}

impl CompiledFilter {
    /// The expanded `expr`, its target languages selecting languageMap keys by
    /// `matching`.
    #[must_use]
    pub fn new(expr: Expr, matching: LanguageMatching) -> CompiledFilter {
        let lowered = FilterReading::lowered(&expr, matching);
        CompiledFilter {
            expr,
            // clone: the reading as written stays beside its normal form, which joins and reorders its terms.
            reading: lowered.clone().normal_form(),
            lowered,
        }
    }

    /// The expanded expression.
    #[must_use]
    pub const fn expr(&self) -> &Expr {
        &self.expr
    }

    /// The reading the in-memory execution runs, in normal form.
    #[must_use]
    pub const fn reading(&self) -> &FilterReading {
        &self.reading
    }

    /// The reading as lowered, term for term as written, which the SQL
    /// renderings and a split query's filters read.
    #[must_use]
    pub const fn lowered(&self) -> &FilterReading {
        &self.lowered
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{compiled_filter::CompiledFilter, language_matching::LanguageMatching, parser::parse, reading::filter_reading::FilterReading};

    /// The lowered reading keeps the filter as written while the normal form
    /// reorders and joins its terms.
    #[test]
    fn the_lowered_reading_keeps_the_filter_as_written() {
        let compiled = CompiledFilter::new(parse("d==1;(c==2|c==3|b~=\"x\");a").expect("the filter parses"), LanguageMatching::TagEquality);
        let attributes: Vec<&str> = compiled.lowered().terms().into_iter().map(|term| term.attribute().as_str()).collect();
        pretty_assertions::assert_eq!(attributes, ["d", "c", "c", "b", "a"]);
        pretty_assertions::assert_eq!(compiled.lowered(), &FilterReading::lowered(compiled.expr(), LanguageMatching::TagEquality));
        pretty_assertions::assert_eq!(compiled.reading().terms().len(), 4, "the normal form joins c==2 and c==3");
    }

    /// Both readings are the expression's, lowered under the matching given.
    #[test]
    fn the_readings_are_lowered_under_the_matching_given() {
        let expr = parse("name[en]==\"y\"|name[en]==\"x\"").expect("the filter parses");
        let compiled = CompiledFilter::new(expr.clone(), LanguageMatching::ExtendedFiltering);
        pretty_assertions::assert_eq!(compiled.reading(), &FilterReading::of(&expr, LanguageMatching::ExtendedFiltering));
        pretty_assertions::assert_eq!(
            compiled.reading(),
            &FilterReading::lowered(&parse("name[en]==\"x\",\"y\"").expect("the filter parses"), LanguageMatching::ExtendedFiltering)
        );
        pretty_assertions::assert_eq!(compiled.lowered(), &FilterReading::lowered(&expr, LanguageMatching::ExtendedFiltering));
        pretty_assertions::assert_eq!(compiled.expr(), &expr);
    }
}
