//! Expanding and compacting NGSI-LD names against one operation's `@context`.
//!
//! Forwarding narrowing has to answer two questions about a name, and both are
//! decided by the request's `@context`, never by the string shape of an IRI.
//! Selecting a registration expands a client's term to an IRI and compares IRIs
//! (clause 4.3.6.1: "all constraints specified in the registration shall be
//! respected"); narrowing the forwarded request must reach the same verdict, so
//! it expands the same way. Naming a covered term back to a source uses the
//! `@context`'s own compaction, which is exact even when a context aliases a
//! term to an unrelated IRI -- the case a last-segment heuristic gets wrong.
//!
//! The four operations that fan out hold the engine and the request context
//! already; each builds one [`TermResolver`] so the context is resolved once and
//! every term lookup in the operation shares it (the engine caches expansions by
//! term and resolved context).

use crate::error::FederationError;
use andromeda_jsonld::{
    compaction::NameCompactor,
    context_spec::ContextSpec,
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
};
use iri_rs::IriBuf;

/// The engine work forwarding narrowing needs, prepared once per operation.
pub(crate) struct TermResolver<'a> {
    /// The engine that expands.
    engine: &'a JsonLdEngine,
    /// The request context resolved once, so each lookup is a cache probe.
    prepared: PreparedTermContext,
    /// Compacts every name against the request context, processed once.
    names: NameCompactor<'a>,
}

impl<'a> TermResolver<'a> {
    /// Prepare the resolver for one operation, resolving `context` once.
    pub(crate) fn new(engine: &'a JsonLdEngine, context: &'a ContextSpec) -> TermResolver<'a> {
        TermResolver {
            engine,
            prepared: engine.prepare_term_context(context),
            names: NameCompactor::new(engine, context),
        }
    }

    /// Expand an entity type term to its IRI.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the term does not expand against
    /// the request context.
    pub(crate) async fn expand_type(&self, term: &str) -> Result<IriBuf, FederationError> {
        Ok(self.engine.expand_type(term, &self.prepared).await?)
    }

    /// Expand an attribute term to its IRI.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the term does not expand against
    /// the request context.
    pub(crate) async fn expand_attr(&self, term: &str) -> Result<IriBuf, FederationError> {
        Ok(self.engine.expand_term(term, &self.prepared).await?)
    }

    /// Compact an entity type IRI to the term the request context spells it
    /// with, or the IRI itself when the context abbreviates none (clause 4.5.1).
    pub(crate) async fn compact_type(&self, iri: &str) -> String {
        self.names.compact_type_iri(iri).await
    }

    /// Compact an attribute IRI to the term the request context spells it with,
    /// or the IRI itself when the context abbreviates none (clause 4.5.1).
    pub(crate) async fn compact_attr(&self, iri: &str) -> String {
        self.names.compact_attr_iri(iri).await
    }
}
