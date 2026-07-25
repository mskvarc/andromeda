//! Error types for JSON-LD processing.
//!
//! All fallible operations in this module return [`Error`].  The variants
//! cover context loading failures (network, size, parse), expansion /
//! compaction failures, IRI validation, document structure issues, and
//! JSON (de)serialisation.

use crate::jsonld::loader::LoaderError;
use jsonld::{CompactError, ContextLoadError, ExpandError};
use reqwest::StatusCode;
use thiserror::Error;

/// Alias for the typed [`ExpandError`] yielded by the fork's processor when
/// using andromeda's loader.
pub type ExpandErrorTyped = ExpandError<LoaderError>;

/// Alias for the typed [`CompactError`] yielded by the fork's processor when
/// using andromeda's loader.
pub type CompactErrorTyped = CompactError<LoaderError>;

/// JSON-LD processing error.
///
/// Carries enough context for callers to produce NGSI-LD Problem Detail
/// responses without inspecting error chains.
#[derive(Debug, Error)]
pub enum Error {
    /// Remote context fetch is disabled by configuration.
    #[error("Remote context fetch disabled for {url}")]
    ContextFetchDisabled {
        /// The URL that was requested.
        url: String,
    },

    /// Failed to load a remote JSON-LD context.
    #[error("Failed to load context from {url}: {message}")]
    ContextLoad {
        /// The URL that was requested.
        url: String,
        /// Human-readable failure reason.
        message: String,
    },

    /// HTTP request returned a non-success status code.
    #[error("HTTP error {status} for {url}")]
    ContextHttp {
        /// The URL that was requested.
        url: String,
        /// HTTP status code returned by the server.
        status: StatusCode,
    },

    /// Remote context exceeded the configured size limit.
    #[error("Context {url} too large ({size} > {max} bytes)")]
    ContextTooLarge {
        /// The URL that was requested.
        url: String,
        /// Actual payload size in bytes.
        size: usize,
        /// Configured maximum in bytes.
        max: usize,
    },

    /// Remote context could not be parsed as valid JSON-LD.
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

impl From<ExpandErrorTyped> for Error {
    fn from(e: ExpandErrorTyped) -> Self {
        if let Some(context_error) = extract_context_error_from_expand_error(&e) {
            return context_error;
        }
        Self::Expansion(Box::new(e))
    }
}

impl From<CompactErrorTyped> for Error {
    fn from(e: CompactErrorTyped) -> Self {
        if let Some(context_error) = extract_context_error_from_compact_error(&e) {
            return context_error;
        }
        Self::Compaction(Box::new(e))
    }
}

impl Error {
    /// Create an [`Error::InvalidIri`].
    pub fn invalid_iri(msg: impl Into<String>) -> Self {
        Self::InvalidIri(msg.into())
    }

    /// Create an [`Error::InvalidDocument`].
    pub fn invalid_document(msg: impl Into<String>) -> Self {
        Self::InvalidDocument(msg.into())
    }

    /// Returns `true` when the error is an explicit context-loading failure.
    ///
    /// These variants carry enough detail for a Problem Detail response
    /// (URL, status code, message) and should be surfaced directly.
    pub const fn is_explicit_context_load_error(&self) -> bool {
        matches!(
            self,
            Self::ContextFetchDisabled { .. }
                | Self::ContextHttp { .. }
                | Self::ContextLoad { .. }
                | Self::ContextTooLarge { .. }
                | Self::ContextInvalid { .. }
        )
    }

    /// Returns `true` when the error indicates a context is unavailable.
    pub const fn is_context_not_available(&self) -> bool {
        self.is_explicit_context_load_error()
    }

    /// Returns `true` if the underlying [`CompactError`] was caused by a
    /// protected-term redefinition in context processing.
    ///
    /// This is used to decide whether a fallback to core-only compaction
    /// is appropriate.
    pub const fn is_protected_term_error(e: &CompactErrorTyped) -> bool {
        match e {
            CompactError::ContextProcessing(cpe) => is_protected_term_redefinition(cpe),
            CompactError::Expand(ExpandError::ContextProcessing(cpe)) => is_protected_term_redefinition(cpe),
            CompactError::Compaction(ce) => {
                if let jsonld::compaction::Error::ContextProcessing(cpe) = ce {
                    is_protected_term_redefinition(cpe)
                } else {
                    false
                }
            }
            CompactError::Expand(ExpandError::Expansion(ee)) => {
                if let jsonld::expansion::Error::ContextProcessing(cpe) = ee {
                    is_protected_term_redefinition(cpe)
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

/// Check whether a context-processing error is specifically a
/// protected-term redefinition.
const fn is_protected_term_redefinition(cpe: &jsonld::context_processing::Error<LoaderError>) -> bool {
    matches!(cpe, jsonld::context_processing::Error::ProtectedTermRedefinition)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract a context-loader error from a typed [`ExpandError`].
///
/// With the fork's typed [`LoadError<LoaderError>`], the loader error is
/// reachable directly via `LoadError::source` — no `Box<dyn Error>` downcasts
/// required.
fn extract_context_error_from_expand_error(e: &ExpandErrorTyped) -> Option<Error> {
    match e {
        ExpandError::Loading(le) => Some(Error::from_loader_error(&le.source)),
        ExpandError::ContextLoading(ContextLoadError::LoadingDocumentFailed(le)) => Some(Error::from_loader_error(&le.source)),
        ExpandError::ContextLoading(ContextLoadError::ContextExtractionFailed(_)) => None,
        ExpandError::ContextProcessing(cpe) => extract_from_context_processing_error(cpe),
        ExpandError::Expansion(ee) => {
            if let jsonld::expansion::Error::ContextProcessing(cpe) = ee {
                extract_from_context_processing_error(cpe)
            } else {
                None
            }
        }
    }
}

/// Same as above but for the typed [`CompactError`].
fn extract_context_error_from_compact_error(e: &CompactErrorTyped) -> Option<Error> {
    match e {
        CompactError::Loading(le) => Some(Error::from_loader_error(&le.source)),
        CompactError::ContextLoading(ContextLoadError::LoadingDocumentFailed(le)) => Some(Error::from_loader_error(&le.source)),
        CompactError::ContextLoading(ContextLoadError::ContextExtractionFailed(_)) => None,
        CompactError::ContextProcessing(cpe) => extract_from_context_processing_error(cpe),
        CompactError::Expand(ee) => extract_context_error_from_expand_error(ee),
        CompactError::Compaction(_) => None,
    }
}

fn extract_from_context_processing_error(cpe: &jsonld::context_processing::Error<LoaderError>) -> Option<Error> {
    if let jsonld::context_processing::Error::ContextLoadingFailed(le) = cpe {
        Some(Error::from_loader_error(&le.source))
    } else {
        None
    }
}
