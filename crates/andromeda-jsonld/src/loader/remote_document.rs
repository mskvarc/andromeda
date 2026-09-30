//! Downloading and parsing one remote `@context` document.
//!
//! Everything a response must satisfy before its body becomes a context lives
//! here: the media type has to be JSON-shaped, the body has to fit the
//! configured limit, it has to parse, and it has to carry a usable top-level
//! `@context`. The response's cache directives are read on the way past, since
//! clause 6.3.16 makes them part of what was fetched rather than an afterthought
//! the cache tiers each re-derive.
//!
//! What leaves here is the document's `@context` subtree and nothing else.
//! Clause 5.13.2.3 requires that "all extra information located outside of the
//! @context subtree in the referenced object shall be discarded", and a
//! downloaded document is exactly such a referenced object, so the reduction
//! happens where the response becomes a document rather than at each tier that
//! later stores or serves it.

use crate::{
    context_subtree::context_subtree,
    loader::{
        cache_lifetime::{CacheLifetime, ContextFreshness, response_freshness},
        error::{ContextUnavailable, InvalidContextDocument, LoaderError, LoaderErrorKind},
    },
};
use andromeda_http_headers::json_ld_accept::JsonLdAccept;
use backon::{ConstantBuilder, Retryable};
use headers::HeaderMapExt;
use iri_rs::IriBuf;
use mediatype::MediaTypeBuf;
use reqwest::{Client, header::HeaderMap};
use serde_json::Value;
use std::{str::from_utf8, sync::Arc, time::Duration};
use tracing::warn;

/// A parsed remote context and the freshness its response granted it.
#[derive(Debug)]
pub(crate) struct RemoteContextDocument {
    /// Parsed JSON-LD document.
    pub(crate) value: jsonld_syntax::Value,
    /// What the response permits a cache to do with the document.
    pub(crate) freshness: ContextFreshness,
}

/// The media type a loaded remote document reports.
pub(crate) fn ld_json_media_type() -> MediaTypeBuf {
    jsonld_core::LD_JSON_MEDIA_TYPE.into()
}

/// Fetch a context over HTTP and parse it, refusing anything that is not
/// JSON-shaped or is larger than the configured limit.
///
/// `default_lifetime` is the heuristic expiration time clause 6.3.16 recommends
/// assigning when the origin server states none.
///
/// # Errors
///
/// Returns the [`LoaderError`] naming the step that refused the response: the
/// request itself, a non-success status, an unusable content type, a body past
/// `max_bytes`, or a document that will not parse as a context.
pub(crate) async fn fetch_and_parse(
    client: &Client,
    url: &IriBuf,
    max_bytes: usize,
    default_lifetime: CacheLifetime,
) -> Result<RemoteContextDocument, LoaderError> {
    let mut request_headers = HeaderMap::new();
    request_headers.typed_insert(JsonLdAccept);
    let response = client.get(url.as_str()).headers(request_headers).send().await.map_err(|error| LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Unavailable(ContextUnavailable::Request(Arc::new(error))),
    })?;

    if !response.status().is_success() {
        return Err(LoaderError {
            url: url.clone(), // clone: the failure is reported under the URL that was requested
            kind: LoaderErrorKind::Http(response.status()),
        });
    }

    if let Some(refusal) = response
        .headers()
        .typed_get::<headers::ContentType>()
        .and_then(|content_type| content_type_refusal(&content_type))
    {
        return Err(LoaderError {
            url: url.clone(), // clone: the failure is reported under the URL that was requested
            kind: LoaderErrorKind::Invalid(refusal),
        });
    }

    // Checked twice on purpose: the advertised length rejects an oversized
    // body before it is read, and the measured length catches a server that
    // did not advertise one.
    if let Some(advertised) = response.content_length() {
        // An advertised length wider than the address space saturates: such a
        // body could not be held in memory, so `usize::MAX` refuses it under
        // every configured limit.
        let advertised_bytes = usize::try_from(advertised).unwrap_or(usize::MAX);
        if advertised_bytes > max_bytes {
            return Err(too_large(url.clone(), advertised_bytes, max_bytes)); // clone: the failure outlives the borrowed URL
        }
    }

    let freshness = response_freshness(response.headers(), default_lifetime);
    let bytes = response.bytes().await.map_err(|error| LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Unavailable(ContextUnavailable::Request(Arc::new(error))),
    })?;

    if bytes.len() > max_bytes {
        return Err(too_large(url.clone(), bytes.len(), max_bytes)); // clone: the failure outlives the borrowed URL
    }

    let text = from_utf8(&bytes).map_err(|error| LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Invalid(InvalidContextDocument::NotUtf8(error)),
    })?;

    let json = serde_json::from_str::<Value>(text).map_err(|error| LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Invalid(InvalidContextDocument::NotJson(Arc::new(error))),
    })?;
    let subtree = context_subtree(json).ok_or_else(|| LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Invalid(InvalidContextDocument::NoContext),
    })?;
    Ok(RemoteContextDocument {
        value: jsonld_syntax::Value::from_serde_json(subtree),
        freshness,
    })
}

