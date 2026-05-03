//! HTTP context loader with in-memory cache, registry, and optional L2 cache.
//!
//! [`HttpContextLoader`] implements the [`jsonld::Loader`] trait.  Contexts
//! are resolved in this order:
//!
//! 1. In-memory moka cache (L1).
//! 2. [`ContextRegistry`] (embedded / preloaded contexts).
//! 3. [`ContextCache`] (optional, e.g. database-backed L2 cache).
//! 4. Remote HTTP fetch (if enabled).
//!
//! L2 persistence happens once per URL on first remote-fetch success.
//! The `number_of_hits` counter required by NGSI-LD spec clause 5.13 is
//! tracked via an in-process [`HitCounter`] and flushed periodically by
//! a background task in the broker startup path, so the ingestion hot
//! path never serialises on a shared `jsonld_contexts` row.

use crate::jsonld::{Error, core_context::is_ngsi_ld_core_context_url, engine::RemoteContextFetch};
use async_trait::async_trait;
use dashmap::DashMap;
use iri_rs::{Iri, IriBuf};
use jsonld::{LoadError, Loader, RemoteDocument};
use jsonld_syntax::Parse;
use mediatype::MediaTypeBuf;
use moka::future::Cache;
use reqwest::{Client, header::CONTENT_TYPE, redirect::Policy};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
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

/// `application/ld+json` media type constant for `jsonld::RemoteDocument` compatibility.
///
/// The fork's `RemoteDocument` uses `mediatype::MediaTypeBuf` for the
/// content-type field. Constructed once via [`jsonld_core::LD_JSON_MEDIA_TYPE`].
fn ld_json_media_type() -> MediaTypeBuf {
    jsonld_core::LD_JSON_MEDIA_TYPE.into()
}

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

    /// Store a context payload for the given URL if no row exists yet.
    ///
    /// Implementations must NOT increment `numberOfHits` or update
    /// `last_usage` here. Hit accounting is handled out-of-band via
    /// [`HitCounter`] and a periodic flush task wired in the broker
    /// startup path.
    async fn put(&self, url: &str, payload: serde_json::Value);
}

/// In-memory accumulator for JSON-LD context hit counts.
///
/// The hot ingestion path bumps a per-URL atomic instead of issuing a
/// `number_of_hits = number_of_hits + 1` UPDATE on every entity
/// expansion. A background task drains pending deltas every few
/// seconds and folds them into a single bulk UPDATE per flush cycle.
///
/// Spec note: NGSI-LD clause 5.13.3.5 requires the broker to maintain
/// a `numberOfHits` counter, but does not require it to be persisted
/// synchronously per access. Eventual consistency between in-memory
/// accumulation and DB state is sufficient.
#[derive(Debug, Default)]
pub struct HitCounter {
    pending: DashMap<String, AtomicU64>,
}

impl HitCounter {
    /// Create an empty counter.
    pub fn new() -> HitCounter {
        HitCounter::default()
    }

