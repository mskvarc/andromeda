//! HTTP context loader with in-memory cache, registry, and optional L2 cache.
//!
//! [`HttpContextLoader`] implements the [`json_ld::Loader`] trait.  Contexts
//! are resolved in this order:
//!
//! 1. In-memory moka cache (L1).
//! 2. [`ContextRegistry`] (embedded / preloaded contexts).
//! 3. [`ContextCache`] (optional, e.g. database-backed L2 cache).
//! 4. Remote HTTP fetch (if enabled).

use crate::jsonld::{Error, core_context::is_ngsi_ld_core_context_url};
use async_trait::async_trait;
use dashmap::DashMap;
use iref3::{Iri, IriBuf};
use json_ld::{LoadError, Loader, RemoteDocument};
use json_syntax::Parse;
use moka::future::Cache;
use reqwest::{Client, header::CONTENT_TYPE, redirect::Policy};
use std::{sync::Arc, time::Duration};
use tracing::trace;

// ---------------------------------------------------------------------------
// ContextCache trait (replaces direct DB dependency)
// ---------------------------------------------------------------------------

/// Asynchronous L2 context cache (e.g. database-backed).
///
/// Implement this trait to provide persistent context caching outside the
/// engine.  The engine will check the cache before making HTTP requests and
/// will persist successfully fetched remote contexts via [`put`](ContextCache::put).
#[async_trait]
pub trait ContextCache: Send + Sync {
    /// Look up a previously cached context by URL.
    async fn get(&self, url: &str) -> Option<serde_json::Value>;

    /// Store a context payload for the given URL.
    async fn put(&self, url: &str, payload: serde_json::Value);
}

// ---------------------------------------------------------------------------
// ContextRegistry
// ---------------------------------------------------------------------------

/// Thread-safe registry of preloaded / embedded JSON-LD contexts.
#[derive(Debug, Default, Clone)]
pub struct ContextRegistry {
    contexts: Arc<DashMap<String, json_syntax::Value>>,
}

impl ContextRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a context document for a URL.
    pub fn register(&self, url: impl Into<String>, value: json_syntax::Value) {
        self.contexts.insert(url.into(), value);
    }

    /// Retrieve a previously registered context.
    pub fn get(&self, url: &str) -> Option<json_syntax::Value> {
        self.contexts.get(url).map(|v| v.clone())
    }
}

// ---------------------------------------------------------------------------
// HttpContextLoader
// ---------------------------------------------------------------------------

/// HTTP context loader with L1 cache, registry, and optional L2 cache.
pub struct HttpContextLoader {
    client: Client,
    cache: Cache<String, Arc<json_syntax::Value>>,
    registry: Arc<ContextRegistry>,
    l2_cache: Option<Arc<dyn ContextCache>>,
    allow_remote: bool,
    max_context_bytes: usize,
}

impl HttpContextLoader {
    /// Create a new loader.
    pub(crate) fn new(
        registry: Arc<ContextRegistry>,
        allow_remote: bool,
        timeout_seconds: u64,
        max_redirects: u32,
        cache_capacity: u64,
        cache_ttl_seconds: u64,
        max_context_bytes: usize,
    ) -> Result<Self, Error> {
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

        Ok(Self {
            client,
            cache,
            registry,
            l2_cache: None,
            allow_remote,
            max_context_bytes,
        })
    }

    /// Attach an L2 context cache.
    pub(crate) fn with_l2_cache(mut self, cache: Arc<dyn ContextCache>) -> Self {
        self.l2_cache = Some(cache);
        self
    }

    /// Reference to the underlying registry.
    pub fn registry(&self) -> &Arc<ContextRegistry> {
        &self.registry
    }

    /// Number of entries currently in the L1 cache.
    pub fn cache_size(&self) -> u64 {
        self.cache.entry_count()
    }

    pub async fn load_value(&self, url: &str) -> Result<Arc<json_syntax::Value>, Error> {
        let url_string = normalize_local_hosted_context_url(url);
        if url_string != url {
            trace!("Normalized hosted context URL for lookup: {} -> {}", url, url_string);
        }
        let url_for_l2 = url_string.clone();
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
                    let normalized = normalize_loaded_context_document(&url_string, value);
                    return Ok(Arc::new(normalized));
                }

                // 2. L2 cache
                if let Some(ref cache) = l2_cache {
                    if let Some(payload) = cache.get(&url_string).await {
                        trace!("L2 cache hit for context: {}", url_string);
                        let (parsed, _) = json_syntax::Value::parse_str(&serde_json::to_string(&payload).unwrap_or_default()).map_err(|e| LoaderError {
                            url: url_string.clone(),
                            kind: LoaderErrorKind::Invalid(e.to_string()),
                        })?;
                        let normalized = normalize_loaded_context_document(&url_string, parsed);
                        return Ok(Arc::new(normalized));
                    }
                }

                // 3. Remote fetch
                if !allow_remote {
                    return Err(LoaderError::fetch_disabled(url_string.clone()));
                }