/// How many extra attempts a transport-level fetch failure is given after the
/// first, before it is reported. A context host that drops or refuses a
/// connection under load recovers within a few hundred milliseconds; only a
/// genuinely unreachable source exhausts all attempts.
const MAX_TRANSPORT_RETRIES: usize = 2;
/// How long to wait between transport-level retries.
const TRANSPORT_RETRY_BACKOFF: Duration = Duration::from_millis(250);

/// [`fetch_and_parse`], retrying only a transport-level failure.
///
/// A dropped or refused connection ([`ContextUnavailable::Request`]) is transient
/// and worth another attempt; every other failure is deterministic -- an HTTP
/// status, an oversized body, or an unparseable document fails identically on a
/// retry -- and is returned at once, filtered by
/// [`is_transient_transport_failure`]. Retrying matters most under
/// `--isolate-tenants`, where the per-tenant context cache makes many fresh
/// tenants cold-fetch the same public context at once and a host occasionally
/// resets one connection, but the same transience arises for any deployment
/// fetching a remote `@context`.
pub(crate) async fn fetch_and_parse_retrying(
    client: &Client,
    url: &IriBuf,
    max_bytes: usize,
    default_lifetime: CacheLifetime,
) -> Result<RemoteContextDocument, LoaderError> {
    let backoff = ConstantBuilder::default()
        .with_delay(TRANSPORT_RETRY_BACKOFF)
        .with_max_times(MAX_TRANSPORT_RETRIES);
    (|| fetch_and_parse(client, url, max_bytes, default_lifetime))
        .retry(backoff)
        .when(is_transient_transport_failure)
        .notify(
            |error: &LoaderError, delay| warn!(url = %error.url, error = %error.kind, ?delay, "Remote context fetch failed at the transport level; retrying"),
        )
        .await
}

/// Whether a loader failure is a transient transport error worth retrying.
///
/// Only a network- or transport-level request failure recovers on a retry.
/// A non-success HTTP status, an oversized body, an unparseable document, a
/// disabled fetch, or a degraded durable cache are all deterministic given the
/// same request, so retrying them only delays the inevitable report.
const fn is_transient_transport_failure(error: &LoaderError) -> bool {
    matches!(error.kind, LoaderErrorKind::Unavailable(ContextUnavailable::Request(_)))
}

/// Why a response's `Content-Type` names nothing a context can be parsed from,
/// or `None` when it names something one can.
fn content_type_refusal(content_type: &headers::ContentType) -> Option<InvalidContextDocument> {
    match content_type.to_string().parse::<MediaTypeBuf>() {
        Ok(media_type) if is_json_like(&media_type) => None,
        Ok(media_type) => Some(InvalidContextDocument::UnsupportedContentType(Box::new(media_type))),
        Err(source) => Some(InvalidContextDocument::MalformedContentType(source)),
    }
}

