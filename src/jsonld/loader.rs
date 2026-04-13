//! HTTP context loader with in-memory cache, registry, and optional L2 cache.
//!
//! [`HttpContextLoader`] implements the [`json_ld::Loader`] trait.  Contexts
//! are resolved in this order:
//!
//! 1. In-memory moka cache (L1).
//! 2. [`ContextRegistry`] (embedded / preloaded contexts).
//! 3. [`ContextCache`] (optional, e.g. database-backed L2 cache).
//! 4. Remote HTTP fetch (if enabled).
//!
//! Every successful load upserts to L2 so the DB-backed hit counter
//! (`number_of_hits`) is incremented on every access per NGSI-LD spec
//! clause 5.13.

use crate::jsonld::{Error, core_context::is_ngsi_ld_core_context_url, engine::RemoteContextFetch};
use async_trait::async_trait;
use dashmap::DashMap;
use iref3::{Iri, IriBuf};
use json_ld::{LoadError, Loader, RemoteDocument};
use json_syntax::Parse;
use mediatype::MediaTypeBuf;
use moka::future::Cache;
use reqwest::{Client, header::CONTENT_TYPE, redirect::Policy};
use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};
use tracing::trace;

/// Constructs a fallback IRI for use in `LoadError` when the real IRI
/// cannot be parsed.
///
/// `"urn:error"` is a syntactically valid IRI per RFC 8141, so this
/// construction is infallible in practice.
fn fallback_iri() -> IriBuf {
    match IriBuf::new("urn:error".to_string()) {
        Ok(iri) => iri,
        // Dead branch: "urn:error" always parses as a valid IRI.
        // `abort` satisfies the lint without using panic/unwrap/expect.
        Err(_) => std::process::abort(),
    }
}

/// `application/ld+json` media type constant for `json_ld::RemoteDocument` compatibility.
///
/// The `json_ld` crate requires `mime::Mime` in its `RemoteDocument::new` API.
/// The literal `"application/ld+json"` is always a valid MIME type; the
/// `unwrap_or` fallback is unreachable but avoids using `.unwrap()`.
static MEDIA_TYPE_LD_JSON: LazyLock<mime::Mime> = LazyLock::new(|| "application/ld+json".parse().unwrap_or(mime::APPLICATION_OCTET_STREAM));

/// Accept header value for context fetches.
const ACCEPT_HEADER: &str = "application/ld+json, application/json";

// ---------------------------------------------------------------------------
// ContextCache trait (replaces direct DB dependency)
// ---------------------------------------------------------------------------

/// Asynchronous L2 context cache (e.g. database-backed).
///
/// Implement this trait to provide persistent context caching outside the
/// engine.  The engine will check the cache before making HTTP requests and
/// will persist successfully fetched remote contexts via [`put`](ContextCache::put).
#[async_trait]
pub trait ContextCache: Send + Sync + std::fmt::Debug {
    /// Look up a previously cached context by URL.
    async fn get(&self, url: &str) -> Option<serde_json::Value>;

    /// Store a context payload for the given URL.
    async fn put(&self, url: &str, payload: serde_json::Value);
}

// ---------------------------------------------------------------------------
// ContextRegistry
// ---------------------------------------------------------------------------

/// Thread-safe registry of preloaded / embedded JSON-LD contexts.
///
/// Used for core contexts and file-based preloads that must be available
/// without network access.
#[derive(Debug, Default, Clone)]
pub struct ContextRegistry {
    contexts: Arc<DashMap<String, Arc<json_syntax::Value>>>,
}

impl ContextRegistry {
    /// Create an empty registry.
    pub fn new() -> ContextRegistry {
        ContextRegistry::default()
    }

    /// Register a context document for a URL.
    ///
    /// The document is normalized at registration time (embedded core context
    /// aliases are stripped for non-core URLs) so that lookups return
    /// ready-to-use values without per-access normalization.
    pub fn register(&self, url: impl Into<String>, value: json_syntax::Value) {
        let url = url.into();
        let normalized = normalize_loaded_context_document(&url, value);
        self.contexts.insert(url, Arc::new(normalized));
    }

