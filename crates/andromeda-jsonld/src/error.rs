//! What can go wrong while resolving a context or reshaping a document.
//!
//! Every fallible operation in this crate answers with [`JsonLdError`]. The
//! variants cover context loading (fetch disabled, HTTP status, size, parse),
//! expansion and compaction, IRI validation, document structure, and JSON
//! (de)serialisation. Nothing here decides what a client is told about the
//! failure: clause 5.5.2 categorisation belongs to the caller, which is why
//! the loading variants carry the URL, the status and the reason separately
//! rather than pre-rendered into one message.

use crate::{
    context_graph::ContextGraphError,
    context_term::ContextTerm,
    loader::error::{ContextUnavailable, InvalidContextDocument, LoaderError},
};
use iri_rs::{InvalidIri, IriBuf};
use jsonld::{CompactError, ContextLoadError, ExpandError, compaction, context_processing, expansion};
use jsonld_syntax::{context::InvalidContext, parse};
use reqwest::StatusCode;
use std::{io, path::PathBuf};
use thiserror::Error;

/// A JSON-LD processing failure.
///
/// Carries enough detail for a caller to produce an NGSI-LD Problem Detail
/// without inspecting error chains.
#[derive(Debug, Error)]
pub enum JsonLdError {
    /// Configuration attempted to replace an embedded NGSI-LD core context.
    #[error("preloading the protected NGSI-LD core context {url} is not allowed")]
    ProtectedContextPreload {
        /// The protected core-context URL supplied by configuration.
        url: IriBuf,
    },

    /// Remote context fetch is disabled by configuration.
    #[error("remote context fetch is disabled for {url}")]
    ContextFetchDisabled {
        /// The URL that was requested.
        url: IriBuf,
    },

    /// A remote JSON-LD context could not be loaded.
    #[error("failed to load context from {url}: {source}")]
    ContextLoad {
        /// The URL that was requested.
        url: IriBuf,
        /// Why the context could not be reached.
        #[source]
        source: ContextUnavailable,
    },

    /// The HTTP request returned a non-success status code.
    #[error("loading context from {url} returned HTTP {status}")]
    ContextHttp {
        /// The URL that was requested.
        url: IriBuf,
        /// HTTP status code returned by the server.
        status: StatusCode,
    },

    /// The remote context exceeded the configured size limit.
    #[error("context at {url} is too large ({size} bytes exceeds the {max}-byte limit)")]
    ContextTooLarge {
        /// The URL that was requested.
        url: IriBuf,
        /// Actual payload size in bytes.
        size: usize,
        /// Configured maximum in bytes.
        max: usize,
    },

    /// The remote context was retrieved but is not a usable JSON-LD context.
    #[error("invalid JSON-LD context at {url}: {source}")]
    ContextInvalid {
        /// The URL that was requested.
        url: IriBuf,
        /// What the retrieved document is missing.
        #[source]
        source: InvalidContextDocument,
    },

    /// A context file configured for preloading could not be read.
    #[error("the preloaded context file {} cannot be read: {source}", path.display())]
    PreloadedContextUnreadable {
        /// The configured file.
        path: PathBuf,
        /// The failed read.
        #[source]
        source: io::Error,
    },

    /// A context file configured for preloading is not JSON.
    #[error("the preloaded context file {} is not JSON: {source}", path.display())]
    PreloadedContextInvalid {
        /// The configured file.
        path: PathBuf,
        /// The JSON parse failure.
        #[source]
        source: parse::Error,
    },

