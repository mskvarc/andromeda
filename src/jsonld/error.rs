//! Error types for JSON-LD processing.
//!
//! All fallible operations in this module return [`Error`].  The variants
//! cover context loading failures (network, size, parse), expansion /
//! compaction failures, IRI validation, document structure issues, and
//! JSON (de)serialisation.

use json_ld::{CompactError, ContextLoadError, ExpandError, LoadError};
use reqwest::StatusCode;
use std::error::Error as StdError;
use thiserror::Error;

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
    Expansion(#[source] ExpandError),

    /// JSON-LD compaction failed.
    #[error("Failed to compact document")]
    Compaction(#[source] CompactError),

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

impl From<ExpandError> for Error {
    fn from(e: ExpandError) -> Self {
        if let Some(context_error) = extract_context_error_from_chain(&e) {
            return context_error;
        }
        if let Some(context_error) = extract_context_error_from_expand_error(&e) {
            return context_error;
        }
        Self::Expansion(e)
    }
}

impl From<CompactError> for Error {
    fn from(e: CompactError) -> Self {
        if let Some(context_error) = extract_context_error_from_chain(&e) {
            return context_error;
        }
        if let Some(context_error) = extract_context_error_from_compact_error(&e) {
            return context_error;
        }
        Self::Compaction(e)
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
    pub fn is_explicit_context_load_error(&self) -> bool {
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
    pub fn is_context_not_available(&self) -> bool {
        self.is_explicit_context_load_error()
    }

    /// Returns `true` if the underlying [`CompactError`] was caused by a
    /// protected-term redefinition in context processing.
    ///
    /// This is used to decide whether a fallback to core-only compaction
    /// is appropriate.
    pub fn is_protected_term_error(e: &CompactError) -> bool {
        match e {
            CompactError::ContextProcessing(cpe) => is_protected_term_redefinition(cpe),
            CompactError::Expand(ExpandError::ContextProcessing(cpe)) => is_protected_term_redefinition(cpe),
            CompactError::Compaction(ce) => {
                if let json_ld::compaction::Error::ContextProcessing(cpe) = ce {
                    is_protected_term_redefinition(cpe)
                } else {
                    false
                }
            }
            CompactError::Loading(_) | CompactError::ContextLoading(_) | CompactError::Expand(_) => false,
        }
    }
}

/// Check whether a context-processing error is specifically a
/// protected-term redefinition.
fn is_protected_term_redefinition(cpe: &json_ld::context_processing::Error) -> bool {
    matches!(cpe, json_ld::context_processing::Error::ProtectedTermRedefinition)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Extract a context error by directly matching `ExpandError` variants.
///
/// `LoadError` from `json-ld-core` stores its inner error in `cause` without
/// a `#[source]` attribute, so `source()` returns `None` and the standard
/// error chain is broken.  `#[error(transparent)]` on `ExpandError::Loading`
/// delegates `source()` to `LoadError::source()`, which also returns `None`.
/// We must pattern-match the concrete variants and access `cause` directly.
fn extract_context_error_from_expand_error(e: &ExpandError) -> Option<Error> {
    match e {
        ExpandError::Loading(le) => extract_from_load_error_cause(le),
        ExpandError::ContextLoading(ContextLoadError::LoadingDocumentFailed(le)) => extract_from_load_error_cause(le),
        ExpandError::ContextLoading(ContextLoadError::ContextExtractionFailed(_)) => None,
        ExpandError::ContextProcessing(cpe) => extract_from_context_processing_error(cpe),
        ExpandError::Expansion(ee) => {
            if let json_ld::expansion::Error::ContextProcessing(cpe) = ee {
                extract_from_context_processing_error(cpe)
            } else {
                None
            }
        }
    }
}

/// Same as above but for `CompactError`.
fn extract_context_error_from_compact_error(e: &CompactError) -> Option<Error> {
    match e {
        CompactError::Loading(le) => extract_from_load_error_cause(le),
        CompactError::ContextLoading(ContextLoadError::LoadingDocumentFailed(le)) => extract_from_load_error_cause(le),
        CompactError::ContextLoading(ContextLoadError::ContextExtractionFailed(_)) => None,
        CompactError::ContextProcessing(cpe) => extract_from_context_processing_error(cpe),
        CompactError::Expand(ee) => extract_context_error_from_expand_error(ee),
        CompactError::Compaction(_) => None,
    }
}

fn extract_from_context_processing_error(cpe: &json_ld::context_processing::Error) -> Option<Error> {
    if let json_ld::context_processing::Error::ContextLoadingFailed(le) = cpe {
        extract_from_load_error_cause(le)
    } else {
        None
    }
}

fn extract_from_load_error_cause(le: &LoadError) -> Option<Error> {
    le.cause.downcast_ref::<Error>().and_then(explicit_context_error_copy)
}

fn extract_context_error_from_chain(err: &(dyn StdError + 'static)) -> Option<Error> {
    let mut current: Option<&(dyn StdError + 'static)> = Some(err);

    while let Some(source) = current {
        if let Some(jsonld_error) = source.downcast_ref::<Error>()
            && let Some(explicit) = explicit_context_error_copy(jsonld_error)
        {
            return Some(explicit);
        }

        // LoadError (from json-ld-core) stores its inner error in a `cause`
        // field without a #[source] attribute, so source() returns None and
        // the chain breaks.  Bridge the gap by accessing `cause` directly.
        if let Some(load_error) = source.downcast_ref::<LoadError>()
            && let Some(inner) = load_error.cause.downcast_ref::<Error>()
            && let Some(explicit) = explicit_context_error_copy(inner)
        {
            return Some(explicit);
        }

        current = source.source();
    }

    None
}

fn explicit_context_error_copy(err: &Error) -> Option<Error> {
    match err {
        Error::ContextFetchDisabled { url } => Some(Error::ContextFetchDisabled { url: url.clone() }),
        Error::ContextLoad { url, message } => Some(Error::ContextLoad {
            url: url.clone(),
            message: message.clone(),
        }),
        Error::ContextHttp { url, status } => Some(Error::ContextHttp {
            url: url.clone(),
            status: *status,
        }),
        Error::ContextTooLarge { url, size, max } => Some(Error::ContextTooLarge {
            url: url.clone(),
            size: *size,
            max: *max,
        }),
        Error::ContextInvalid { url, message } => Some(Error::ContextInvalid {
            url: url.clone(),
            message: message.clone(),
        }),
        Error::EmbeddedContextInvalid(_) | Error::Expansion(_) | Error::Compaction(_) | Error::InvalidIri(_) | Error::InvalidDocument(_) | Error::Json(_) => {
            None
        }
    }
}
