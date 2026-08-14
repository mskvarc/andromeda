//! The four-step context resolution the JSON-LD processor drives.
//!
//! A URL is looked for in the in-process moka cache, then in the registry of
//! embedded and preloaded documents, then in the caller-supplied L2 cache,
//! and only then over HTTP. The L2 write happens once per URL, on the first
//! remote-fetch success.
//!
//! Access accounting is deliberately not part of that path. Clause 5.13.3.5
//! permits a `numberOfHits` counter (numberOfHits is optional per clause
//! 5.13.3.5) but does not require it to be durable per
//! access, so a hit bumps an in-process
//! [`crate::hit_counter::HitCounter`] and a background flush folds
//! the deltas into the store — otherwise every expansion would serialise on
//! one `jsonld_contexts` row.

use crate::{
    context_cache::ContextCache,
    context_registry::ContextRegistry,
    core_context_precedence::without_embedded_core_context,
    engine::options::RemoteContextFetch,
    error::JsonLdError,
    hit_counter::{HitCounter, HitDelta},
    loader::error::{LoaderError, LoaderErrorKind},
};
use andromeda_http_headers::json_ld_accept::JsonLdAccept;
use headers::HeaderMapExt;
use iri_rs::{Iri, IriBuf};
use jsonld::{LoadError, Loader, RemoteDocument};
use jsonld_syntax::Parse;
use mediatype::MediaTypeBuf;
use moka::future::Cache;
use reqwest::{Client, redirect::Policy};
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tracing::{trace, warn};

/// A loaded context together with the backing-store ownership of its hit count.
///
/// Only contexts that have a durable cache row participate in clause 5.13.3.5
/// accounting. Registry and preloaded contexts have no row to update.
#[derive(Debug, Clone)]
struct LoadedContext {
    /// The parsed JSON-LD context payload supplied to the processor.
    value: Arc<jsonld_syntax::Value>,
    /// Where the cached document's durability and hit accounting live.
    durability: ContextDurability,
    /// Whether the remote response permits retaining this L1 entry.
    cacheable: bool,
}

#[derive(Debug)]
/// Parsed remote context and its response-specific storage permission.
struct FetchedContext {
    /// Parsed JSON-LD document.
    value: jsonld_syntax::Value,
    /// Whether response caching directives permit storage.
    cacheable: bool,
}

#[derive(Debug, Clone)]
/// Durability and hit-accounting ownership of one L1 entry.
enum ContextDurability {
    /// Embedded or preloaded registry entry with no durable hit row.
    Registry,
    /// Entry already persisted in the durable cache.
    Durable,
    /// Remotely loaded entry awaiting persistence.
    Pending(Arc<PendingPersistence>),
}

#[derive(Debug)]
/// State retained while an L1 context awaits durable persistence.
struct PendingPersistence {
    /// Valid remote payload served while persistence is pending.
    payload: Value,
    /// Single-flight guard for the persistence retry.
    retrying: AtomicBool,
    /// L1 hits accumulated before persistence completes.
    hits: AtomicU64,
    /// Whether a retry has completed durable persistence.
    durable: AtomicBool,
}

/// Cancellation-safe ownership of one persistence retry.
struct PersistenceRetryLease<'a>(&'a AtomicBool);

impl Drop for PersistenceRetryLease<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Resolves a context URL to a context document.
///
/// Not `Clone`: wrap in `Arc` to share across tasks.
#[derive(Debug)]
pub struct HttpContextLoader {
    /// The HTTP client remote fetches go through.
    client: Client,
    /// The in-process cache in front of every other step.
    cache: Cache<IriBuf, LoadedContext>,
    /// Embedded and preloaded documents, available without network access.
    registry: Arc<ContextRegistry>,
    /// The caller's durable cache, when one is attached.
    l2_cache: Option<Arc<dyn ContextCache>>,
    /// Where clause 5.13.3.5 access counts accumulate, when one is attached.
    hit_counter: Option<Arc<HitCounter>>,
    /// Whether configuration permits a remote fetch at all.
    allow_remote: RemoteContextFetch,
    /// The size beyond which a remote document is refused.
    max_context_bytes: usize,
}

