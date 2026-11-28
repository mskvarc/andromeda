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
    context_fault::{ContextFault, FaultyContext},
    context_graph::ContextGraphError,
    context_term::ContextTerm,
    engine::{
        context_fault_reading::{extraction_failure, processing_fault, syntax_fault},
        processor_text::ProcessorTextError,
    },
    loader::error::{ContextUnavailable, InvalidContextDocument, LoaderError},
};
use andromeda_json_literal::{admission::JsonLiteralError, json_text::quoted};
use andromeda_ngsi_ld_representation::embedded_context_admission::EmbeddedContext;
use iri_rs::{InvalidIri, IriBuf};
use jsonld::{CompactError, ContextLoadError, ExpandError, ExtractContextError, compaction, context_processing, expansion};
use reqwest::StatusCode;
use std::{io, path::PathBuf};
use thiserror::Error;

/// A JSON-LD processing failure.
///
/// Carries enough detail for a caller to produce an NGSI-LD Problem Detail
/// without inspecting error chains.
#[derive(Debug, Error)]
pub enum JsonLdError {
    /// Configuration attempted to replace a context compiled into the broker:
    /// an NGSI-LD core context or the W3C Data Integrity context.
    #[error("preloading the context {url} is not allowed, because the broker embeds it")]
    ProtectedContextPreload {
        /// The embedded context's URL supplied by configuration.
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
        /// The JSON admission failure.
        #[source]
        source: JsonLiteralError,
    },

