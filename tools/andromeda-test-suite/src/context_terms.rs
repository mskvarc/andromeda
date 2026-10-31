//! Requirements on an `@context` read back from an answer: that it is written as a list, and that an inline entry
//! defines exactly the terms a test sent, whatever their order.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{Context, ContextEntry};
use serde_json::{Map, Value};

/// The entries of `context` when it is written as an array, `None` when it is written as a single entry (JSON-LD 1.1
/// section 9.15 reads both alike; a test about how the answer is written tells them apart).
#[must_use]
pub const fn listed_entries(context: &Context) -> Option<&[ContextEntry]> {
    match context {
        Context::Multiple(entries) => Some(entries.as_slice()),
        Context::Single(_) => None,
    }
}

/// An inline entry defining exactly some terms, each to the IRI given.
#[derive(Debug, Clone, MatcherBase)]
pub struct DefinesTerms {
    /// The terms and the IRIs they expand to.
    terms: Vec<(String, String)>,
}

/// An inline `@context` entry defining exactly `terms`, each a term and the IRI it expands to, in any order.
#[must_use]
pub fn defines_terms<'a>(terms: impl IntoIterator<Item = (&'a str, &'a str)>) -> DefinesTerms {
    DefinesTerms {
        terms: terms.into_iter().map(|(term, iri)| (term.to_owned(), iri.to_owned())).collect(),
    }
}

impl DefinesTerms {
    /// Whether the definition `text` holds exactly the terms.
    fn holds(&self, text: &str) -> bool {
        serde_json::from_str::<Map<String, Value>>(text).is_ok_and(|definition| {
            definition.len() == self.terms.len()
                && self
                    .terms
                    .iter()
                    .all(|(term, iri)| definition.get(term).and_then(Value::as_str) == Some(iri.as_str()))
        })
    }
}

impl Matcher<&ContextEntry> for DefinesTerms {
    fn matches(&self, actual: &ContextEntry) -> MatcherResult {
        match actual {
            ContextEntry::Inline(inline) => self.holds(&inline.to_string()).into(),
            ContextEntry::Reference(_) => MatcherResult::NoMatch,
        }
    }

    fn describe(&self, result: MatcherResult) -> Description {
        let terms = self.terms.iter().map(|(term, iri)| format!("{term} -> {iri}")).collect::<Vec<_>>().join(", ");
        match result {
            MatcherResult::Match => format!("is an inline context defining exactly {terms}").into(),
            MatcherResult::NoMatch => format!("isn't an inline context defining exactly {terms}").into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::context_terms::{defines_terms, listed_entries};
    use googletest::prelude::*;
    use iri_rs::IriRefBuf;
    use ngsi_ld::model::{Context, ContextEntry, InlineContext};

    /// The inline entry the text defines.
    fn inline(text: &str) -> Result<ContextEntry> {
        Ok(ContextEntry::Inline(text.parse::<InlineContext>()?))
    }

    #[gtest]
    fn an_inline_entry_defines_its_terms_in_any_order() -> Result<()> {
        let entry = inline(r#"{"a": "https://x.test/a", "b": "https://x.test/b"}"#)?;
        expect_that!(&entry, defines_terms([("b", "https://x.test/b"), ("a", "https://x.test/a")]));
        expect_that!(&entry, not(defines_terms([("a", "https://x.test/a")])));
        expect_that!(&entry, not(defines_terms([("a", "https://x.test/a"), ("b", "https://x.test/other")])));
        verify_that!(
            &entry,
            not(defines_terms([("a", "https://x.test/a"), ("b", "https://x.test/b"), ("c", "https://x.test/c")]))
        )
    }

    #[gtest]
    fn a_reference_defines_no_terms() -> Result<()> {
        let entry = ContextEntry::Reference(IriRefBuf::new("https://x.test/context.jsonld".to_owned())?);
        verify_that!(&entry, not(defines_terms([])))
    }

    #[gtest]
    fn only_an_array_context_has_listed_entries() -> Result<()> {
        let entry = inline(r#"{"a": "https://x.test/a"}"#)?;
        expect_that!(listed_entries(&Context::Single(entry.clone())), none());
        verify_that!(listed_entries(&Context::Multiple(vec![entry.clone()])), some(elements_are![eq(&entry)]))
    }
}