impl HttpContextLoader {
    /// Build a loader over the given registry.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::ClientBuild`] when the HTTP client cannot be
    /// built from the supplied timeout and redirect limit.
    pub(crate) fn new(
        registry: Arc<ContextRegistry>,
        allow_remote: RemoteContextFetch,
        timeout_seconds: u64,
        max_redirects: u32,
        cache_capacity: u64,
        cache_ttl_seconds: u64,
        max_context_bytes: usize,
    ) -> Result<HttpContextLoader, JsonLdError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .redirect(Policy::limited(max_redirects as usize))
            .build()
            .map_err(JsonLdError::ClientBuild)?;

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

    /// Attach the caller's durable context cache.
    pub(crate) fn with_l2_cache(mut self, cache: Arc<dyn ContextCache>) -> HttpContextLoader {
        self.l2_cache = Some(cache);
        self
    }

    /// Attach the shared hit counter.
    ///
    /// The same `Arc` must reach the periodic flush task, or the accumulated
    /// deltas never become durable.
    pub(crate) fn with_hit_counter(mut self, counter: Arc<HitCounter>) -> HttpContextLoader {
        self.hit_counter = Some(counter);
        self
    }

    /// Drop the in-process cache entry for a context URL.
    ///
    /// The durable L2 cache is the repository's lifecycle to manage (a delete or
    /// a reload writes it directly), so this invalidates only the moka layer the
    /// loader owns.
    pub async fn invalidate(&self, url: &IriBuf) {
        self.cache.invalidate(url).await;
    }

    /// Resolve a context URL through the full pipeline.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] variant describing which step failed and
    /// why.
    pub async fn load_value(&self, url: &IriBuf) -> Result<Arc<jsonld_syntax::Value>, JsonLdError> {
        self.load_value_raw(url).await.map_err(|error| JsonLdError::from(error.as_ref()))
    }