    /// A URL configured for preloading is not an IRI.
    #[error("a preloaded context URL is not an IRI")]
    PreloadedContextUrl(#[source] InvalidIri<String>),

    /// A URL the broker embeds a context under has no payload.
    #[error("no embedded payload exists for the context {url}")]
    EmbeddedContextMissing {
        /// The embedded context URL without a payload.
        url: IriBuf,
    },

    /// A context embedded in the broker is not JSON.
    #[error("the embedded context {url} is not JSON: {source}")]
    EmbeddedContextInvalid {
        /// The embedded context's URL.
        url: IriBuf,
        /// The JSON admission failure.
        #[source]
        source: JsonLiteralError,
    },

    /// A `@context` is not valid JSON-LD: the processor refused its syntax or
    /// the processing of one of its definitions, with no document it names
    /// failing to load.
    ///
    /// Clause 5.5.4: data that is "not syntactically correct, according to the
    /// @context" is `BadRequestData`, and so is the `@context` itself.
    #[error("{context} {fault}")]
    ContextUnprocessable {
        /// The `@context` the fault was found in.
        context: FaultyContext,
        /// What is wrong with it, and where.
        fault: ContextFault,
    },

    /// A user `@context` declares a JSON-LD Scoped Context.
    ///
    /// ETSI GS CIM 009 v1.9.1 clause 5.5.7: the user `@context` "shall not
    /// contain JSON-LD Scoped Contexts", because one "could be used to modify
    /// terms defined in the Core `@context` or to reshape NGSI-LD Elements
    /// during the expansion of terms".
    #[error("the user @context defines the term {} with a scoped @context, which is not allowed", quoted(.term.as_str()))]
    ScopedUserContext {
        /// The term whose definition carries the scoped `@context`, named
        /// quoted as JSON writes the key.
        term: ContextTerm,
    },

    /// Clause 5.5.7: the user `@context` shall not "be embedded into NGSI-LD
    /// Attributes, i.e. there cannot be parts of the user @context other than
    /// at the top level of the NGSI-LD document".
    #[error(transparent)]
    EmbeddedUserContext(#[from] EmbeddedContext),

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

    /// An attribute term expands to a JSON-LD keyword against the active
    /// context, as the core context's `type` and `id` do.
    #[error("the term expands to a JSON-LD keyword against the active context")]
    TermIsKeyword,

    /// A term expanded to text that is not an absolute IRI.
    #[error("the expanded term is not an absolute IRI")]
    ExpandedTermNotIri(#[source] InvalidIri<String>),

    /// A context URL handed to the resolver is not an IRI.
    #[error("the context URL is not an IRI")]
    ContextUrlNotIri(#[source] InvalidIri<String>),

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

    /// A `@context` value assembled from the contexts a request named could
    /// not be held as JSON text, which text written from admitted pieces never
    /// fails to be.
    #[error("the @context value could not be written: {0}")]
    ContextText(#[from] JsonLiteralError),

    /// A document handed to the JSON-LD processor is not JSON text the
    /// processor reads, which the broker's own writers never produce.
    #[error("the document is not JSON text the processor reads: {0}")]
    ProcessorText(#[from] ProcessorTextError),
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
    pub fn is_protected_term_error(error: &CompactError<LoaderError>) -> bool {
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
/// redefinition, located in the term redefined or not.
fn is_protected_term_redefinition(error: &context_processing::Error<LoaderError>) -> bool {
    matches!(error.cause(), context_processing::Error::ProtectedTermRedefinition)
}

/// The typed context failure buried inside an [`ExpandError`], or `None` for
/// a defect of the document being expanded.
///
/// The typed `LoadError<LoaderError>` exposes its source directly, so no
/// downcast is needed. The context the failure is in has no name here.
fn context_error_of_expansion(error: &ExpandError<LoaderError>) -> Option<JsonLdError> {
    match error {
        ExpandError::Loading(load) | ExpandError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load)) => Some(JsonLdError::from(&load.source)),
        ExpandError::ContextLoading(ContextLoadError::ContextExtractionFailed { target, error }) => Some(unextractable_context(target, error)),
        ExpandError::ContextProcessing(processing) => Some(context_error_of_processing(processing, FaultyContext::Unnamed)),
        // Written as `if let` rather than an exhaustive match because
        // `expansion::Error` is a foreign enum of document grammar violations,
        // of which only these two concern the `@context`.
        ExpandError::Expansion(expanding) => {
            if let expansion::Error::ContextProcessing(processing) = expanding {
                Some(context_error_of_processing(processing, FaultyContext::Unnamed))
            } else if let expansion::Error::ContextSyntax(invalid) = expanding {
                Some(JsonLdError::ContextUnprocessable {
                    context: FaultyContext::Unnamed,
                    fault: syntax_fault(invalid),
                })
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
        CompactError::ContextLoading(ContextLoadError::ContextExtractionFailed { target, error }) => Some(unextractable_context(target, error)),
        CompactError::ContextProcessing(processing) => Some(context_error_of_processing(processing, FaultyContext::Unnamed)),
        // Written as `if let` for the reason `context_error_of_expansion` gives.
        CompactError::Compaction(compacting) => {
            if let compaction::Error::ContextProcessing(processing) = compacting {
                Some(context_error_of_processing(processing, FaultyContext::Unnamed))
            } else {
                None
            }
        }
        CompactError::Expand(expand) => context_error_of_expansion(expand),
    }
}

/// The typed failure a context-processing error reports for the `@context`
/// named `context`.
///
/// A loader failure keeps its URL, status and reason. A `context overflow` or
/// a `recursive context inclusion` is a `@context` whose references cycle or
/// run on, which clause 5.5.6 answers with `BadRequestData` exactly as
/// [`crate::context_graph`] refuses it. A document the processor loaded but
/// read no context from is named by its URL. Every other failure is a fault
/// of the `@context` itself, located in the term whose definition failed.
///
/// Written as `if let` rather than an exhaustive match because
/// `context_processing::Error` is a foreign enum of twenty-odd grammar
/// violations, every one of which is the same fault of the `@context`, and
/// enumerating them here would only restate that list.
pub(crate) fn context_error_of_processing(error: &context_processing::Error<LoaderError>, context: FaultyContext) -> JsonLdError {
    let cause = error.cause();
    if let context_processing::Error::ContextLoadingFailed(load) = cause {
        JsonLdError::from(&load.source)
    } else if let context_processing::Error::ContextExtractionFailed { target, error } = cause {
        unextractable_context(target, error)
    } else if matches!(
        cause,
        context_processing::Error::ContextOverflow | context_processing::Error::RecursiveContextInclusion
    ) {
        ContextGraphError::ProcessorOverflow.into()
    } else {
        JsonLdError::ContextUnprocessable {
            context,
            fault: processing_fault(error),
        }
    }
}

/// The failure of a document at `target` the processor loaded but read no
/// context from.
fn unextractable_context(target: &IriBuf, error: &ExtractContextError) -> JsonLdError {
    JsonLdError::ContextInvalid {
        url: target.clone(), // clone: the processor keeps its error, and the failure owns the URL
        source: extraction_failure(error),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::ContextCacheError,
        context_fault::{ContextFault, FaultyContext},
        context_graph::ContextGraphError,
        context_subtree::UnusableLocalContext,
        context_term::ContextTerm,
        error::{JsonLdError, context_error_of_compaction, context_error_of_expansion, context_error_of_processing, is_protected_term_redefinition},
        loader::error::{ContextUnavailable, InvalidContextDocument, LoaderError, LoaderErrorKind},
    };
    use andromeda_ngsi_ld_representation::embedded_context_admission::find_embedded_context;
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use iri_rs::IriBuf;
    use jsonld::{
        CompactError,
        ContextLoadError,
        ExpandError,
        ExtractContextError,
        LoadError,
        compaction,
        context_processing,
        expansion,
        syntax::{ErrorCode, context::definition::KeyOrKeywordRef},
    };
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

    /// `error` as the processor reports it while defining the term `term`.
    fn located(term: &str, error: context_processing::Error<LoaderError>) -> context_processing::Error<LoaderError> {
        context_processing::Error::TermDefinition {
            term: KeyOrKeywordRef::from(term).to_owned(),
            error: Box::new(error),
        }
    }

    /// The document at [`url`] the processor loaded and read no `@context` from.
    fn unextractable() -> ContextLoadError<LoaderError> {
        ContextLoadError::ContextExtractionFailed {
            target: url(),
            error: ExtractContextError::NoContext,
        }
    }

    /// Whether `error` names the unextractable document at [`url`].
    fn names_the_unextractable_document(error: Option<JsonLdError>) -> bool {
        matches!(
            error,
            Some(JsonLdError::ContextInvalid {
                url: named,
                source: InvalidContextDocument::NoContext(UnusableLocalContext::NoContextMember),
            }) if named == url()
        )
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
                source: InvalidContextDocument::NoContext(UnusableLocalContext::NoContextMember),
            },
            JsonLdError::EmbeddedContextMissing { url: url() },
            JsonLdError::TermNotExpanded,
            JsonLdError::TypeNotExpanded,
            JsonLdError::ScopedUserContext {
                term: ContextTerm::new("speed"),
            },
            JsonLdError::EmbeddedUserContext(
                find_embedded_context(br#"{"speed": {"@context": {}}}"#)
                    .expect("valid JSON")
                    .expect("an embedded context"),
            ),
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
        assert!(is_protected_term_redefinition(&located(
            "type",
            context_processing::Error::ProtectedTermRedefinition
        )));
    }

    #[test]
    fn a_protected_term_redefinition_is_recognised_through_every_nesting() {
        let cases: [CompactError<LoaderError>; 5] = [
            CompactError::ContextProcessing(context_processing::Error::ProtectedTermRedefinition),
            CompactError::ContextProcessing(located("type", context_processing::Error::ProtectedTermRedefinition)),
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
    fn context_error_of_expansion_types_every_context_failure_and_leaves_document_defects() {
        assert!(
            context_error_of_expansion(&ExpandError::Loading(load_error())).is_some_and(|error| error.is_context_not_available()),
            "a direct load failure is a context error"
        );
        assert!(
            context_error_of_expansion(&ExpandError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load_error()))).is_some(),
            "a failed context document load is a context error"
        );
        assert!(
            names_the_unextractable_document(context_error_of_expansion(&ExpandError::ContextLoading(unextractable()))),
            "a document with no @context is named by its URL"
        );
        assert!(
            context_error_of_expansion(&ExpandError::ContextProcessing(context_processing::Error::ContextLoadingFailed(load_error()))).is_some(),
            "a load failure nested in context processing is still a context error"
        );
        assert!(
            matches!(
                context_error_of_expansion(&ExpandError::ContextProcessing(located("speed", context_processing::Error::InvalidIriMapping))),
                Some(JsonLdError::ContextUnprocessable {
                    context: FaultyContext::Unnamed,
                    fault: ContextFault::TermDefinition {
                        reason: ErrorCode::InvalidIriMapping,
                        ..
                    },
                })
            ),
            "a refused term definition is a fault of the @context, located in the term"
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
        let refusals: [fn() -> context_processing::Error<LoaderError>; 3] = [
            || context_processing::Error::ContextOverflow,
            || context_processing::Error::RecursiveContextInclusion,
            || located("gear", context_processing::Error::ContextOverflow),
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
    fn context_error_of_compaction_types_every_context_failure_and_leaves_document_defects() {
        assert!(context_error_of_compaction(&CompactError::Loading(load_error())).is_some());
        assert!(context_error_of_compaction(&CompactError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load_error()))).is_some());
        assert!(names_the_unextractable_document(context_error_of_compaction(&CompactError::ContextLoading(
            unextractable()
        ))));
        assert!(matches!(
            context_error_of_compaction(&CompactError::Compaction(compaction::Error::ContextProcessing(
                context_processing::Error::InvalidBaseIri
            ))),
            Some(JsonLdError::ContextUnprocessable { .. })
        ));
        assert!(context_error_of_compaction(&CompactError::ContextProcessing(context_processing::Error::ContextLoadingFailed(load_error()))).is_some());
        assert!(
            context_error_of_compaction(&CompactError::Expand(ExpandError::Loading(load_error()))).is_some(),
            "compaction delegates its Expand arm to the expansion rule"
        );
        assert!(context_error_of_compaction(&CompactError::Compaction(compaction::Error::IriConfusedWithPrefix)).is_none());
    }

    #[test]
    fn context_error_of_processing_names_the_context_a_fault_is_in() {
        assert!(
            context_error_of_processing(&context_processing::Error::ContextLoadingFailed(load_error()), FaultyContext::Url(url())).is_context_not_available()
        );
        let fault = context_error_of_processing(
            &located("speed", context_processing::Error::ProtectedTermRedefinition),
            FaultyContext::Url(url()),
        );
        pretty_assertions::assert_eq!(
            fault.to_string(),
            r#"the @context https://example.com/ctx.jsonld cannot define the term "speed": protected term redefinition"#
        );
    }

    /// The empty key is a term to the scan, and it reads as `""`.
    #[test]
    fn a_scoped_user_context_names_its_term_in_quotes() {
        for (term, detail) in [
            (
                "speed",
                r#"the user @context defines the term "speed" with a scoped @context, which is not allowed"#,
            ),
            ("", r#"the user @context defines the term "" with a scoped @context, which is not allowed"#),
        ] {
            let error = JsonLdError::ScopedUserContext { term: ContextTerm::new(term) };
            pretty_assertions::assert_eq!(error.to_string(), detail);
        }
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
