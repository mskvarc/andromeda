//! A term expander for tests, resolving every term against one namespace.

use crate::{error::QueryError, term_expander::TermExpander};
use iri_rs::{Iri, IriBuf, IriRef, iri};
use std::future::{Future, ready};

/// The namespace every term and type expands into.
pub(crate) const NAMESPACE: Iri<&'static str> = iri!("https://example.org/");

/// The IRI `term` names under [`NAMESPACE`]: the term read as a relative
/// reference and resolved against it (RFC 3986 section 5.2).
pub(crate) fn namespaced(term: &str) -> IriBuf {
    let reference = IriRef::parse(term).expect("a term is a relative IRI reference");
    let resolved = reference.resolved(&NAMESPACE).expect("a term resolves against the namespace");
    IriBuf::try_from(resolved).expect("a reference resolved against an IRI is an IRI")
}

/// Expands every attribute term and every type term to its IRI under
/// [`NAMESPACE`], as a context mapping each term to its own IRI does.
pub(crate) struct PrefixExpander;

impl TermExpander for PrefixExpander {
    fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        ready(Ok(namespaced(term)))
    }

    fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        ready(Ok(namespaced(term)))
    }
}