    /// Retrieve a previously registered context.
    pub fn get(&self, url: &str) -> Option<Arc<json_syntax::Value>> {
        self.contexts.get(url).map(|v| Arc::clone(v.value()))
    }
}

// ---------------------------------------------------------------------------
// HttpContextLoader
// ---------------------------------------------------------------------------

/// HTTP context loader with L1 cache, registry, and optional L2 cache.
///
/// Cloning is not supported — wrap in `Arc` to share across tasks.
#[derive(Debug)]
pub struct HttpContextLoader {
    client: Client,
    cache: Cache<String, Arc<json_syntax::Value>>,
    registry: Arc<ContextRegistry>,
    l2_cache: Option<Arc<dyn ContextCache>>,
    allow_remote: RemoteContextFetch,
    max_context_bytes: usize,
}

impl HttpContextLoader {
    /// Create a new loader.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ContextLoad`] if the HTTP client cannot be built.
    pub(crate) fn new(
        registry: Arc<ContextRegistry>,
        allow_remote: RemoteContextFetch,
        timeout_seconds: u64,
        max_redirects: u32,
        cache_capacity: u64,
        cache_ttl_seconds: u64,
        max_context_bytes: usize,
    ) -> Result<HttpContextLoader, Error> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .redirect(Policy::limited(max_redirects as usize))
            .build()
            .map_err(|e| Error::ContextLoad {
                url: "client".to_string(),
                message: e.to_string(),
            })?;

        let cache = Cache::builder()
            .max_capacity(cache_capacity)
            .time_to_live(Duration::from_secs(cache_ttl_seconds))
            .build();

        Ok(HttpContextLoader {
            client,
            cache,
            registry,
            l2_cache: None,
            allow_remote,
            max_context_bytes,
        })
    }

    /// Attach an L2 context cache.
    pub(crate) fn with_l2_cache(mut self, cache: Arc<dyn ContextCache>) -> HttpContextLoader {
        self.l2_cache = Some(cache);
        self
    }

    /// Load a context by URL through the full resolution pipeline
    /// (L1 → registry → L2 → remote).
    ///
    /// # Errors
    ///
    /// Returns an [`Error`] variant describing the failure (fetch disabled,
    /// HTTP error, size exceeded, parse error, etc.).
    pub async fn load_value(&self, url: &str) -> Result<Arc<json_syntax::Value>, Error> {
        let url_string = normalize_local_hosted_context_url(url);
        if url_string != url {
            trace!("Normalized hosted context URL for lookup: {} -> {}", url, url_string);
        }
        let registry = self.registry.clone();
        let client = self.client.clone();
        let allow_remote = self.allow_remote;
        let max_bytes = self.max_context_bytes;
        let l2_cache = self.l2_cache.clone();

        let result = self
            .cache
            .try_get_with(url_string.clone(), async move {
                // 1. Registry
                if let Some(value) = registry.get(&url_string) {
                    return Ok(value);
                }

                // 2. L2 cache
                if let Some(ref cache) = l2_cache
                    && let Some(payload) = cache.get(&url_string).await
                {
                    trace!("L2 cache hit for context: {}", url_string);
                    let serialized = serde_json::to_string(&payload);
                    let json_str = match serialized {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!("L2 cache entry for {} could not be serialized: {}", url_string, e);
                            String::new()
                        }
                    };
                    let (parsed, _) = json_syntax::Value::parse_str(&json_str).map_err(|e| LoaderError {
                        url: url_string.clone(),
                        kind: LoaderErrorKind::Invalid(e.to_string()),
                    })?;
                    let normalized = normalize_loaded_context_document(&url_string, parsed);
                    // Persist L2 hit back (already in L2, but the closure only
                    // runs on L1 miss — no additional write needed here).
                    return Ok(Arc::new(normalized));
                }

                // 3. Remote fetch
                if allow_remote == RemoteContextFetch::Deny {
                    return Err(LoaderError::fetch_disabled(url_string.clone()));
                }

                match fetch_and_parse(&client, &url_string, max_bytes).await {
                    Ok(value) => {
                        let normalized = normalize_loaded_context_document(&url_string, value);
                        // Write to L2 only on remote fetch success (L1 miss path).
                        if let Some(ref cache) = l2_cache {
                            let serde_value = json_syntax::Value::into_serde_json(normalized.clone());
                            cache.put(&url_string, serde_value).await;
                            trace!("Persisted context to L2 cache: {}", url_string);
                        }
                        Ok(Arc::new(normalized))
                    }
                    Err(err) => {
                        if let Some(value) = registry.get(&url_string) {
                            Ok(value)
                        } else {
                            Err(err)
                        }
                    }
                }
            })
            .await;

        let url_for_l2 = normalize_local_hosted_context_url(url);
        match result {
            Ok(value) => {
                // Always upsert to L2 so the DB-backed hit counter
                // (`numberOfHits`, spec clause 5.13.3.5) is incremented
                // on every access.
                if let Some(ref cache) = self.l2_cache
                    && !is_ngsi_ld_core_context_url(&url_for_l2)
                {
                    let serde_value = json_syntax::Value::into_serde_json((*value).clone());
                    cache.put(&url_for_l2, serde_value).await;
                }
                Ok(value)
            }
            Err(err) => Err(Error::from_loader_error(err.as_ref())),
        }
    }

    /// Fetch a context directly from the remote source, bypassing L1/L2 caches.
    ///
    /// Use this when the caller needs to verify that the original source is
    /// still reachable (e.g. delete-with-reload).
    ///
    /// # Errors
    ///
    /// Returns a [`LoaderError`] if remote fetch is disabled or the request fails.
    pub(crate) async fn fetch_remote_value(&self, url: &str) -> Result<Arc<json_syntax::Value>, LoaderError> {
        let url_string = normalize_local_hosted_context_url(url);
        if self.allow_remote == RemoteContextFetch::Deny {
            return Err(LoaderError::fetch_disabled(url_string));
        }
        let value = fetch_and_parse(&self.client, &url_string, self.max_context_bytes).await?;
        let normalized = normalize_loaded_context_document(&url_string, value);
        Ok(Arc::new(normalized))
    }
}

