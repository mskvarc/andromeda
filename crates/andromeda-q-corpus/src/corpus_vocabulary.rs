//! How a corpus term expands, with no JSON-LD processor.

use andromeda_ngsi_ld_query::{
    error::QueryError,
    term_expander::{TermExpander, TermExpansionFailure},
};
use andromeda_ngsi_ld_vocabulary::compact_to_expanded;
use iri_rs::{Iri, IriBuf, iri};
use std::future::{Future, ready};

/// The vocabulary every corpus term that is no core term expands under.
const CORPUS_VOCABULARY: Iri<&str> = iri!("https://example.com/");

/// Expands corpus terms: a term the core `@context` maps to an IRI expands to
/// it, and every other term, an entity type included, to
/// `https://example.com/` followed by the term.
#[derive(Clone, Copy, Debug)]
pub struct CorpusVocabulary;

impl CorpusVocabulary {
    /// The IRI `term` expands to as an attribute name, `https://example.com/`
    /// followed by the term unless the core `@context` maps it.
    #[must_use]
    pub fn attribute_iri(term: &str) -> String {
        compact_to_expanded(term)
            .filter(|expanded| expanded.contains("://"))
            .map_or_else(|| CorpusVocabulary::vocabulary_iri(term), str::to_owned)
    }

    /// The IRI `term` expands to as an entity type or a vocabulary term:
    /// `https://example.com/` followed by the term.
    #[must_use]
    pub fn vocabulary_iri(term: &str) -> String {
        format!("{}{term}", CORPUS_VOCABULARY.as_str())
    }
}

/// `text` as an IRI.
fn vocabulary_iri(text: String) -> Result<IriBuf, QueryError> {
    IriBuf::new(text).map_err(|_| QueryError::TermExpansion(TermExpansionFailure::NotAnIri))
}

impl TermExpander for CorpusVocabulary {
    fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        ready(vocabulary_iri(CorpusVocabulary::attribute_iri(term)))
    }

    fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        ready(vocabulary_iri(CorpusVocabulary::vocabulary_iri(term)))
    }
}