    /// Record one hit for the given URL.
    ///
    /// Lock-free path on the steady-state branch: a `get` + atomic
    /// `fetch_add` against an existing `AtomicU64`. Only the very first
    /// hit per URL takes the DashMap insert path.
    pub fn bump(&self, url: &str) {
        if let Some(entry) = self.pending.get(url) {
            entry.value().fetch_add(1, Ordering::Relaxed);
            return;
        }
        self.pending
            .entry(url.to_string()) // clone: DashMap requires owned key on insert
            .or_insert_with(|| AtomicU64::new(0))
            .value()
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Atomically swap each pending counter to zero and return the drained
    /// `(url, delta)` pairs. Entries with zero delta are skipped. Map
    /// entries themselves are retained so subsequent bumps stay on the
    /// fast path.
    pub fn drain(&self) -> Vec<(String, u64)> {
        let mut out = Vec::with_capacity(self.pending.len());
        for entry in self.pending.iter() {
            let n = entry.value().swap(0, Ordering::Relaxed);
            if n > 0 {
                out.push((entry.key().clone(), n)); // clone: send drained delta to caller
            }
        }
        out
    }
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
    contexts: Arc<DashMap<String, Arc<jsonld_syntax::Value>>>,
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
    pub fn register(&self, url: impl Into<String>, value: jsonld_syntax::Value) {
        let url = url.into();
        let normalized = normalize_loaded_context_document(&url, value);
        self.contexts.insert(url, Arc::new(normalized));
    }

    /// Retrieve a previously registered context.
    pub fn get(&self, url: &str) -> Option<Arc<jsonld_syntax::Value>> {
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
    cache: Cache<String, Arc<jsonld_syntax::Value>>,
    registry: Arc<ContextRegistry>,
    l2_cache: Option<Arc<dyn ContextCache>>,
    hit_counter: Option<Arc<HitCounter>>,
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
            hit_counter: None,
            allow_remote,
            max_context_bytes,
        })
    }

    /// Attach an L2 context cache.
    pub(crate) fn with_l2_cache(mut self, cache: Arc<dyn ContextCache>) -> HttpContextLoader {
        self.l2_cache = Some(cache);
        self
    }

    /// Attach a shared in-memory hit counter. The same `Arc<HitCounter>`
    /// must be handed to the periodic flush task so accumulated deltas
    /// reach the database.
    pub(crate) fn with_hit_counter(mut self, counter: Arc<HitCounter>) -> HttpContextLoader {
        self.hit_counter = Some(counter);
        self
    }

    /// Load a context by URL through the full resolution pipeline
    /// (L1 → registry → L2 → remote).
    ///
    /// # Errors
    ///
    /// Returns an [`Error`] variant describing the failure (fetch disabled,
    /// HTTP error, size exceeded, parse error, etc.).
    pub async fn load_value(&self, url: &str) -> Result<Arc<jsonld_syntax::Value>, Error> {
        self.load_value_raw(url).await.map_err(|err| Error::from_loader_error(err.as_ref()))
    }

    /// Same as [`Self::load_value`] but yields the raw [`LoaderError`] (wrapped
    /// in `Arc` to satisfy `moka`'s `Clone + Send + Sync` requirement on cached
    /// errors). Used by the [`jsonld::Loader`] trait implementation, which
    /// requires the loader-specific error type.
    pub(crate) async fn load_value_raw(&self, url: &str) -> Result<Arc<jsonld_syntax::Value>, Arc<LoaderError>> {
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
                    let (parsed, _) = jsonld_syntax::Value::parse_str(&json_str).map_err(|e| LoaderError {
                        url: url_string.clone(),
                        kind: LoaderErrorKind::Invalid(e.to_string()),
                    })?;
                    let normalized = normalize_loaded_context_document(&url_string, parsed);
                    return Ok(Arc::new(normalized));
                }

                // 3. Remote fetch
                if allow_remote == RemoteContextFetch::Deny {
                    return Err(LoaderError::fetch_disabled(url_string.clone()));
                }

                match fetch_and_parse(&client, &url_string, max_bytes).await {
                    Ok(value) => {
                        let normalized = normalize_loaded_context_document(&url_string, value);
                        // Write to L2 only on remote fetch success (L1 miss
                        // path). Persisted as an empty-counter row; hit
                        // accounting is handled separately by HitCounter.
                        if let Some(ref cache) = l2_cache {
                            let serde_value = jsonld_syntax::Value::into_serde_json(normalized.clone());
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
                // Bump in-memory hit counter; the periodic flusher folds
                // accumulated deltas into one bulk UPDATE per cycle so the
                // ingestion hot path never serialises on a shared row.
                // Skip core contexts: they live in the registry, never have
                // a `jsonld_contexts` row, and the bump would be a no-op.
                if let Some(ref counter) = self.hit_counter
                    && !is_ngsi_ld_core_context_url(&url_for_l2)
                {
                    counter.bump(&url_for_l2);
                }
                Ok(value)
            }
            Err(err) => Err(err),
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
    pub(crate) async fn fetch_remote_value(&self, url: &str) -> Result<Arc<jsonld_syntax::Value>, LoaderError> {
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
    type Error = LoaderError;

    async fn load(&self, url: Iri<&str>) -> Result<RemoteDocument<IriBuf>, LoadError<Self::Error>> {
        let requested_url = url.as_str();
        let normalized_url = normalize_local_hosted_context_url(requested_url);
        let value = self.load_value_raw(&normalized_url).await.map_err(|e| {
            let iri = IriBuf::new(normalized_url.clone()).unwrap_or_else(|_| fallback_iri());
            LoadError::new(iri, (*e).clone())
        })?;

        let iri = IriBuf::new(normalized_url.clone()).map_err(|e| {
            LoadError::new(
                fallback_iri(),
                LoaderError {
                    url: normalized_url,
                    kind: LoaderErrorKind::Invalid(format!("invalid IRI: {e}")),
                },
            )
        })?;

        Ok(RemoteDocument::new(Some(iri), Some(ld_json_media_type()), (*value).clone()))
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

fn normalize_loaded_context_document(url: &str, value: jsonld_syntax::Value) -> jsonld_syntax::Value {
    if is_ngsi_ld_core_context_url(url) {
        return value;
    }

    let mut json = jsonld_syntax::Value::into_serde_json(value);
    remove_embedded_core_context_aliases(&mut json);
    jsonld_syntax::Value::from_serde_json(json)
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

/// Loader pipeline error surfaced via [`jsonld::Loader::Error`].
///
/// Converted to the public [`Error`] type via [`Error::from_loader_error`].
#[derive(Debug, Clone, thiserror::Error)]
#[error("{kind} for {url}")]
pub struct LoaderError {
    /// URL that was being resolved when the failure occurred.
    pub url: String,
    /// Classification of the failure.
    pub kind: LoaderErrorKind,
}

/// Classification of loader failures.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LoaderErrorKind {
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
    pub(crate) const fn fetch_disabled(url: String) -> LoaderError {
        LoaderError {
            url,
            kind: LoaderErrorKind::FetchDisabled,
        }
    }
}

impl Error {
    /// Convert a [`LoaderError`] into the public [`Error`] type.
    pub fn from_loader_error(err: &LoaderError) -> Error {
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

async fn fetch_and_parse(client: &Client, url: &str, max_bytes: usize) -> Result<jsonld_syntax::Value, LoaderError> {
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

    let (value, _) = jsonld_syntax::Value::parse_str(text).map_err(|e| LoaderError {
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
        let input = jsonld_syntax::Value::from_serde_json(serde_json::json!({
            "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                { "location": "https://example.com/location" }
            ]
        }));

        let normalized = normalize_loaded_context_document("https://example.com/compound.jsonld", input);
        let normalized_json = jsonld_syntax::Value::into_serde_json(normalized);

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
        let input = jsonld_syntax::Value::from_serde_json(serde_json::json!({
            "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                { "location": "https://example.com/location" }
            ]
        }));

        let normalized = normalize_loaded_context_document("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld", input);
        let normalized_json = jsonld_syntax::Value::into_serde_json(normalized);

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

    #[test]
    fn hit_counter_aggregates_per_url_then_drains_to_zero() {
        let counter = HitCounter::new();
        let url_a = "https://example.com/ctx-a";
        let url_b = "https://example.com/ctx-b";

        for _ in 0..5 {
            counter.bump(url_a);
        }
        counter.bump(url_b);
        counter.bump(url_b);

        let mut drained = counter.drain();
        drained.sort_by(|left, right| left.0.cmp(&right.0));
        pretty_assertions::assert_eq!(drained, vec![(url_a.to_string(), 5), (url_b.to_string(), 2)]);

        // Second drain returns nothing because the swap reset every counter
        // to zero; entries with zero delta are filtered out.
        let empty = counter.drain();
        assert!(empty.is_empty(), "second drain must be empty, got {:?}", empty);

        // After drain, fast-path entries remain; new bumps reuse them.
        counter.bump(url_a);
        let again = counter.drain();
        pretty_assertions::assert_eq!(again, vec![(url_a.to_string(), 1)]);
    }

    #[tokio::test]
    async fn load_from_registry_without_remote() {
        let registry = Arc::new(ContextRegistry::new());
        registry.register(
            "https://example.com/context.jsonld",
            jsonld_syntax::Value::from_serde_json(serde_json::json!({
                "@context": { "name": "https://example.com/name" }
            })),
        );

        let loader = HttpContextLoader::new(registry, RemoteContextFetch::Deny, 30, 5, 1024, 3600, 2_000_000).expect("loader");
        let iri = IriBuf::new("https://example.com/context.jsonld".to_string()).unwrap();

        let doc = loader.load(iri.as_ref()).await.expect("load");
        assert!(doc.document.is_object());
    }

    #[tokio::test]
    async fn loader_uses_normalized_url_for_registry_lookup() {
        let registry = Arc::new(ContextRegistry::new());
        registry.register(
            "http://localhost:1056/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc",
            jsonld_syntax::Value::from_serde_json(serde_json::json!({
                "@context": { "A1": "https://example.com/A1" }
            })),
        );

        let loader = HttpContextLoader::new(registry, RemoteContextFetch::Deny, 30, 5, 1024, 3600, 2_000_000).expect("loader");
        let iri = IriBuf::new("http://localhost:1056/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc".to_string()).unwrap();

        let doc = loader.load(iri.as_ref()).await.expect("load");
        assert!(doc.document.is_object());
    }
}
