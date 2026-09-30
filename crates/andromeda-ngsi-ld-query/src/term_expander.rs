//! Expanding compact NGSI-LD terms to IRIs, without naming the expander.
//!
//! Every query parameter that carries a name — `attrs`, `q`, `type`, `pick`,
//! `omit`, `geoproperty`, `orderBy` — arrives compact and has to be expanded
//! through the request's `@context` before it can address anything the broker
//! stores. Doing that requires a JSON-LD processor, which sits above this
//! module: `jsonld` reads a parsed `q` filter, and `query` would read the
//! engine, which is a cycle.
//!
//! This trait breaks it. `query` states what it needs from an expander; the
//! JSON-LD layer supplies an implementation bound to one context, and nothing
//! in this module names it.

use crate::error::QueryError;
use iri_rs::IriBuf;
use std::future::Future;
use thiserror::Error;

/// Expands compact NGSI-LD terms against an already-selected `@context`.
///
/// Implementations carry the context, so nothing here has to thread one
/// through. Attribute terms and entity-type terms are separate methods because
/// JSON-LD expansion of a type differs from that of a property term (a type is
/// expanded as a vocabulary-relative IRI).
///
/// Both methods yield an [`IriBuf`]: NGSI-LD mandates a core `@context` whose
/// `@vocab` resolves every unmapped term, so expansion of any term the broker
/// can address produces an absolute IRI. One that does not is a broken context,
/// which is why the failure is typed rather than papered over with the term's
/// own text.
pub trait TermExpander: Sync {
    /// Expand an attribute term to its full IRI.
    fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send;

    /// Expand an entity-type term to its full IRI.
    fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send;
}

/// Why an expander could not turn a term into an IRI.
///
/// The expander's own failure type lives above this crate, so an
/// implementation classifies its failure into one of these and records the
/// detail where it still holds it.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum TermExpansionFailure {
    /// The term expands to nothing against the active `@context`.
    #[error("the term expands to nothing against the active @context")]
    NotExpanded,
    /// The term expanded to text that is not an absolute IRI.
    #[error("the term expands to something that is not an absolute IRI")]
    NotAnIri,
    /// A `@context` the term is expanded against could not be retrieved.
    #[error("a @context the term is expanded against is not available")]
    ContextUnavailable,
    /// A `@context` the term is expanded against could not be processed.
    #[error("a @context the term is expanded against could not be processed")]
    ContextUnprocessable,
}

#[cfg(test)]
mod tests {
    use crate::{error::QueryError, term_expander::TermExpansionFailure};

    #[test]
    fn an_expansion_failure_names_its_class_in_the_query_error() {
        pretty_assertions::assert_eq!(
            QueryError::TermExpansion(TermExpansionFailure::NotAnIri).to_string(),
            "could not expand a term: the term expands to something that is not an absolute IRI"
        );
    }
}