    /// [`Self::load_value`] yielding the raw [`LoaderError`].
    ///
    /// `Arc`-wrapped because `moka` requires a cached error to be
    /// `Clone + Send + Sync`. The [`Loader`] implementation needs this form,
    /// since the trait fixes the error type.
    pub(crate) async fn load_value_raw(&self, url: &IriBuf) -> Result<Arc<jsonld_syntax::Value>, Arc<LoaderError>> {
        if let Some(loaded) = self.cache.get(url).await {
            self.record_cached_access(url, &loaded).await;
            return Ok(loaded.value);
        }
        let url_iri = url.clone(); // owned: the moka key and the cache-fill future both need it
        let registry = Arc::clone(&self.registry); // clone: Arc ref-count bump into the cache-fill future
        let client = self.client.clone(); // clone: reqwest::Client is an Arc handle
        let allow_remote = self.allow_remote;
        let max_bytes = self.max_context_bytes;
        let l2_cache = self.l2_cache.clone(); // clone: Arc ref-count bump into the cache-fill future
        let initialized = Arc::new(AtomicBool::new(false));
        let initializer = Arc::clone(&initialized); // clone: the cache-fill future records whether this caller initialized the entry

        let result = self
            .cache
            .try_get_with(url_iri.clone(), async move {
                initializer.store(true, Ordering::Relaxed);
                if let Some(value) = registry.get(&url_iri) {
                    return Ok(LoadedContext {
                        value,
                        durability: ContextDurability::Registry,
                        cacheable: true,
                    });
                }

                if let Some(ref cache) = l2_cache {
                    match cache.get(&url_iri).await {
                        Ok(Some(payload)) => {
                            trace!("L2 cache hit for context: {}", url_iri);
                            let parsed = jsonld_syntax::Value::from_serde_json(payload);
                            return Ok(LoadedContext {
                                value: Arc::new(without_embedded_core_context(&url_iri, parsed)),
                                durability: ContextDurability::Durable,
                                cacheable: true,
                            });
                        }
                        Ok(None) => {}
                        Err(error) if allow_remote == RemoteContextFetch::Allow => {
                            warn!(url = %url_iri, error = %error, "Durable context-cache read failed; attempting remote resolution");
                        }
                        Err(error) => {
                            return Err(LoaderError {
                                url: url_iri.clone(),
                                kind: LoaderErrorKind::Cache(error.to_string()),
                            });
                        }
                    }
                }

                if allow_remote == RemoteContextFetch::Deny {
                    return Err(LoaderError::fetch_disabled(url_iri.clone()));
                }

                match fetch_and_parse(&client, &url_iri, max_bytes).await {
                    Ok(fetched) => {
                        let normalized = without_embedded_core_context(&url_iri, fetched.value);
                        if !fetched.cacheable {
                            return Ok(LoadedContext {
                                value: Arc::new(normalized),
                                durability: ContextDurability::Registry,
                                cacheable: false,
                            });
                        }
                        // Written to L2 only here, on the first remote-fetch
                        // success: the row carries no counter, because hit
                        // accounting is the HitCounter's job.
                        if let Some(ref cache) = l2_cache {
                            let serde_value = jsonld_syntax::Value::into_serde_json(normalized.clone()); // clone: the normalized document is also returned
                            let durability = match cache.put(&url_iri, &serde_value).await {
                                Ok(()) => {
                                    trace!("Persisted context to L2 cache: {}", url_iri);
                                    ContextDurability::Durable
                                }
                                Err(error) => {
                                    warn!(url = %url_iri, error = %error, "Serving remotely loaded context while durable persistence is pending");
                                    ContextDurability::Pending(Arc::new(PendingPersistence {
                                        payload: serde_value,
                                        retrying: AtomicBool::new(false),
                                        hits: AtomicU64::new(0),
                                        durable: AtomicBool::new(false),
                                    }))
                                }
                            };
                            return Ok(LoadedContext {
                                value: Arc::new(normalized),
                                durability,
                                cacheable: true,
                            });
                        }
                        Ok(LoadedContext {
                            value: Arc::new(normalized),
                            durability: ContextDurability::Registry,
                            cacheable: true,
                        })
                    }
                    Err(error) => match registry.get(&url_iri) {
                        Some(value) => Ok(LoadedContext {
                            value,
                            durability: ContextDurability::Registry,
                            cacheable: true,
                        }),
                        None => Err(error),
                    },
                }
            })
            .await;

        let loaded = result?;
        if !loaded.cacheable {
            self.cache.invalidate(url).await;
        }
        if !initialized.load(Ordering::Relaxed) {
            self.record_cached_access(url, &loaded).await;
        } else if matches!(loaded.durability, ContextDurability::Durable) {
            // A durable L2 read is a cache hit; the first successful remote
            // fetch and persistence is not.
            if let Some(ref counter) = self.hit_counter {
                counter.bump(url);
            }
        }
        Ok(loaded.value)
    }

    /// Record one L1 access and retry pending persistence when necessary.
    async fn record_cached_access(&self, url: &IriBuf, loaded: &LoadedContext) {
        match &loaded.durability {
            ContextDurability::Registry => {}
            ContextDurability::Durable => {
                if let Some(ref counter) = self.hit_counter {
                    counter.bump(url);
                }
            }
            ContextDurability::Pending(pending) if pending.durable.load(Ordering::Acquire) => {
                if let Some(ref counter) = self.hit_counter {
                    counter.bump(url);
                }
            }
            ContextDurability::Pending(pending) => {
                pending.hits.fetch_add(1, Ordering::Relaxed);
                if pending.retrying.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                    return;
                }
                let _lease = PersistenceRetryLease(&pending.retrying);
                let Some(ref cache) = self.l2_cache else {
                    return;
                };
                match cache.put(url, &pending.payload).await {
                    Ok(()) => {
                        pending.durable.store(true, Ordering::Release);
                        let count = pending.hits.swap(0, Ordering::AcqRel);
                        if count > 0
                            && let Some(ref counter) = self.hit_counter
                        {
                            counter.requeue([HitDelta { url: url.clone(), count }]);
                        }
                    }
                    Err(error) => warn!(url = %url, error = %error, "Durable context-cache persistence retry failed"),
                }
            }
        }
    }

    /// Fetch a context from its source, bypassing both caches.
    ///
    /// Used when the caller needs to know the original source is still
    /// reachable, as delete-with-reload does.
    ///
    /// # Errors
    ///
    /// Returns a [`LoaderError`] when remote fetch is disabled or the request
    /// fails.
    pub(crate) async fn fetch_remote_value(&self, url: &IriBuf) -> Result<Arc<jsonld_syntax::Value>, LoaderError> {
        if self.allow_remote == RemoteContextFetch::Deny {
            return Err(LoaderError::fetch_disabled(url.clone()));
        }
        let fetched = fetch_and_parse(&self.client, url, self.max_context_bytes).await?;
        Ok(Arc::new(without_embedded_core_context(url, fetched.value)))
    }
}