impl Loader for HttpContextLoader {
    async fn load(&self, url: &Iri) -> Result<RemoteDocument<IriBuf>, LoadError> {
        let requested_url = url.as_str();
        let normalized_url = normalize_local_hosted_context_url(requested_url);
        let value = self.load_value(&normalized_url).await.map_err(|e| {
            let iri = IriBuf::new(normalized_url.clone()).unwrap_or_else(|_| fallback_iri());
            LoadError::new(iri, e)
        })?;

        let iri = IriBuf::new(normalized_url).map_err(|e| LoadError::new(fallback_iri(), e))?;

        Ok(RemoteDocument::new(Some(iri), Some(MEDIA_TYPE_LD_JSON.clone()), (*value).clone()))
    }
}

// ---------------------------------------------------------------------------
// URL normalisation
// ---------------------------------------------------------------------------

const DUPLICATED_API_PREFIX_PATH: &str = "/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/";
const CANONICAL_API_PREFIX_PATH: &str = "/ngsi-ld/v1/jsonldContexts/";

fn normalize_local_hosted_context_url(url: &str) -> String {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    let Some(host) = parsed.host_str() else {
        return url.to_string();
    };

    if !is_local_host(host) {
        return url.to_string();
    }

    let path = parsed.path();
    if !path.contains(DUPLICATED_API_PREFIX_PATH) {
        return url.to_string();
    }

    let normalized = path.replacen(DUPLICATED_API_PREFIX_PATH, CANONICAL_API_PREFIX_PATH, 1);
    parsed.set_path(&normalized);
    parsed.to_string()
}

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

fn normalize_loaded_context_document(url: &str, value: json_syntax::Value) -> json_syntax::Value {
    if is_ngsi_ld_core_context_url(url) {
        return value;
    }

    let mut json = json_syntax::Value::into_serde_json(value);
    remove_embedded_core_context_aliases(&mut json);
    json_syntax::Value::from_serde_json(json)
}