                match fetch_and_parse(&client, &url_string, max_bytes).await {
                    Ok(value) => {
                        let normalized = normalize_loaded_context_document(&url_string, value);
                        if let Some(ref cache) = l2_cache {
                            let serde_value = json_syntax::Value::into_serde_json(normalized.clone());
                            cache.put(&url_string, serde_value).await;
                            trace!("Persisted context to L2 cache: {}", url_string);
                        }
                        Ok(Arc::new(normalized))
                    }
                    Err(err) => {
                        if let Some(value) = registry.get(&url_string) {
                            let normalized = normalize_loaded_context_document(&url_string, value);
                            Ok(Arc::new(normalized))
                        } else {
                            Err(err)
                        }
                    }
                }
            })
            .await;

        match result {
            Ok(value) => {
                // Always ensure the context is persisted to L2 cache.
                // The try_get_with closure only runs on L1 miss, so an L1
                // hit would skip the L2 put.  This is cheap (upsert / no-op
                // if already present) and keeps the DB-backed list consistent.
                if let Some(ref cache) = self.l2_cache {
                    if !is_ngsi_ld_core_context_url(&url_for_l2) {
                        let serde_value = json_syntax::Value::into_serde_json((*value).clone());
                        cache.put(&url_for_l2, serde_value).await;
                    }
                }
                Ok(value)
            }
            Err(err) => Err(Error::from_loader_error(err.as_ref())),
        }
    }

    /// Fetch a context directly from the remote source, bypassing L1/L2 caches.
    pub(crate) async fn fetch_remote_value(&self, url: &str) -> Result<Arc<json_syntax::Value>, LoaderError> {
        let url_string = normalize_local_hosted_context_url(url);
        if !self.allow_remote {
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
            LoadError::new(
                IriBuf::new(normalized_url.clone()).unwrap_or_else(|_| IriBuf::new("urn:error".to_string()).unwrap()),
                e,
            )
        })?;

        let iri = IriBuf::new(normalized_url).map_err(|e| LoadError::new(IriBuf::new("urn:error".to_string()).unwrap(), e))?;

        Ok(RemoteDocument::new(Some(iri), Some("application/ld+json".parse().unwrap()), (*value).clone()))
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
        _ => {}
    }
}

fn strip_core_aliases_from_context_slot(context: &mut serde_json::Value) {
    match context {
        serde_json::Value::String(url) => {
            if is_ngsi_ld_core_context_url(url) {
                *context = serde_json::Value::Array(Vec::new());
            }
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
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Internal error types (not exposed)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub(crate) struct LoaderError {
    pub(crate) url: String,
    pub(crate) kind: LoaderErrorKind,
}

#[derive(Debug)]
pub(crate) enum LoaderErrorKind {
    FetchDisabled,
    Http(reqwest::StatusCode),
    Request(String),
    TooLarge { size: usize, max: usize },
    Invalid(String),
}

impl LoaderError {
    fn fetch_disabled(url: String) -> Self {
        Self {
            url,
            kind: LoaderErrorKind::FetchDisabled,
        }
    }
}

impl Error {
    pub(crate) fn from_loader_error(err: &LoaderError) -> Self {
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
    let response = client
        .get(url)
        .header("Accept", "application/ld+json, application/json")
        .send()
        .await
        .map_err(|e| LoaderError {
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
        let ct = content_type.to_ascii_lowercase();
        let json_like = ct.contains("application/ld+json") || ct.contains("application/json") || ct.contains("+json") || ct.contains("text/plain");
        if !json_like {
            return Err(LoaderError {
                url: url.to_string(),
                kind: LoaderErrorKind::Invalid(format!("Unsupported context content type: {}", content_type)),
            });
        }
    }

    if let Some(len) = response.content_length() {
        if len as usize > max_bytes {
            return Err(LoaderError {
                url: url.to_string(),
                kind: LoaderErrorKind::TooLarge {
                    size: len as usize,
                    max: max_bytes,
                },
            });
        }
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
    use super::*;

    #[test]
    fn normalizes_localhost_duplicated_jsonld_context_path() {
        let url = "http://localhost:1056/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc";
        let normalized = normalize_local_hosted_context_url(url);
        assert_eq!(normalized, "http://localhost:1056/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc");
    }

    #[test]
    fn does_not_normalize_non_local_context_url() {
        let url = "https://forge.etsi.org/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc";
        assert_eq!(normalize_local_hosted_context_url(url), url);
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

        assert_eq!(
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

        assert_eq!(
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

        let loader = HttpContextLoader::new(
            registry, false,     // allow_remote
            30,        // timeout_seconds
            5,         // max_redirects
            1024,      // cache_capacity
            3600,      // cache_ttl_seconds
            2_000_000, // max_context_bytes
        )
        .expect("loader");
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

        let loader = HttpContextLoader::new(registry, false, 30, 5, 1024, 3600, 2_000_000).expect("loader");
        let iri = IriBuf::new("http://localhost:1056/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc".to_string()).unwrap();

        let doc = loader.load(iri.as_iri()).await.expect("load");
        assert!(doc.document.is_object());
    }
}