/// Whether a media type names something a context can be parsed from.
fn is_json_like(media_type: &MediaTypeBuf) -> bool {
    let subtype = media_type.subty().as_str();
    subtype == "json"
        || subtype == "ld+json"
        || media_type.suffix().is_some_and(|suffix| suffix.as_str() == "json")
        || (media_type.ty().as_str() == "text" && subtype == "plain")
}

/// The failure reported for a context past the configured size limit.
const fn too_large(url: IriBuf, size: usize, max: usize) -> LoaderError {
    LoaderError {
        url,
        kind: LoaderErrorKind::TooLarge { size, max },
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::ContextCacheError,
        context_subtree::context_subtree,
        loader::{
            error::{ContextUnavailable, InvalidContextDocument, LoaderError, LoaderErrorKind},
            remote_document::{is_json_like, is_transient_transport_failure, ld_json_media_type},
        },
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use iri_rs::IriBuf;
    use mediatype::MediaTypeBuf;
    use reqwest::{Client, StatusCode};
    use serde_json::json;
    use std::sync::Arc;

    /// A loader error under a fixed URL, for the retry-classification test.
    fn error(kind: LoaderErrorKind) -> LoaderError {
        LoaderError {
            url: IriBuf::new("https://example.com/context.jsonld".to_owned()).expect("valid URL"),
            kind,
        }
    }

    #[test]
    fn only_a_transport_failure_is_retried() {
        assert!(
            is_transient_transport_failure(&error(LoaderErrorKind::Unavailable(ContextUnavailable::Request(Arc::new(
                Client::new().get("not a url").build().expect_err("a malformed URL fails the request builder")
            ))))),
            "a dropped or refused connection is transient and retried"
        );
        for deterministic in [
            LoaderErrorKind::Http(StatusCode::NOT_FOUND),
            LoaderErrorKind::TooLarge { size: 10, max: 5 },
            LoaderErrorKind::Invalid(InvalidContextDocument::NoContext),
            LoaderErrorKind::FetchDisabled,
            LoaderErrorKind::Unavailable(ContextUnavailable::Cache(ContextCacheError::Read(PersistenceFailure::Unavailable))),
        ] {
            assert!(!is_transient_transport_failure(&error(deterministic)), "a deterministic failure is not retried");
        }
    }

    /// Clause 5.13.2.3: a downloaded `@context` is a referenced object, so
    /// "all extra information located outside of the @context subtree" is gone
    /// before the document reaches either cache tier. A document whose extras
    /// survived was stored whole, and clause 5.13.4 would have served them back.
    #[test]
    fn a_downloaded_document_is_reduced_to_its_context_subtree() {
        let downloaded = json!({
            "@context": { "Device": "https://example.org/Device" },
            "id": "urn:ngsi-ld:Context:from-the-origin-server",
            "unrelated": [1, 2, 3]
        });

        pretty_assertions::assert_eq!(
            context_subtree(downloaded),
            Some(json!({ "@context": { "Device": "https://example.org/Device" } }))
        );
    }

    #[test]
    fn only_json_shaped_content_types_are_accepted_for_a_context() {
        for accepted in ["application/json", "application/ld+json", "application/vnd.example+json", "text/plain"] {
            let media_type: MediaTypeBuf = accepted.parse().expect("a media type");
            assert!(is_json_like(&media_type), "{accepted} must be accepted");
        }
        for rejected in ["text/html", "application/octet-stream"] {
            let media_type: MediaTypeBuf = rejected.parse().expect("a media type");
            assert!(!is_json_like(&media_type), "{rejected} must be refused");
        }
    }

    #[test]
    fn a_loaded_document_reports_the_json_ld_media_type() {
        pretty_assertions::assert_eq!(ld_json_media_type().to_string(), "application/ld+json");
    }
}
