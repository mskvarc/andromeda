//! A term expander for tests that resolves terms against a namespace, standing in for a request `@context` without a JSON-LD processor.

use crate::{
    filter::{Filter, filter_error::FilterError},
    name_stage::{Expanded, Written},
    query_error::QueryError,
    term_expander::{TermExpander, TermExpansionError},
};
use andromeda_ngsi_ld_lexicon::{attribute_iri::AttributeIri, entity_type_iri::EntityTypeIri};
use iri_rs::{Iri, IriBuf, IriRef, iri};
use std::future::{Future, ready};

/// An expander for `https://example.com/`: attribute and type terms both resolve under it.
pub const EXAMPLE_NAMESPACE: NamespaceExpander = NamespaceExpander::uniform(iri!("https://example.com/"));

/// An expander for `https://example.com/`: attribute terms resolve under it and type terms under `https://example.com/types/`.
pub const EXAMPLE_NAMESPACE_TYPED: NamespaceExpander = NamespaceExpander::new(iri!("https://example.com/"), iri!("https://example.com/types/"));

/// Expands a term by resolving it as a relative reference against a namespace (RFC 3986 section 5.2), so a short name lands under the namespace and an absolute IRI stays as written.
///
/// Attribute terms and type terms resolve against separate namespaces. Terms listed as core members expand to their fixed IRI instead, as the core `@context` maps them.
#[derive(Clone, Copy, Debug)]
pub struct NamespaceExpander {
    /// The namespace attribute terms resolve against.
    terms: Iri<&'static str>,
    /// The namespace type terms resolve against.
    types: Iri<&'static str>,
    /// Attribute terms with a fixed IRI of their own.
    core_members: &'static [(&'static str, Iri<&'static str>)],
}

impl NamespaceExpander {
    /// An expander resolving attribute terms against `terms` and type terms against `types`.
    #[must_use]
    pub const fn new(terms: Iri<&'static str>, types: Iri<&'static str>) -> NamespaceExpander {
        NamespaceExpander {
            terms,
            types,
            core_members: &[],
        }
    }

    /// An expander resolving attribute and type terms against `namespace`.
    #[must_use]
    pub const fn uniform(namespace: Iri<&'static str>) -> NamespaceExpander {
        NamespaceExpander::new(namespace, namespace)
    }

    /// This expander, with every attribute term in `core_members` expanding to the IRI it is paired with.
    #[must_use]
    pub const fn with_core_members(self, core_members: &'static [(&'static str, Iri<&'static str>)]) -> NamespaceExpander {
        NamespaceExpander { core_members, ..self }
    }

    /// The IRI the attribute `term` expands to.
    ///
    /// # Errors
    ///
    /// [`TermExpansionError::NotAnIri`] when `term` is no IRI reference or does not resolve.
    pub fn term_iri(&self, term: &str) -> Result<IriBuf, TermExpansionError> {
        match self.core_members.iter().find(|(core_term, _)| *core_term == term) {
            Some((_, iri)) => Ok(IriBuf::from(*iri)),
            None => resolved(term, &self.terms),
        }
    }

    /// The IRI the type `term` expands to.
    ///
    /// # Errors
    ///
    /// [`TermExpansionError::NotAnIri`] when `term` is no IRI reference or does not resolve.
    pub fn type_iri(&self, term: &str) -> Result<IriBuf, TermExpansionError> {
        resolved(term, &self.types)
    }

    /// The filter `filter` with each attribute and entity type it names resolved by this namespace.
    ///
    /// The synchronous counterpart of [`expand_filter`](crate::filter::expansion::expand_filter) for a filter that needs no sub-attribute, member or operand expansion: only the names that make the expanded stage are rewritten.
    ///
    /// # Errors
    ///
    /// The [`QueryError`] of a name that does not resolve to a usable IRI.
    pub fn expanded(&self, filter: Filter<Written>) -> Result<Filter<Expanded>, QueryError> {
        filter.into_expanded_by(
            &|name| Ok(AttributeIri::try_from(self.term_iri(name.as_str())?).map_err(FilterError::from)?),
            &|name| Ok(EntityTypeIri::from(self.type_iri(name.as_str())?)),
        )
    }
}

impl TermExpander for NamespaceExpander {
    fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
        ready(self.term_iri(term))
    }

    fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, TermExpansionError>> + Send {
        ready(self.type_iri(term))
    }
}

/// `term` resolved against `namespace`.
fn resolved(term: &str, namespace: &Iri<&'static str>) -> Result<IriBuf, TermExpansionError> {
    let reference = IriRef::parse(term).map_err(|_| not_an_iri())?;
    let resolved = reference.resolved(namespace).map_err(|_| not_an_iri())?;
    IriBuf::try_from(resolved).map_err(|_| not_an_iri())
}

/// The failure of a term that names no IRI.
const fn not_an_iri() -> TermExpansionError {
    TermExpansionError::NotAnIri
}
