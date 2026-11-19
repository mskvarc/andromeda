//! A `q` filter compiled for the executions that run it (clause 4.9).
//!
//! An execution reads a filter in two forms. The expanded filter is the
//! text a filter is stored, forwarded and written back as, and what the Linked
//! Entity retrieval reads. Its reading, with every clause 4.9 decision taken
//! and in normal form, is what the in-memory execution runs, what the SQL
//! renderings render and what a split query's filters are leaves of, so every
//! spelling of one filter runs, renders and splits alike. A compiled filter
//! holds the two together, lowered once where a query or a Subscription is
//! compiled, so no matching path lowers a filter and the forms cannot
//! disagree.

use crate::{
    filter::{Filter, language_matching::LanguageMatching, reading::filter_reading::FilterReading},
    name_stage::Expanded,
};

/// An expanded `q` filter and its reading, lowered under one language
/// matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFilter {
    /// The expanded filter.
    filter: Filter<Expanded>,
    /// The reading of `filter` in normal form.
    reading: FilterReading,
}

impl CompiledFilter {
    /// The expanded `filter`, its target languages selecting languageMap keys by
    /// `matching`.
    #[must_use]
    pub fn new(filter: Filter<Expanded>, matching: LanguageMatching) -> CompiledFilter {
        CompiledFilter {
            reading: FilterReading::of(&filter, matching),
            filter,
        }
    }

    /// The expanded filter.
    #[must_use]
    pub const fn filter(&self) -> &Filter<Expanded> {
        &self.filter
    }

    /// The reading every execution runs, in normal form.
    #[must_use]
    pub const fn reading(&self) -> &FilterReading {
        &self.reading
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching, reading::filter_reading::FilterReading},
        name_stage::{Expanded, Written},
        namespace_expander::EXAMPLE_NAMESPACE,
    };

    /// `q` expanded under the example namespace.
    fn expanded(q: &str) -> Filter<Expanded> {
        EXAMPLE_NAMESPACE
            .expanded(q.parse::<Filter<Written>>().expect("a clause 4.9 filter"))
            .expect("the names expand")
    }

    /// The reading is the normal form, which reorders and joins the terms
    /// as written.
    #[test]
    fn the_reading_is_the_normal_form_of_the_filter_as_written() {
        let compiled = CompiledFilter::new(expanded("d==1;(c==2|c==3|b~=\"x\");a"), LanguageMatching::TagEquality);
        pretty_assertions::assert_eq!(compiled.reading(), &FilterReading::of(compiled.filter(), LanguageMatching::TagEquality));
        pretty_assertions::assert_eq!(compiled.reading().terms().len(), 4, "the normal form joins c==2 and c==3");
    }

    /// The reading is the filter's, lowered under the matching given.
    #[test]
    fn the_reading_is_lowered_under_the_matching_given() {
        let filter = expanded("name[en]==\"y\"|name[en]==\"x\"");
        let compiled = CompiledFilter::new(filter.clone(), LanguageMatching::ExtendedFiltering);
        pretty_assertions::assert_eq!(compiled.reading(), &FilterReading::of(&filter, LanguageMatching::ExtendedFiltering));
        pretty_assertions::assert_eq!(
            compiled.reading(),
            &FilterReading::lowered(&expanded("name[en]==\"x\",\"y\""), LanguageMatching::ExtendedFiltering)
        );
        pretty_assertions::assert_eq!(compiled.filter(), &filter);
    }
}