    /// A URL configured for preloading is not an IRI.
    #[error("a preloaded context URL is not an IRI")]
    PreloadedContextUrl(#[source] InvalidIri<String>),

    /// A core context URL has no payload embedded in the broker.
    #[error("no embedded payload exists for the context {url}")]
    EmbeddedContextMissing {
        /// The core context URL without a payload.
        url: IriBuf,
    },

    /// A context embedded in the broker is not JSON.
    #[error("the embedded context {url} is not JSON: {source}")]
    EmbeddedContextInvalid {
        /// The embedded context's URL.
        url: IriBuf,
        /// The JSON parse failure.
        #[source]
        source: parse::Error,
    },

    /// An inline context could not be parsed as valid JSON-LD.
    #[error("inline context is invalid: {0}")]
    InlineContextInvalid(#[source] InvalidContext),

    /// A user `@context` declares a JSON-LD Scoped Context.
    ///
    /// ETSI GS CIM 009 v1.9.1 clause 5.5.7: the user `@context` "shall not
    /// contain JSON-LD Scoped Contexts", because one "could be used to modify
    /// terms defined in the Core `@context` or to reshape NGSI-LD Elements
    /// during the expansion of terms".
    #[error("the user @context defines the term {term} with a scoped @context, which is not allowed")]
    ScopedUserContext {
        /// The term whose definition carries the scoped `@context`.
        term: ContextTerm,
    },

    /// The HTTP client could not be constructed from the configured settings.
    #[error("failed to build the JSON-LD HTTP client: {0}")]
    ClientBuild(#[source] reqwest::Error),

    /// JSON-LD expansion failed.
    #[error("failed to expand the document: {0}")]
    Expansion(#[source] Box<ExpandError<LoaderError>>),

    /// JSON-LD compaction failed.
    #[error("failed to compact the document: {0}")]
    Compaction(#[source] Box<CompactError<LoaderError>>),

    /// An entity type term expands to nothing against the active context.
    #[error("the entity type does not expand to an IRI against the active context")]
    TypeNotExpanded,

    /// An attribute term expands to nothing against the active context.
    #[error("the term does not expand to an IRI against the active context")]
    TermNotExpanded,

    /// A term expanded to text that is not an absolute IRI.
    #[error("the expanded term is not an absolute IRI")]
    ExpandedTermNotIri(#[source] InvalidIri<String>),

    /// A context URL handed to the resolver is not an IRI.
    #[error("the context URL is not an IRI")]
    ContextUrlNotIri(#[source] InvalidIri<String>),

    /// A `jsonldContext` written as JSON text does not parse.
    #[error("jsonldContext is not valid JSON: {0}")]
    JsonLdContextText(#[source] serde_json::Error),

    /// A `jsonldContext` value names no context in any form.
    #[error("jsonldContext must be a valid URI or JSON-LD context")]
    JsonLdContextShape,

    /// A document to carry a `@context` is neither an object nor an array.
    #[error("the document must be an object or an array")]
    DocumentShape,

    /// A `@context`'s references form a cycle or nest too deep.
    ///
    /// Clause 5.5.6 makes an invalid remote `@context` a `BadRequestData`, and
    /// one the processor could only overflow on is invalid (JSON-LD 1.1 API
    /// section 4.1.2 step 5.2.3).
    #[error("invalid @context: {0}")]
    ContextGraph(#[from] ContextGraphError),

    /// JSON serialisation / deserialisation error.
    #[error("JSON processing failed: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<ExpandError<LoaderError>> for JsonLdError {
    fn from(error: ExpandError<LoaderError>) -> JsonLdError {
        match context_error_of_expansion(&error) {
            Some(context_error) => context_error,
            None => JsonLdError::Expansion(Box::new(error)),
        }
    }
}

impl From<CompactError<LoaderError>> for JsonLdError {
    fn from(error: CompactError<LoaderError>) -> JsonLdError {
        match context_error_of_compaction(&error) {
            Some(context_error) => context_error,
            None => JsonLdError::Compaction(Box::new(error)),
        }
    }
}

impl JsonLdError {
    /// Whether this is an explicit context-loading failure.
    ///
    /// These variants carry the URL, status and reason a clause 5.5.2
    /// `LdContextNotAvailable` Problem Detail needs, so a caller can surface
    /// them directly instead of reporting an internal error.
    #[must_use]
    pub const fn is_context_not_available(&self) -> bool {
        matches!(
            self,
            JsonLdError::ContextFetchDisabled { .. } | JsonLdError::ContextHttp { .. } | JsonLdError::ContextLoad { .. } | JsonLdError::ContextTooLarge { .. }
        )
    }

    /// Whether a [`CompactError`] was caused by a protected-term redefinition.
    ///
    /// Decides whether falling back to core-only compaction is appropriate:
    /// a user context that redefines a protected core term is the one failure
    /// a retry without that context can fix.
    #[must_use]
    pub const fn is_protected_term_error(error: &CompactError<LoaderError>) -> bool {
        match error {
            CompactError::ContextProcessing(processing) | CompactError::Expand(ExpandError::ContextProcessing(processing)) => {
                is_protected_term_redefinition(processing)
            }
            CompactError::Compaction(compacting) => {
                if let compaction::Error::ContextProcessing(processing) = compacting {
                    is_protected_term_redefinition(processing)
                } else {
                    false
                }
            }
            CompactError::Expand(ExpandError::Expansion(expanding)) => {
                if let expansion::Error::ContextProcessing(processing) = expanding {
                    is_protected_term_redefinition(processing)
                } else {
                    false
                }
            }
            CompactError::Loading(_) | CompactError::ContextLoading(_) | CompactError::Expand(ExpandError::Loading(_) | ExpandError::ContextLoading(_)) => {
                false
            }
        }
    }
}

/// Whether a context-processing failure is specifically a protected-term
/// redefinition.
const fn is_protected_term_redefinition(error: &context_processing::Error<LoaderError>) -> bool {
    matches!(error, context_processing::Error::ProtectedTermRedefinition)
}

/// Recover the loader failure buried inside a typed [`ExpandError`].
///
/// The typed `LoadError<LoaderError>` exposes its source directly, so this is
/// a match rather than a chain of `Box<dyn Error>` downcasts.
fn context_error_of_expansion(error: &ExpandError<LoaderError>) -> Option<JsonLdError> {
    match error {
        ExpandError::Loading(load) | ExpandError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load)) => Some(JsonLdError::from(&load.source)),
        ExpandError::ContextLoading(ContextLoadError::ContextExtractionFailed(_)) => None,
        ExpandError::ContextProcessing(processing) => context_error_of_processing(processing),
        ExpandError::Expansion(expanding) => {
            if let expansion::Error::ContextProcessing(processing) = expanding {
                context_error_of_processing(processing)
            } else {
                None
            }
        }
    }
}

/// The [`context_error_of_expansion`] rule, over a typed [`CompactError`].
fn context_error_of_compaction(error: &CompactError<LoaderError>) -> Option<JsonLdError> {
    match error {
        CompactError::Loading(load) | CompactError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load)) => Some(JsonLdError::from(&load.source)),
        CompactError::ContextLoading(ContextLoadError::ContextExtractionFailed(_)) | CompactError::Compaction(_) => None,
        CompactError::ContextProcessing(processing) => context_error_of_processing(processing),
        CompactError::Expand(expand) => context_error_of_expansion(expand),
    }
}

/// The loader failure a context-processing error wraps, if it wraps one, or
/// the invalid reference graph it reports.
///
/// A `context overflow` or a `recursive context inclusion` is a `@context`
/// whose references cycle or run on, which clause 5.5.6 answers with
/// `BadRequestData` exactly as [`crate::context_graph`] refuses it.
///
/// Written as `if let` rather than an exhaustive match because
/// `context_processing::Error` is a foreign enum of twenty-odd grammar
/// violations, none of which carries a loader failure or a reference-graph
/// fault, and enumerating them here would only restate that list.
fn context_error_of_processing(error: &context_processing::Error<LoaderError>) -> Option<JsonLdError> {
    if let context_processing::Error::ContextLoadingFailed(load) = error {
        Some(JsonLdError::from(&load.source))
    } else if matches!(
        error,
        context_processing::Error::ContextOverflow | context_processing::Error::RecursiveContextInclusion
    ) {
        Some(ContextGraphError::ProcessorOverflow.into())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::ContextCacheError,
        context_graph::ContextGraphError,
        context_term::ContextTerm,
        error::{JsonLdError, context_error_of_compaction, context_error_of_expansion, context_error_of_processing, is_protected_term_redefinition},
        loader::error::{ContextUnavailable, InvalidContextDocument, LoaderError, LoaderErrorKind},
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use iri_rs::IriBuf;
    use jsonld::{CompactError, ContextLoadError, ExpandError, ExtractContextError, LoadError, compaction, context_processing, expansion};
    use reqwest::StatusCode;

    fn url() -> IriBuf {
        IriBuf::new("https://example.com/ctx.jsonld".to_string()).expect("valid URL")
    }

    fn loader_error() -> LoaderError {
        LoaderError {
            url: url(),
            kind: LoaderErrorKind::Unavailable(ContextUnavailable::Cache(ContextCacheError::Read(PersistenceFailure::Unavailable))),
        }
    }

    fn load_error() -> LoadError<LoaderError> {
        LoadError::new(url(), loader_error())
    }

    #[test]
    fn only_unavailable_remote_contexts_are_reportable_as_context_not_available() {
        let available = [
            JsonLdError::ContextFetchDisabled { url: url() },
            JsonLdError::ContextHttp {
                url: url(),
                status: StatusCode::NOT_FOUND,
            },
            JsonLdError::ContextLoad {
                url: url(),
                source: ContextUnavailable::Cache(ContextCacheError::Read(PersistenceFailure::Unavailable)),
            },
            JsonLdError::ContextTooLarge { url: url(), size: 10, max: 5 },
        ];
        for error in &available {
            assert!(error.is_context_not_available(), "{error} must report as LdContextNotAvailable");
        }

        let internal = [
            JsonLdError::ContextInvalid {
                url: url(),
                source: InvalidContextDocument::NoContext,
            },
            JsonLdError::EmbeddedContextMissing { url: url() },
            JsonLdError::TermNotExpanded,
            JsonLdError::TypeNotExpanded,
            JsonLdError::JsonLdContextShape,
            JsonLdError::DocumentShape,
            JsonLdError::ScopedUserContext {
                term: ContextTerm::new("speed"),
            },
        ];
        for error in &internal {
            assert!(!error.is_context_not_available(), "{error} must not report as a loading failure");
        }
    }

    #[test]
    fn only_the_protected_term_redefinition_variant_is_classified_as_one() {
        assert!(is_protected_term_redefinition(&context_processing::Error::ProtectedTermRedefinition));
        assert!(!is_protected_term_redefinition(&context_processing::Error::InvalidBaseIri));
        assert!(!is_protected_term_redefinition(&context_processing::Error::ContextLoadingFailed(load_error())));
    }

    #[test]
    fn a_protected_term_redefinition_is_recognised_through_every_nesting() {
        let cases: [CompactError<LoaderError>; 4] = [
            CompactError::ContextProcessing(context_processing::Error::ProtectedTermRedefinition),
            CompactError::Expand(ExpandError::ContextProcessing(context_processing::Error::ProtectedTermRedefinition)),
            CompactError::Compaction(compaction::Error::ContextProcessing(context_processing::Error::ProtectedTermRedefinition)),
            CompactError::Expand(ExpandError::Expansion(expansion::Error::ContextProcessing(
                context_processing::Error::ProtectedTermRedefinition,
            ))),
        ];
        for case in &cases {
            assert!(JsonLdError::is_protected_term_error(case), "must detect the redefinition in {case}");
        }
    }

    #[test]
    fn a_loading_or_unrelated_failure_is_not_a_protected_term_error() {
        let cases: [CompactError<LoaderError>; 8] = [
            CompactError::Loading(load_error()),
            CompactError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load_error())),
            CompactError::Expand(ExpandError::Loading(load_error())),
            CompactError::Expand(ExpandError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load_error()))),
            CompactError::ContextProcessing(context_processing::Error::InvalidBaseIri),
            CompactError::Compaction(compaction::Error::ContextProcessing(context_processing::Error::InvalidBaseIri)),
            CompactError::Compaction(compaction::Error::IriConfusedWithPrefix),
            CompactError::Expand(ExpandError::Expansion(expansion::Error::InvalidTypeValue)),
        ];
        for case in &cases {
            assert!(!JsonLdError::is_protected_term_error(case), "must not treat {case} as a protected-term error");
        }
    }

    #[test]
    fn context_error_of_expansion_recovers_only_the_loader_failures() {
        assert!(
            context_error_of_expansion(&ExpandError::Loading(load_error())).is_some_and(|error| error.is_context_not_available()),
            "a direct load failure is a context error"
        );
        assert!(
            context_error_of_expansion(&ExpandError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load_error()))).is_some(),
            "a failed context document load is a context error"
        );
        assert!(
            context_error_of_expansion(&ExpandError::ContextLoading(ContextLoadError::ContextExtractionFailed(
                ExtractContextError::NoContext
            )))
            .is_none(),
            "a missing @context is a document defect, not a load failure"
        );
        assert!(
            context_error_of_expansion(&ExpandError::ContextProcessing(context_processing::Error::ContextLoadingFailed(load_error()))).is_some(),
            "a load failure nested in context processing is still a context error"
        );
        assert!(
            context_error_of_expansion(&ExpandError::ContextProcessing(context_processing::Error::ProtectedTermRedefinition)).is_none(),
            "a protected-term redefinition is not a load failure"
        );
        assert!(
            context_error_of_expansion(&ExpandError::Expansion(expansion::Error::ContextProcessing(
                context_processing::Error::ContextLoadingFailed(load_error())
            )))
            .is_some(),
            "a load failure nested in expansion's context processing is a context error"
        );
        assert!(
            context_error_of_expansion(&ExpandError::Expansion(expansion::Error::InvalidTypeValue)).is_none(),
            "a pure expansion defect is not a context error"
        );
    }

