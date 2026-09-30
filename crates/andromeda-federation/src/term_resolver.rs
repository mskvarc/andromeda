//! Expanding NGSI-LD names against one operation's inbound `@context`.
//!
//! Forwarding narrowing has to answer whether a registration covers a name, and
//! that is decided by the request's `@context`, never by the string shape of an
//! IRI. Selecting a registration expands a client's term to an IRI and compares
//! IRIs (clause 4.3.6.1: "all constraints specified in the registration shall
//! be respected"); narrowing the forwarded request must reach the same verdict,
//! so it expands the same way. Spelling a name for the source is the other
//! half, and depends on the registration's forwarding context
//! ([`crate::forwarded_names`]).
//!
//! The operations that fan out hold the engine and the request context
//! already; each builds one [`TermResolver`] so the context is resolved once and
//! every term lookup in the operation shares it (the engine caches expansions by
//! term and resolved context).

use crate::error::FederationError;
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
};
use iri_rs::IriBuf;

/// The inbound-context expansion forwarding narrowing needs, prepared once per
/// operation.
pub(crate) struct TermResolver<'a> {
    /// The engine that expands.
    engine: &'a JsonLdEngine,
    /// The request context resolved once, so each lookup is a cache probe.
    prepared: PreparedTermContext,
}

impl<'a> TermResolver<'a> {
    /// Prepare the resolver for one operation, resolving `context` once.
    pub(crate) fn new(engine: &'a JsonLdEngine, context: &ContextSpec) -> TermResolver<'a> {
        TermResolver {
            engine,
            prepared: engine.prepare_term_context(context),
        }
    }

    /// The inbound context, prepared for term lookups.
    pub(crate) const fn prepared(&self) -> &PreparedTermContext {
        &self.prepared
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
}
