//! How a corpus term expands, with no JSON-LD processor.

use andromeda_ngsi_ld_query::{
    namespace_expander::NamespaceExpander,
    term_expander::{TermExpander, TermExpansionError},
};
use andromeda_ngsi_ld_vocabulary::compact_to_expanded;
use iri_rs::{Iri, IriBuf, iri};
use std::future::{Future, ready};

/// The vocabulary every corpus term that is no core term expands under.
const CORPUS_VOCABULARY: Iri<&str> = iri!("https://example.com/");

/// Resolves the terms the core `@context` does not map and every entity type.
const CORPUS_NAMESPACE: NamespaceExpander = NamespaceExpander::uniform(CORPUS_VOCABULARY);

/// Expands corpus terms: a term the core `@context` maps to an IRI expands to
/// it, and every other term, an entity type included, to
/// `https://example.com/` followed by the term.
///
/// It differs from the shared namespace expander in mapping through the whole
/// core `@context` and in giving the same IRIs as keys for fixture documents.
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

impl TermExpander for CorpusVocabulary {
    fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
        let core = compact_to_expanded(term).filter(|expanded| expanded.contains("://"));
        ready(match core {
            Some(expanded) => IriBuf::new(expanded.to_owned()).map_err(|_| TermExpansionError::NotAnIri),
            None => CORPUS_NAMESPACE.term_iri(term),
        })
    }

    fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
        ready(CORPUS_NAMESPACE.type_iri(term))
    }
}
