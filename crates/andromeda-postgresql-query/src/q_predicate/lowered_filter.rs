//! The reading of a `q` filter as lowered, which the tests of the renderings
//! compile.

use andromeda_ngsi_ld_query::filter::{
    Filter,
    language_matching::LanguageMatching,
    reading::{filter_reading::FilterReading, term_reading::TermReading},
};

/// The reading of `q`, term for term as written, its target languages
/// selecting keys by tag equality.
pub(crate) fn reading_of(q: &str) -> FilterReading {
    FilterReading::lowered(&q.parse::<Filter>().expect("the filter parses"), LanguageMatching::TagEquality)
}

/// The reading of `q`, one Query Term, its target languages selecting keys by
/// tag equality.
pub(crate) fn term_of(q: &str) -> TermReading {
    let Filter::Term(predicate) = q.parse::<Filter>().expect("the filter parses") else {
        panic!("a filter of one Query Term");
    };
    TermReading::of(&predicate, LanguageMatching::TagEquality)
}