impl Loader for HttpContextLoader {
    type Error = LoaderError;

    async fn load(&self, url: Iri<&str>) -> Result<RemoteDocument<IriBuf>, LoadError<Self::Error>> {
        let url_iri = IriBuf::from(url);
        let value = self.load_value_raw(&url_iri).await.map_err(|error| {
            LoadError::new(url_iri.clone(), (*error).clone()) // clone: the Arc is shared with the moka cache
        })?;
        Ok(RemoteDocument::new(Some(url_iri), Some(ld_json_media_type()), (*value).clone())) // clone: the document is shared with the moka cache
    }
}

/// The media type a [`RemoteDocument`] reports for a loaded context.
fn ld_json_media_type() -> MediaTypeBuf {
    jsonld_core::LD_JSON_MEDIA_TYPE.into()
}

/// Fetch a context over HTTP and parse it, refusing anything that is not
/// JSON-shaped or is larger than the configured limit.
async fn fetch_and_parse(client: &Client, url: &IriBuf, max_bytes: usize) -> Result<FetchedContext, LoaderError> {
    let mut request_headers = reqwest::header::HeaderMap::new();
    request_headers.typed_insert(JsonLdAccept);
    let response = client.get(url.as_str()).headers(request_headers).send().await.map_err(|error| LoaderError {
        url: url.clone(),
        kind: LoaderErrorKind::Request(error.to_string()),
    })?;

    if !response.status().is_success() {
        return Err(LoaderError {
            url: url.clone(),
            kind: LoaderErrorKind::Http(response.status()),
        });
    }

    if let Some(content_type) = response.headers().typed_get::<headers::ContentType>()
        && !is_json_like(&content_type.to_string())
    {
        return Err(LoaderError {
            url: url.clone(),
            kind: LoaderErrorKind::Invalid(format!("Unsupported context content type: {content_type}")),
        });
    }

    // Checked twice on purpose: the advertised length rejects an oversized
    // body before it is read, and the measured length catches a server that
    // did not advertise one.
    if let Some(len) = response.content_length()
        && len as usize > max_bytes
    {
        return Err(too_large(url.clone(), len as usize, max_bytes));
    }

    let cacheable = response
        .headers()
        .typed_get::<headers::CacheControl>()
        .is_none_or(|value| !value.no_store() && !value.no_cache() && value.max_age() != Some(Duration::ZERO));
    let bytes = response.bytes().await.map_err(|error| LoaderError {
        url: url.clone(),
        kind: LoaderErrorKind::Request(error.to_string()),
    })?;

    if bytes.len() > max_bytes {
        return Err(too_large(url.clone(), bytes.len(), max_bytes));
    }

    let text = std::str::from_utf8(&bytes).map_err(|error| LoaderError {
        url: url.clone(),
        kind: LoaderErrorKind::Invalid(error.to_string()),
    })?;

    let json = serde_json::from_str::<Value>(text).map_err(|error| LoaderError {
        url: url.clone(),
        kind: LoaderErrorKind::Invalid(error.to_string()),
    })?;
    if !json.as_object().is_some_and(|object| object.contains_key("@context")) {
        return Err(LoaderError {
            url: url.clone(),
            kind: LoaderErrorKind::Invalid("remote document has no usable top-level @context".to_owned()),
        });
    }
    let (value, _) = jsonld_syntax::Value::parse_str(text).map_err(|error| LoaderError {
        url: url.clone(),
        kind: LoaderErrorKind::Invalid(error.to_string()),
    })?;
    Ok(FetchedContext { value, cacheable })
}