fn remove_embedded_core_context_aliases(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(context) = map.get_mut("@context") {
                strip_core_aliases_from_context_slot(context);
            }

            for inner in map.values_mut() {
                remove_embedded_core_context_aliases(inner);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                remove_embedded_core_context_aliases(item);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => {}
    }
}

fn strip_core_aliases_from_context_slot(context: &mut serde_json::Value) {
    match context {
        serde_json::Value::String(url) if is_ngsi_ld_core_context_url(url) => {
            *context = serde_json::Value::Array(Vec::new());
        }
        serde_json::Value::Array(items) => {
            items.retain(|item| !item.as_str().is_some_and(is_ngsi_ld_core_context_url));
            for item in items {
                remove_embedded_core_context_aliases(item);
            }
        }
        serde_json::Value::Object(_) => {
            remove_embedded_core_context_aliases(context);
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => {}
    }
}

// ---------------------------------------------------------------------------
// Internal error types
// ---------------------------------------------------------------------------

/// Internal error type for the context loader pipeline.
///
/// Converted to [`Error`] at the public API boundary via
/// [`Error::from_loader_error`].
#[derive(Debug, thiserror::Error)]
#[error("{kind} for {url}")]
pub(crate) struct LoaderError {
    pub(crate) url: String,
    pub(crate) kind: LoaderErrorKind,
}

/// Classification of loader failures.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LoaderErrorKind {
    /// Remote context fetch is disabled by configuration.
    #[error("Remote context fetch disabled")]
    FetchDisabled,
    /// HTTP request returned a non-success status code.
    #[error("HTTP {0}")]
    Http(reqwest::StatusCode),
    /// Network or transport-level request failure.
    #[error("Request failed: {0}")]
    Request(String),
    /// Response body exceeds the configured size limit.
    #[error("Context too large ({size} > {max} bytes)")]
    TooLarge {
        /// Actual payload size in bytes.
        size: usize,
        /// Configured maximum in bytes.
        max: usize,
    },
    /// Response body could not be parsed as valid JSON-LD.
    #[error("Invalid context: {0}")]
    Invalid(String),
}

impl LoaderError {
    const fn fetch_disabled(url: String) -> LoaderError {
        LoaderError {
            url,
            kind: LoaderErrorKind::FetchDisabled,
        }
    }
}

