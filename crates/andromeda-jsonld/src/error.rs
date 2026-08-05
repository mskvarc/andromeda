//! What can go wrong while resolving a context or reshaping a document.
//!
//! Every fallible operation in this crate answers with [`JsonLdError`]. The
//! variants cover context loading (fetch disabled, HTTP status, size, parse),
//! expansion and compaction, IRI validation, document structure, and JSON
//! (de)serialisation. Nothing here decides what a client is told about the
//! failure: clause 5.5.2 categorisation belongs to the caller, which is why
//! the loading variants carry the URL, the status and the reason separately
//! rather than pre-rendered into one message.

use crate::loader::error::LoaderError;
use jsonld::{CompactError, ContextLoadError, ExpandError, compaction, context_processing, expansion};
use reqwest::StatusCode;
use thiserror::Error;

/// The typed [`ExpandError`] the processor yields when driven by this crate's
/// loader.
pub type ExpandErrorTyped = ExpandError<LoaderError>;

/// The typed [`CompactError`] the processor yields when driven by this crate's
/// loader.
pub type CompactErrorTyped = CompactError<LoaderError>;

/// A JSON-LD processing failure.
///
/// Carries enough detail for a caller to produce an NGSI-LD Problem Detail
/// without inspecting error chains.
#[derive(Debug, Error)]
pub enum JsonLdError {
    /// Remote context fetch is disabled by configuration.
    #[error("Remote context fetch disabled for {url}")]
    ContextFetchDisabled {
        /// The URL that was requested.
        url: String,
    },

    /// A remote JSON-LD context could not be loaded.
    #[error("Failed to load context from {url}: {message}")]
    ContextLoad {
        /// The URL that was requested.
        url: String,
        /// Human-readable failure reason.
        message: String,
    },

    /// The HTTP request returned a non-success status code.
    #[error("HTTP error {status} for {url}")]
    ContextHttp {
        /// The URL that was requested.
        url: String,
        /// HTTP status code returned by the server.
        status: StatusCode,
    },

    /// The remote context exceeded the configured size limit.
    #[error("Context {url} too large ({size} > {max} bytes)")]
    ContextTooLarge {
        /// The URL that was requested.
        url: String,
        /// Actual payload size in bytes.
        size: usize,
        /// Configured maximum in bytes.
        max: usize,
    },

    /// The remote context could not be parsed as valid JSON-LD.
    #[error("Invalid JSON-LD context at {url}: {message}")]
    ContextInvalid {
        /// The URL that was requested.
        url: String,
        /// Parse / validation error message.
        message: String,
    },

    /// An embedded (inline) context could not be parsed.
    #[error("Embedded context invalid: {0}")]
    EmbeddedContextInvalid(String),

    /// JSON-LD expansion failed.
    #[error("Failed to expand document")]
    Expansion(#[source] Box<ExpandErrorTyped>),

    /// JSON-LD compaction failed.
    #[error("Failed to compact document")]
    Compaction(#[source] Box<CompactErrorTyped>),

    /// An IRI could not be parsed or resolved.
    #[error("Invalid IRI: {0}")]
    InvalidIri(String),

    /// The document structure is not a valid JSON-LD input.
    #[error("Invalid document: {0}")]
    InvalidDocument(String),

    /// JSON serialisation / deserialisation error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl From<ExpandErrorTyped> for JsonLdError {
    fn from(error: ExpandErrorTyped) -> JsonLdError {
        match context_error_of_expansion(&error) {
            Some(context_error) => context_error,
            None => JsonLdError::Expansion(Box::new(error)),
        }
    }
}

impl From<CompactErrorTyped> for JsonLdError {
    fn from(error: CompactErrorTyped) -> JsonLdError {
        match context_error_of_compaction(&error) {
            Some(context_error) => context_error,
            None => JsonLdError::Compaction(Box::new(error)),
        }
    }
}

impl JsonLdError {
    /// Build an [`JsonLdError::InvalidIri`].
    pub fn invalid_iri(message: impl Into<String>) -> JsonLdError {
        JsonLdError::InvalidIri(message.into())
    }

    /// Build an [`JsonLdError::InvalidDocument`].
    pub fn invalid_document(message: impl Into<String>) -> JsonLdError {
        JsonLdError::InvalidDocument(message.into())
    }

    /// Whether this is an explicit context-loading failure.
    ///
    /// These variants carry the URL, status and reason a clause 5.5.2
    /// `LdContextNotAvailable` Problem Detail needs, so a caller can surface
    /// them directly instead of reporting an internal error.
    pub const fn is_context_not_available(&self) -> bool {
        matches!(
            self,
            JsonLdError::ContextFetchDisabled { .. }
                | JsonLdError::ContextHttp { .. }
                | JsonLdError::ContextLoad { .. }
                | JsonLdError::ContextTooLarge { .. }
                | JsonLdError::ContextInvalid { .. }
        )
    }

    /// Whether a [`CompactError`] was caused by a protected-term redefinition.
    ///
    /// Decides whether falling back to core-only compaction is appropriate:
    /// a user context that redefines a protected core term is the one failure
    /// a retry without that context can fix.
    pub const fn is_protected_term_error(error: &CompactErrorTyped) -> bool {
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
            CompactError::Loading(_)
            | CompactError::ContextLoading(_)
            | CompactError::Expand(ExpandError::Loading(_))
            | CompactError::Expand(ExpandError::ContextLoading(_)) => false,
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
fn context_error_of_expansion(error: &ExpandErrorTyped) -> Option<JsonLdError> {
    match error {
        ExpandError::Loading(load) => Some(JsonLdError::from(&load.source)),
        ExpandError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load)) => Some(JsonLdError::from(&load.source)),
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
fn context_error_of_compaction(error: &CompactErrorTyped) -> Option<JsonLdError> {
    match error {
        CompactError::Loading(load) => Some(JsonLdError::from(&load.source)),
        CompactError::ContextLoading(ContextLoadError::LoadingDocumentFailed(load)) => Some(JsonLdError::from(&load.source)),
        CompactError::ContextLoading(ContextLoadError::ContextExtractionFailed(_)) => None,
        CompactError::ContextProcessing(processing) => context_error_of_processing(processing),
        CompactError::Expand(expand) => context_error_of_expansion(expand),
        CompactError::Compaction(_) => None,
    }
}

/// The loader failure a context-processing error wraps, if it wraps one.
///
/// Written as an `if let` rather than an exhaustive match because
/// `context_processing::Error` is a foreign enum of twenty-odd grammar
/// violations, none of which carries a loader failure, and enumerating them
/// here would only restate that list.
fn context_error_of_processing(error: &context_processing::Error<LoaderError>) -> Option<JsonLdError> {
    if let context_processing::Error::ContextLoadingFailed(load) = error {
        Some(JsonLdError::from(&load.source))
    } else {
        None
    }
}