/// Whether one Cache-Control field permits retaining the remote context.
#[cfg(test)]
fn cache_control_allows_storage(value: &str) -> bool {
    !value.split(',').map(str::trim).any(|directive| {
        directive.eq_ignore_ascii_case("no-store")
            || directive.eq_ignore_ascii_case("no-cache")
            || directive
                .split_once('=')
                .is_some_and(|(name, seconds)| name.trim().eq_ignore_ascii_case("max-age") && seconds.trim() == "0")
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
        context_registry::ContextRegistry,
        engine::options::RemoteContextFetch,
        loader::http::{HttpContextLoader, cache_control_allows_storage, is_json_like},
    };
    use iri_rs::IriBuf;
    use jsonld::Loader;
    use serde_json::json;
    use std::sync::Arc;

    fn registry_with(url: &str, context: serde_json::Value) -> Arc<ContextRegistry> {
        let registry = Arc::new(ContextRegistry::new());
        registry.register(IriBuf::new(url.to_string()).expect("valid URL"), jsonld_syntax::Value::from_serde_json(context));
        registry
    }

    fn offline_loader(registry: Arc<ContextRegistry>) -> HttpContextLoader {
        HttpContextLoader::new(registry, RemoteContextFetch::Deny, 30, 5, 1024, 3600, 2_000_000).expect("loader builds")
    }

    /// An L2 cache that always hands back one stored payload.
    #[derive(Debug)]
    struct FixedL2(serde_json::Value);

    #[async_trait::async_trait]
    impl crate::context_cache::ContextCache for FixedL2 {
        async fn get(&self, _url: &IriBuf) -> Result<Option<serde_json::Value>, crate::context_cache::ContextCacheError> {
            Ok(Some(self.0.clone()))
        }
        async fn put(&self, _url: &IriBuf, _payload: &serde_json::Value) -> Result<(), crate::context_cache::ContextCacheError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn an_l2_cache_hit_loads_without_a_spurious_invalid_context_error() {
        // A stored payload is converted straight through `from_serde_json`; the
        // old `to_string`/`parse_str` round trip fell back to an empty string on
        // a (serde-impossible) failure and then reported an unrelated "invalid
        // context" error. Even with remote fetching denied, an L2 hit resolves.
        let l2 = Arc::new(FixedL2(json!({ "@context": { "name": "https://example.com/name" } })));
        let loader = offline_loader(Arc::new(ContextRegistry::new())).with_l2_cache(l2);

        let url = IriBuf::new("https://example.com/from-l2.jsonld".to_string()).expect("valid URL");
        let value = loader.load_value(&url).await.expect("an L2 hit resolves");

        let json = jsonld_syntax::Value::into_serde_json((*value).clone());
        assert!(json.is_object() || json.is_array(), "expected a parsed context document, got {json:?}");
    }

    #[tokio::test]
    async fn a_registered_context_resolves_with_remote_fetch_denied() {
        let registry = registry_with(
            "https://example.com/context.jsonld",
            json!({ "@context": { "name": "https://example.com/name" } }),
        );
        let loader = offline_loader(registry);
        let iri = IriBuf::new("https://example.com/context.jsonld".to_string()).expect("valid IRI");

        let document = loader.load(iri.as_ref()).await.expect("registry hit");

        assert!(document.document.is_object());
    }

    #[tokio::test]
    async fn an_unregistered_context_reports_that_fetching_is_disabled() {
        let loader = offline_loader(Arc::new(ContextRegistry::new()));

        let url = IriBuf::new("https://example.com/unknown.jsonld".to_string()).expect("valid URL");
        let error = loader.load_value(&url).await.expect_err("no registry entry");

        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
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
    fn cache_control_directives_that_require_revalidation_are_not_stored() {
        for value in ["no-store", "NO-CACHE", "public, max-age=0"] {
            assert!(!cache_control_allows_storage(value), "{value} must disable storage");
        }
        for value in ["public, max-age=60", "immutable"] {
            assert!(cache_control_allows_storage(value), "{value} permits storage");
        }
    }
}