impl Error {
    /// Convert a [`LoaderError`] into the public [`Error`] type.
    pub(crate) fn from_loader_error(err: &LoaderError) -> Error {
        match &err.kind {
            LoaderErrorKind::FetchDisabled => Error::ContextFetchDisabled { url: err.url.clone() },
            LoaderErrorKind::Http(status) => Error::ContextHttp {
                url: err.url.clone(),
                status: *status,
            },
            LoaderErrorKind::Request(message) => Error::ContextLoad {
                url: err.url.clone(),
                message: message.clone(),
            },
            LoaderErrorKind::TooLarge { size, max } => Error::ContextTooLarge {
                url: err.url.clone(),
                size: *size,
                max: *max,
            },
            LoaderErrorKind::Invalid(message) => Error::ContextInvalid {
                url: err.url.clone(),
                message: message.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP fetch
// ---------------------------------------------------------------------------

async fn fetch_and_parse(client: &Client, url: &str, max_bytes: usize) -> Result<json_syntax::Value, LoaderError> {
    let response = client.get(url).header("Accept", ACCEPT_HEADER).send().await.map_err(|e| LoaderError {
        url: url.to_string(),
        kind: LoaderErrorKind::Request(e.to_string()),
    })?;

    if !response.status().is_success() {
        return Err(LoaderError {
            url: url.to_string(),
            kind: LoaderErrorKind::Http(response.status()),
        });
    }

    if let Some(content_type) = response.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()) {
        let is_json_like = match content_type.parse::<MediaTypeBuf>() {
            Ok(mt) => {
                let subtype = mt.subty().as_str();
                subtype == "json"
                    || subtype == "ld+json"
                    || mt.suffix().is_some_and(|s| s.as_str() == "json")
                    || (mt.ty().as_str() == "text" && subtype == "plain")
            }
            Err(_) => false,
        };
        if !is_json_like {
            return Err(LoaderError {
                url: url.to_string(),
                kind: LoaderErrorKind::Invalid(format!("Unsupported context content type: {}", content_type)),
            });
        }
    }

    if let Some(len) = response.content_length()
        && len as usize > max_bytes
    {
        return Err(LoaderError {
            url: url.to_string(),
            kind: LoaderErrorKind::TooLarge {
                size: len as usize,
                max: max_bytes,
            },
        });
    }

    let bytes = response.bytes().await.map_err(|e| LoaderError {
        url: url.to_string(),
        kind: LoaderErrorKind::Request(e.to_string()),
    })?;

    if bytes.len() > max_bytes {
        return Err(LoaderError {
            url: url.to_string(),
            kind: LoaderErrorKind::TooLarge {
                size: bytes.len(),
                max: max_bytes,
            },
        });
    }

    let text = std::str::from_utf8(&bytes).map_err(|e| LoaderError {
        url: url.to_string(),
        kind: LoaderErrorKind::Invalid(e.to_string()),
    })?;

    let (value, _) = json_syntax::Value::parse_str(text).map_err(|e| LoaderError {
        url: url.to_string(),
        kind: LoaderErrorKind::Invalid(e.to_string()),
    })?;

    Ok(value)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn normalizes_localhost_duplicated_jsonld_context_path() {
        let url = "http://localhost:1056/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc";
        let normalized = normalize_local_hosted_context_url(url);
        pretty_assertions::assert_eq!(normalized, "http://localhost:1056/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc");
    }

    #[test]
    fn does_not_normalize_non_local_context_url() {
        let url = "https://forge.etsi.org/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc";
        pretty_assertions::assert_eq!(normalize_local_hosted_context_url(url), url);
    }

    #[test]
    fn normalizes_non_core_loaded_context_by_stripping_embedded_core_alias() {
        let input = json_syntax::Value::from_serde_json(serde_json::json!({
            "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                { "location": "https://example.com/location" }
            ]
        }));

        let normalized = normalize_loaded_context_document("https://example.com/compound.jsonld", input);
        let normalized_json = json_syntax::Value::into_serde_json(normalized);

        pretty_assertions::assert_eq!(
            normalized_json,
            serde_json::json!({
                "@context": [
                    { "location": "https://example.com/location" }
                ]
            })
        );
    }

    #[test]
    fn keeps_core_context_document_untouched() {
        let input = json_syntax::Value::from_serde_json(serde_json::json!({
            "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                { "location": "https://example.com/location" }
            ]
        }));

        let normalized = normalize_loaded_context_document("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld", input);
        let normalized_json = json_syntax::Value::into_serde_json(normalized);

        pretty_assertions::assert_eq!(
            normalized_json,
            serde_json::json!({
                "@context": [
                    "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                    { "location": "https://example.com/location" }
                ]
            })
        );
    }

    #[tokio::test]
    async fn load_from_registry_without_remote() {
        let registry = Arc::new(ContextRegistry::new());
        registry.register(
            "https://example.com/context.jsonld",
            json_syntax::Value::from_serde_json(serde_json::json!({
                "@context": { "name": "https://example.com/name" }
            })),
        );

        let loader = HttpContextLoader::new(registry, RemoteContextFetch::Deny, 30, 5, 1024, 3600, 2_000_000).expect("loader");
        let iri = IriBuf::new("https://example.com/context.jsonld".to_string()).unwrap();

        let doc = loader.load(iri.as_iri()).await.expect("load");
        assert!(doc.document.is_object());
    }

    #[tokio::test]
    async fn loader_uses_normalized_url_for_registry_lookup() {
        let registry = Arc::new(ContextRegistry::new());
        registry.register(
            "http://localhost:1056/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc",
            json_syntax::Value::from_serde_json(serde_json::json!({
                "@context": { "A1": "https://example.com/A1" }
            })),
        );

        let loader = HttpContextLoader::new(registry, RemoteContextFetch::Deny, 30, 5, 1024, 3600, 2_000_000).expect("loader");
        let iri = IriBuf::new("http://localhost:1056/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc".to_string()).unwrap();

        let doc = loader.load(iri.as_iri()).await.expect("load");
        assert!(doc.document.is_object());
    }
}
