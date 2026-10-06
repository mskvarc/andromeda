//! An engine bound to one `@context`, as a query-term expander.
//!
//! `andromeda-ngsi-ld-query` states what it needs from an expander and names
//! nothing here; binding an engine and a context to that trait happens on this
//! side of the boundary, which is what keeps the query language free of
//! JSON-LD.

use crate::{
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
    error::JsonLdError,
};
use andromeda_ngsi_ld_query::{
    error::QueryError,
    term_expander::{TermExpander, TermExpansionFailure},
};
use iri_rs::IriBuf;
use std::future::Future;

/// A [`JsonLdEngine`] paired with the `@context` one request selected.
///
/// Borrowed rather than owned: it lives for one query compilation and both
/// halves outlive it.
#[derive(Debug, Clone)]
pub struct ContextualExpander<'a> {
    /// The processor that performs the expansion.
    engine: &'a JsonLdEngine,
    /// The prepared context every term is expanded against.
    context: &'a PreparedTermContext,
}

impl<'a> ContextualExpander<'a> {
    /// Bind an engine to a context for one compilation.
    #[must_use]
    pub const fn new(engine: &'a JsonLdEngine, context: &'a PreparedTermContext) -> ContextualExpander<'a> {
        ContextualExpander { engine, context }
    }
}

// Spelled with the trait's own `impl Future` return rather than as `async fn`
// bodies. The two are equivalent to the compiler, but rustdoc cannot yet
// document an `async fn` that implements a return-position `impl Future`
// method on a type carrying a lifetime (rust-lang/rust#100013), and this
// crate documents with private items included.
impl TermExpander for ContextualExpander<'_> {
    fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        let (engine, context) = (self.engine, self.context);
        async move { engine.expand_term(term, context).await.map_err(|error| expansion_failed(term, &error)) }
    }

    fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
        let (engine, context) = (self.engine, self.context);
        async move { engine.expand_type(term, context).await.map_err(|error| expansion_failed(term, &error)) }
    }
}

/// The query-language failure a JSON-LD expansion failure becomes.
///
/// The engine already yields an [`IriBuf`], so a term that expands to something
/// that is not an absolute IRI surfaces here as an expansion failure rather than
/// through a separate re-parse. The query language cannot hold the JSON-LD
/// failure itself, so it is recorded here, beside the term, while it is still
/// in hand.
fn expansion_failed(term: &str, error: &JsonLdError) -> QueryError {
    tracing::debug!(term, error = %error, "a query term did not expand against the request @context");
    QueryError::TermExpansion(expansion_failure(error))
}

/// The class of expansion failure a JSON-LD failure is.
const fn expansion_failure(error: &JsonLdError) -> TermExpansionFailure {
    match error {
        JsonLdError::TypeNotExpanded | JsonLdError::TermNotExpanded => TermExpansionFailure::NotExpanded,
        JsonLdError::ExpandedTermNotIri(_) => TermExpansionFailure::NotAnIri,
        JsonLdError::ContextFetchDisabled { .. } | JsonLdError::ContextLoad { .. } | JsonLdError::ContextHttp { .. } | JsonLdError::ContextTooLarge { .. } => {
            TermExpansionFailure::ContextUnavailable
        }
        JsonLdError::ProtectedContextPreload { .. }
        | JsonLdError::ContextInvalid { .. }
        | JsonLdError::PreloadedContextUnreadable { .. }
        | JsonLdError::PreloadedContextInvalid { .. }
        | JsonLdError::PreloadedContextUrl(_)
        | JsonLdError::EmbeddedContextMissing { .. }
        | JsonLdError::EmbeddedContextInvalid { .. }
        | JsonLdError::InlineContextInvalid(_)
        | JsonLdError::ScopedUserContext { .. }
        | JsonLdError::EmbeddedUserContext(_)
        | JsonLdError::ClientBuild(_)
        | JsonLdError::Expansion(_)
        | JsonLdError::Compaction(_)
        | JsonLdError::ContextUrlNotIri(_)
        | JsonLdError::JsonLdContextText(_)
        | JsonLdError::JsonLdContextShape
        | JsonLdError::ContextGraph(_)
        | JsonLdError::Json(_)
        | JsonLdError::ContextText(_)
        | JsonLdError::ProcessorText(_) => TermExpansionFailure::ContextUnprocessable,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        engine::processor::JsonLdEngine,
        error::JsonLdError,
        term_expansion::{ContextualExpander, expansion_failure},
    };
    use andromeda_ngsi_ld_query::term_expander::{TermExpander, TermExpansionFailure};

    #[tokio::test]
    async fn a_core_context_term_expands_to_an_absolute_iri() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = ContextSpec::none();
        let prepared = engine.prepare_term_context(&context);
        let expander = ContextualExpander::new(&engine, &prepared);

        let attribute = expander.expand_term("speed").await.expect("a term expands");
        let entity_type = expander.expand_type("Vehicle").await.expect("a type expands");

        assert!(attribute.as_str().starts_with("https://"), "got {attribute}");
        assert!(entity_type.as_str().starts_with("https://"), "got {entity_type}");
    }

    #[test]
    fn a_json_ld_failure_is_classified_by_what_went_wrong() {
        pretty_assertions::assert_eq!(expansion_failure(&JsonLdError::TermNotExpanded), TermExpansionFailure::NotExpanded);
        pretty_assertions::assert_eq!(expansion_failure(&JsonLdError::TypeNotExpanded), TermExpansionFailure::NotExpanded);
        pretty_assertions::assert_eq!(expansion_failure(&JsonLdError::JsonLdContextShape), TermExpansionFailure::ContextUnprocessable);
    }
}
