//! The JSON-LD engine, bound to one `@context`, as a query term expander.
//!
//! `query` states what it needs from an expander and names nothing in this
//! module; the binding of an engine and a context to that trait happens here,
//! on the JSON-LD side of the boundary.

use crate::jsonld::{ContextSpec, JsonLdEngine};
use andromeda_ngsi_ld_query::{error::QueryError, term_expander::TermExpander};
use iri_rs::IriBuf;

/// A [`JsonLdEngine`] paired with the `@context` one request selected.
///
/// Borrowed rather than owned: it lives for the duration of a single query
/// compilation, and both halves outlive it.
#[derive(Debug, Clone, Copy)]
pub struct ContextualExpander<'a> {
    /// The processor that performs the expansion.
    engine: &'a JsonLdEngine,
    /// The context every term is expanded against.
    context: &'a ContextSpec,
}

impl<'a> ContextualExpander<'a> {
    /// Bind an engine to a context for the duration of one compilation.
    pub const fn new(engine: &'a JsonLdEngine, context: &'a ContextSpec) -> ContextualExpander<'a> {
        ContextualExpander { engine, context }
    }
}

impl TermExpander for ContextualExpander<'_> {
    async fn expand_term(&self, term: &str) -> Result<IriBuf, QueryError> {
        let expanded = self.engine.expand_term(term, self.context).await.map_err(|error| QueryError::TermExpansion {
            term: term.to_string(),
            message: error.to_string(),
        })?;
        as_iri(term, expanded)
    }

    async fn expand_type(&self, term: &str) -> Result<IriBuf, QueryError> {
        let expanded = self.engine.expand_type(term, self.context).await.map_err(|error| QueryError::TermExpansion {
            term: term.to_string(),
            message: error.to_string(),
        })?;
        as_iri(term, expanded)
    }
}

/// Validate an expansion result as an absolute IRI.
fn as_iri(term: &str, expanded: String) -> Result<IriBuf, QueryError> {
    IriBuf::new(expanded).map_err(|error| QueryError::TermNotAnIri {
        term: term.to_string(),
        expanded: error.0,
    })
}
