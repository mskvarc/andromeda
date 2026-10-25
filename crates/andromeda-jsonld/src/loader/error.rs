//! Why a context URL did not resolve to a document.
//!
//! [`LoaderError`] is the error the JSON-LD processor sees, because
//! [`jsonld::Loader`] fixes the associated type. [`JsonLdError`] is the error
//! the rest of the crate sees. The conversion between them is the whole of
//! this module: it exists so a loading failure keeps its URL, its status and
//! its reason as separate fields all the way to whoever renders the Problem
//! Detail, instead of being flattened into one message at the boundary.

use crate::{context_cache::ContextCacheError, context_subtree::UnusableLocalContext, error::JsonLdError};
use andromeda_json_literal::admission::JsonLiteralError;
use iri_rs::{InvalidIri, IriBuf};
use mediatype::{MediaTypeBuf, MediaTypeError};
use reqwest::StatusCode;
use std::{str::Utf8Error, sync::Arc};
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
    #[error("remote context fetch is disabled")]
    FetchDisabled,
    /// The HTTP request returned a non-success status code.
    #[error("upstream returned HTTP {0}")]
    Http(StatusCode),
    /// The document could not be reached at all.
    #[error(transparent)]
    Unavailable(ContextUnavailable),
    /// The response body exceeds the configured size limit.
    #[error("context too large ({size} bytes exceeds the {max}-byte limit)")]
    TooLarge {
        /// Actual payload size in bytes.
        size: usize,
        /// Configured maximum in bytes.
        max: usize,
    },
    /// The response body could not be parsed as valid JSON-LD.
    #[error("invalid context: {0}")]
    Invalid(#[source] InvalidContextDocument),
    /// A reference was recognised as one of the tenant's own stored
    /// `@context`s, but its identifier is not an IRI to resolve it by.
    ///
    /// The lexicon proves every `urn:ngsi-ld:JsonLdContext:` identifier is an
    /// IRI, so this reports a broken invariant, never a client mistake. Shared
    /// for the same reason as [`ContextUnavailable::Request`].
    #[error("the stored context identifier is not an IRI: {0}")]
    StoredIdentifier(#[source] Arc<InvalidIri<String>>),
}

/// Why a context document could not be reached.
#[derive(Debug, Clone, Error)]
pub enum ContextUnavailable {
    /// Network or transport-level request failure.
    ///
    /// Shared rather than owned because the processor reports a loader failure
    /// by reference, and the conversion into [`JsonLdError`] keeps the source.
    #[error("request failed: {0}")]
    Request(#[source] Arc<reqwest::Error>),
    /// The attached durable cache failed while remote loading was unavailable.
    #[error("context cache failed: {0}")]
    Cache(#[source] ContextCacheError),
    /// The response is served as a media type no context is read from, and
    /// names no single JSON-LD alternate to read it from instead.
    ///
    /// Boxed so the rare refusal does not widen every loader failure.
    #[error("the context is served as {0}, not JSON, and names no JSON-LD alternate")]
    UnsupportedMediaType(Box<MediaTypeBuf>),
    /// The response declared a content type that is not a media type.
    #[error("the context response content type is not a media type: {0}")]
    MalformedMediaType(#[source] MediaTypeError),
    /// The response names more than one JSON-LD alternate, so which one is the
    /// context is not stated.
    #[error("the context response names {0} JSON-LD alternates")]
    AmbiguousAlternates(usize),
    /// The URL the document was finally read from is not an IRI.
    ///
    /// Shared for the same reason as [`ContextUnavailable::Request`].
    #[error("the context was read from a location that is not an IRI: {0}")]
    UnaddressableDocument(#[source] Arc<InvalidIri<String>>),
}

/// Why a retrieved JSON-shaped response is not a usable `@context` document.
///
/// A response whose media type no context is read from is not here: clause
/// 5.5.6 reports an invalid remote context as `BadRequestData`, but such a
/// response yielded no document at all, which is
/// [`ContextUnavailable::UnsupportedMediaType`] and `LdContextNotAvailable`.
#[derive(Debug, Clone, Error)]
pub enum InvalidContextDocument {
    /// The response body is not UTF-8.
    #[error("the response body is not UTF-8: {0}")]
    NotUtf8(#[source] Utf8Error),
    /// The response body is not JSON the broker admits: malformed, naming a
    /// member twice in one object, or nesting too deep.
    ///
    /// Shared for the same reason as [`ContextUnavailable::Request`].
    #[error("the response body is not JSON: {0}")]
    NotJson(#[source] Arc<JsonLiteralError>),
    /// The document carries no usable top-level `@context`.
    #[error("the document has no usable top-level @context: {0}")]
    NoContext(#[source] UnusableLocalContext),
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
            LoaderErrorKind::Unavailable(source) => JsonLdError::ContextLoad {
                url,
                source: source.clone(), // clone: the caller keeps the LoaderError; the sources are shared or small
            },
            LoaderErrorKind::TooLarge { size, max } => JsonLdError::ContextTooLarge { url, size: *size, max: *max },
            LoaderErrorKind::Invalid(source) => JsonLdError::ContextInvalid {
                url,
                source: source.clone(), // clone: the caller keeps the LoaderError; the sources are shared or small
            },
            LoaderErrorKind::StoredIdentifier(source) => JsonLdError::ContextUrlNotIri(InvalidIri(source.0.clone())), // clone: the caller keeps the LoaderError
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        context_cache::ContextCacheError,
        context_subtree::UnusableLocalContext,
        error::JsonLdError,
        loader::error::{ContextUnavailable, InvalidContextDocument, LoaderError, LoaderErrorKind},
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use iri_rs::IriBuf;
    use reqwest::StatusCode;

    #[test]
    fn unavailable_loader_failures_reach_the_caller_as_context_not_available() {
        let kinds = [
            LoaderErrorKind::FetchDisabled,
            LoaderErrorKind::Http(StatusCode::NOT_FOUND),
            LoaderErrorKind::Unavailable(ContextUnavailable::Cache(ContextCacheError::Read(PersistenceFailure::Unavailable))),
            LoaderErrorKind::TooLarge { size: 10, max: 5 },
            LoaderErrorKind::Unavailable(ContextUnavailable::UnsupportedMediaType(Box::new("text/html".parse().expect("a media type")))),
            LoaderErrorKind::Unavailable(ContextUnavailable::AmbiguousAlternates(2)),
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
            kind: LoaderErrorKind::Invalid(InvalidContextDocument::NoContext(UnusableLocalContext::NoContextMember)),
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
            | JsonLdError::PreloadedContextUnreadable { .. }
            | JsonLdError::PreloadedContextInvalid { .. }
            | JsonLdError::PreloadedContextUrl(_)
            | JsonLdError::EmbeddedContextMissing { .. }
            | JsonLdError::EmbeddedContextInvalid { .. }
            | JsonLdError::InlineContextInvalid(_)
            | JsonLdError::ClientBuild(_)
            | JsonLdError::Expansion(_)
            | JsonLdError::Compaction(_)
            | JsonLdError::TypeNotExpanded
            | JsonLdError::TermNotExpanded
            | JsonLdError::ExpandedTermNotIri(_)
            | JsonLdError::ContextUrlNotIri(_)
            | JsonLdError::ContextGraph(_)
            | JsonLdError::ScopedUserContext { .. }
            | JsonLdError::EmbeddedUserContext(_)
            | JsonLdError::Json(_)
            | JsonLdError::ContextText(_)
            | JsonLdError::ProcessorText(_)) => panic!("expected ContextHttp, got {other}"),
        }
    }
}
