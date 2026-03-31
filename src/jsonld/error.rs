//! Error types for JSON-LD processing.
//!
//! All fallible operations in this crate return [`Error`].

use json_ld::{CompactError, ContextLoadError, ExpandError, LoadError};
use reqwest::StatusCode;
use std::error::Error as StdError;
use thiserror::Error;

/// JSON-LD processing error.
#[derive(Debug, Error)]
pub enum Error {
    /// Remote context fetch is disabled by configuration.
    #[error("Remote context fetch disabled for {url}")]
    ContextFetchDisabled { url: String },

    /// Failed to load a remote JSON-LD context.
    #[error("Failed to load context from {url}: {message}")]
    ContextLoad { url: String, message: String },

    /// HTTP request returned a non-success status code.
    #[error("HTTP error {status} for {url}")]
    ContextHttp { url: String, status: StatusCode },

    /// Remote context exceeded the configured size limit.
    #[error("Context {url} too large ({size} > {max} bytes)")]
    ContextTooLarge { url: String, size: usize, max: usize },

    /// Remote context could not be parsed as valid JSON-LD.
    #[error("Invalid JSON-LD context at {url}: {message}")]
    ContextInvalid { url: String, message: String },

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
        ExpandError::ContextProcessing(cpe) => extract_from_context_processing_error(cpe),
        ExpandError::Expansion(ee) => {
            if let json_ld::expansion::Error::ContextProcessing(cpe) = ee {
                extract_from_context_processing_error(cpe)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Same as above but for `CompactError`.
fn extract_context_error_from_compact_error(e: &CompactError) -> Option<Error> {
    match e {
        CompactError::Loading(le) => extract_from_load_error_cause(le),
        CompactError::ContextLoading(ContextLoadError::LoadingDocumentFailed(le)) => extract_from_load_error_cause(le),
        CompactError::ContextProcessing(cpe) => extract_from_context_processing_error(cpe),
        CompactError::Expand(ee) => extract_context_error_from_expand_error(ee),
        _ => None,
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
        _ => None,
    }
}
