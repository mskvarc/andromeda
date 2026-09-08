//! Why a context URL did not resolve to a document.
//!
//! [`LoaderError`] is the error the JSON-LD processor sees, because
//! [`jsonld::Loader`] fixes the associated type. [`JsonLdError`] is the error
//! the rest of the crate sees. The conversion between them is the whole of
//! this module: it exists so a loading failure keeps its URL, its status and
//! its reason as separate fields all the way to whoever renders the Problem
//! Detail, instead of being flattened into one message at the boundary.

use crate::error::JsonLdError;
use iri_rs::IriBuf;
use reqwest::StatusCode;
use thiserror::Error;

/// A failure of the context-resolution pipeline, as
/// [`jsonld::Loader::Error`](jsonld::Loader) requires it.
#[derive(Debug, Clone, Error)]
#[error("{kind} for {url}")]
pub struct LoaderError {
    /// The URL being resolved when the failure occurred.
    pub url: IriBuf,
    /// What went wrong.
    pub kind: LoaderErrorKind,
}

/// The failures [`LoaderError`] classifies.
#[derive(Debug, Clone, Error)]
pub enum LoaderErrorKind {
    /// Remote context fetch is disabled by configuration.
    #[error("Remote context fetch disabled")]
    FetchDisabled,
    /// The HTTP request returned a non-success status code.
    #[error("HTTP {0}")]
    Http(StatusCode),
    /// Network or transport-level request failure.
    #[error("Request failed: {0}")]
    Request(String),
    /// The attached durable cache failed while remote loading was unavailable.
    #[error("Context cache failed: {0}")]
    Cache(String),
    /// The response body exceeds the configured size limit.
    #[error("Context too large ({size} > {max} bytes)")]
    TooLarge {
        /// Actual payload size in bytes.
        size: usize,
        /// Configured maximum in bytes.
        max: usize,
    },
    /// The response body could not be parsed as valid JSON-LD.
    #[error("Invalid context: {0}")]
    Invalid(String),
}

impl LoaderError {
    /// The failure a loader reports when configuration forbids the fetch.
    pub(crate) const fn fetch_disabled(url: IriBuf) -> LoaderError {
        LoaderError {
            url,
            kind: LoaderErrorKind::FetchDisabled,
        }
    }
}

// Borrowed rather than owned: the processor hands back a `&LoaderError` out of
// a `LoadError` it still owns, and every field this conversion needs is either
// `Copy` or cloned regardless.
impl From<&LoaderError> for JsonLdError {
    fn from(error: &LoaderError) -> JsonLdError {
        let url = error.url.clone(); // clone: the caller keeps the LoaderError
        match &error.kind {
            LoaderErrorKind::FetchDisabled => JsonLdError::ContextFetchDisabled { url },
            LoaderErrorKind::Http(status) => JsonLdError::ContextHttp { url, status: *status },
            LoaderErrorKind::Request(message) | LoaderErrorKind::Cache(message) => JsonLdError::ContextLoad {
                url,
                message: message.clone(), // clone: the caller keeps the LoaderError
            },
            LoaderErrorKind::TooLarge { size, max } => JsonLdError::ContextTooLarge { url, size: *size, max: *max },
            LoaderErrorKind::Invalid(message) => JsonLdError::ContextInvalid {
                url,
                message: message.clone(), // clone: the caller keeps the LoaderError
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::JsonLdError,
        loader::error::{LoaderError, LoaderErrorKind},
    };
    use iri_rs::IriBuf;
    use reqwest::StatusCode;

    #[test]
    fn unavailable_loader_failures_reach_the_caller_as_context_not_available() {
        let kinds = [
            LoaderErrorKind::FetchDisabled,
            LoaderErrorKind::Http(StatusCode::NOT_FOUND),
            LoaderErrorKind::Request("connection refused".to_string()),
            LoaderErrorKind::Cache("database unavailable".to_string()),
            LoaderErrorKind::TooLarge { size: 10, max: 5 },
        ];
        for kind in kinds {
            let error = JsonLdError::from(&LoaderError {
                url: IriBuf::new("https://example.com/context.jsonld".to_string()).expect("valid URL"),
                kind,
            });
            assert!(
                error.is_context_not_available(),
                "a loading failure must stay reportable as LdContextNotAvailable: {error}"
            );
        }
        let invalid = JsonLdError::from(&LoaderError {
            url: IriBuf::new("https://example.com/context.jsonld".to_string()).expect("valid URL"),
            kind: LoaderErrorKind::Invalid("missing @context".to_string()),
        });
        assert!(!invalid.is_context_not_available(), "a retrieved invalid document is BadRequestData");
    }

    #[test]
    fn the_url_and_the_status_survive_the_conversion_as_separate_fields() {
        let error = JsonLdError::from(&LoaderError {
            url: IriBuf::new("https://example.com/gone.jsonld".to_string()).expect("valid URL"),
            kind: LoaderErrorKind::Http(StatusCode::GONE),
        });
        match error {
            JsonLdError::ContextHttp { url, status } => {
                pretty_assertions::assert_eq!(url.as_str(), "https://example.com/gone.jsonld");
                pretty_assertions::assert_eq!(status, StatusCode::GONE);
            }
            other @ (JsonLdError::ProtectedContextPreload { .. }
            | JsonLdError::ContextFetchDisabled { .. }
            | JsonLdError::ContextLoad { .. }
            | JsonLdError::ContextTooLarge { .. }
            | JsonLdError::ContextInvalid { .. }
            | JsonLdError::EmbeddedContextInvalid(_)
            | JsonLdError::InlineContextInvalid(_)
            | JsonLdError::ClientBuild(_)
            | JsonLdError::Expansion(_)
            | JsonLdError::Compaction(_)
            | JsonLdError::InvalidIri(_)
            | JsonLdError::InvalidDocument(_)
            | JsonLdError::Json(_)) => panic!("expected ContextHttp, got {other}"),
        }
    }
}
