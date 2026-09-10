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
        error::{LoaderError, LoaderErrorKind},
    },
};
use andromeda_http_headers::json_ld_accept::JsonLdAccept;
use headers::HeaderMapExt;
use iri_rs::IriBuf;
use mediatype::MediaTypeBuf;
use reqwest::{Client, header::HeaderMap};
use serde_json::Value;
use std::str::from_utf8;

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
        kind: LoaderErrorKind::Request(error.to_string()),
    })?;

    if !response.status().is_success() {
        return Err(LoaderError {
            url: url.clone(), // clone: the failure is reported under the URL that was requested
            kind: LoaderErrorKind::Http(response.status()),
        });
    }

    if let Some(content_type) = response.headers().typed_get::<headers::ContentType>()
        && !is_json_like(&content_type.to_string())
    {
        return Err(LoaderError {
            url: url.clone(), // clone: the failure is reported under the URL that was requested
            kind: LoaderErrorKind::Invalid(format!("Unsupported context content type: {content_type}")),
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
        kind: LoaderErrorKind::Request(error.to_string()),
    })?;

    if bytes.len() > max_bytes {
        return Err(too_large(url.clone(), bytes.len(), max_bytes)); // clone: the failure outlives the borrowed URL
    }

    let text = from_utf8(&bytes).map_err(|error| LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Invalid(error.to_string()),
    })?;

    let json = serde_json::from_str::<Value>(text).map_err(|error| LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Invalid(error.to_string()),
    })?;
    let subtree = context_subtree(json).ok_or_else(|| LoaderError {
        url: url.clone(), // clone: the failure is reported under the URL that was requested
        kind: LoaderErrorKind::Invalid("remote document has no usable top-level @context".to_owned()),
    })?;
    Ok(RemoteContextDocument {
        value: jsonld_syntax::Value::from_serde_json(subtree),
        freshness,
    })
}

/// Whether a `Content-Type` names something a context can be parsed from.
fn is_json_like(content_type: &str) -> bool {
    match content_type.parse::<MediaTypeBuf>() {
        Ok(media_type) => {
            let subtype = media_type.subty().as_str();
            subtype == "json"
                || subtype == "ld+json"
                || media_type.suffix().is_some_and(|suffix| suffix.as_str() == "json")
                || (media_type.ty().as_str() == "text" && subtype == "plain")
        }
        Err(_) => false,
    }
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
        context_subtree::context_subtree,
        loader::remote_document::{is_json_like, ld_json_media_type},
    };
    use serde_json::json;

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
            assert!(is_json_like(accepted), "{accepted} must be accepted");
        }
        for rejected in ["text/html", "application/octet-stream", "not a media type"] {
            assert!(!is_json_like(rejected), "{rejected} must be refused");
        }
    }

    #[test]
    fn a_loaded_document_reports_the_json_ld_media_type() {
        pretty_assertions::assert_eq!(ld_json_media_type().to_string(), "application/ld+json");
    }
}