    /// Clause 5.5.6: a `@context` the processor overflows on is invalid, so
    /// wherever the processor reports it, it becomes the typed reference-graph
    /// refusal rather than an opaque expansion or compaction failure.
    #[test]
    fn a_processor_overflow_or_recursive_inclusion_is_a_context_graph_refusal() {
        let refusals: [fn() -> context_processing::Error<LoaderError>; 2] = [
            || context_processing::Error::ContextOverflow,
            || context_processing::Error::RecursiveContextInclusion,
        ];
        for refusal in refusals {
            let expanded = JsonLdError::from(ExpandError::ContextProcessing(refusal()));
            assert!(
                matches!(expanded, JsonLdError::ContextGraph(ContextGraphError::ProcessorOverflow)),
                "{expanded}"
            );
            let compacted = JsonLdError::from(CompactError::ContextProcessing(refusal()));
            assert!(
                matches!(compacted, JsonLdError::ContextGraph(ContextGraphError::ProcessorOverflow)),
                "{compacted}"
            );
        }
    }

    #[test]
    fn context_error_of_compaction_recovers_only_the_loader_failures() {
        assert!(context_error_of_compaction(&CompactError::Loading(load_error())).is_some());
        assert!(context_error_of_compaction(&CompactError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load_error()))).is_some());
        assert!(
            context_error_of_compaction(&CompactError::ContextLoading(ContextLoadError::ContextExtractionFailed(
                ExtractContextError::NoContext
            )))
            .is_none()
        );
        assert!(context_error_of_compaction(&CompactError::ContextProcessing(context_processing::Error::ContextLoadingFailed(load_error()))).is_some());
        assert!(
            context_error_of_compaction(&CompactError::Expand(ExpandError::Loading(load_error()))).is_some(),
            "compaction delegates its Expand arm to the expansion rule"
        );
        assert!(context_error_of_compaction(&CompactError::Compaction(compaction::Error::IriConfusedWithPrefix)).is_none());
    }

    #[test]
    fn context_error_of_processing_maps_only_the_loading_failure() {
        assert!(context_error_of_processing(&context_processing::Error::ContextLoadingFailed(load_error())).is_some());
        assert!(context_error_of_processing(&context_processing::Error::ProtectedTermRedefinition).is_none());
    }

    #[test]
    fn from_expand_error_maps_a_loader_failure_and_boxes_everything_else() {
        let mapped = JsonLdError::from(ExpandError::Loading(load_error()));
        assert!(mapped.is_context_not_available(), "a load failure surfaces as a context error, got {mapped}");

        let boxed = JsonLdError::from(ExpandError::Expansion(expansion::Error::InvalidTypeValue));
        assert!(matches!(boxed, JsonLdError::Expansion(_)), "a pure expansion failure is boxed, got {boxed}");
    }

    #[test]
    fn from_compact_error_maps_a_loader_failure_and_boxes_everything_else() {
        let mapped = JsonLdError::from(CompactError::Loading(load_error()));
        assert!(mapped.is_context_not_available(), "a load failure surfaces as a context error, got {mapped}");

        let boxed = JsonLdError::from(CompactError::Compaction(compaction::Error::IriConfusedWithPrefix));
        assert!(matches!(boxed, JsonLdError::Compaction(_)), "a pure compaction failure is boxed, got {boxed}");
    }
}
