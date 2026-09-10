//! The four-step context resolution the JSON-LD processor drives.
//!
//! A URL is looked for in the in-process moka cache, then in the registry of
//! embedded and preloaded documents, then in the caller-supplied L2 cache,
//! and only then over HTTP. The L2 write happens once per URL, on the first
//! remote-fetch success.
//!
//! The in-process cache has no cache-wide time-to-live. Clause 6.3.16 makes the
//! `Expires` field and the `max-age`/`s-maxage` directives of a downloaded
//! `@context` authoritative over how long it may be reused, so each entry
//! carries its own lifetime and
//! [`ResponseDrivenExpiry`](crate::loader::loaded_context::ResponseDrivenExpiry)
//! applies it; the configured TTL is only the heuristic used when a response
//! states none.
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
    loader::{
        cache_lifetime::CacheLifetime,
        durable_cache::{fetch_and_persist, load_from_durable_cache},
        error::LoaderError,
        loaded_context::{ContextDurability, EntryLifetime, LoadedContext, ResponseDrivenExpiry, registry_context},
        pending_persistence::PersistenceRetryLease,
        remote_document::{fetch_and_parse, ld_json_media_type},
    },
};
use iri_rs::{Iri, IriBuf};
use jsonld::{LoadError, Loader, RemoteDocument};
use moka::future::Cache;
use reqwest::{Client, redirect::Policy};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tracing::warn;

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
    /// The lifetime assigned to a response that states no expiration time
    /// (clause 6.3.16).
    default_lifetime: CacheLifetime,
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
        default_lifetime: CacheLifetime,
        max_context_bytes: usize,
    ) -> Result<HttpContextLoader, JsonLdError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .redirect(Policy::limited(max_redirects as usize))
            .build()
            .map_err(JsonLdError::ClientBuild)?;

        // No `time_to_live`: a cache-wide bound would cut a `max-age` the origin
        // server granted short of what clause 6.3.16 requires be honoured.
        let cache = Cache::builder().max_capacity(cache_capacity).expire_after(ResponseDrivenExpiry).build();

        Ok(HttpContextLoader {
            client,
            cache,
            registry,
            l2_cache: None,
            hit_counter: None,
            allow_remote,
            default_lifetime,
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
        let default_lifetime = self.default_lifetime;
        let max_bytes = self.max_context_bytes;
        let l2_cache = self.l2_cache.clone(); // clone: Arc ref-count bump into the cache-fill future
        let filled_here = Arc::new(AtomicBool::new(false));
        let fill_marker = Arc::clone(&filled_here); // clone: the cache-fill future records whether this caller filled the entry

        let result = self
            .cache
            .try_get_with(url_iri.clone(), async move {
                fill_marker.store(true, Ordering::Relaxed);
                if let Some(value) = registry.get(&url_iri) {
                    return Ok(registry_context(value));
                }

                let durable = match l2_cache {
                    Some(ref cache) => load_from_durable_cache(cache.as_ref(), &url_iri, allow_remote).await?,
                    None => None,
                };
                if let Some(loaded) = durable {
                    return Ok(loaded);
                }

                if allow_remote == RemoteContextFetch::Deny {
                    return Err(LoaderError::fetch_disabled(url_iri.clone()));
                }

                // A registry entry is preferable to a failed expansion when the
                // source has become unreachable.
                fetch_and_persist(&client, &url_iri, max_bytes, default_lifetime, l2_cache.as_ref())
                    .await
                    .or_else(|unreachable_source| registry.get(&url_iri).map(registry_context).ok_or(unreachable_source))
            })
            .await;

        let loaded = result?;
        if matches!(loaded.lifetime, EntryLifetime::Immediate) {
            // The per-entry expiry lapses it too, but only once housekeeping
            // runs; a response that forbade storage gets the entry removed now.
            self.cache.invalidate(url).await;
        }
        if !filled_here.load(Ordering::Relaxed) {
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

    /// Record one in-process access and retry pending persistence when necessary.
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
                match cache.put(url, &pending.payload, pending.lifetime).await {
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
        let fetched = fetch_and_parse(&self.client, url, self.max_context_bytes, self.default_lifetime).await?;
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

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::{CachedContext, ContextCache, ContextCacheError},
        context_registry::ContextRegistry,
        engine::options::RemoteContextFetch,
        loader::{
            cache_lifetime::{CacheLifetime, ResidualLifetime},
            http::HttpContextLoader,
        },
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
        HttpContextLoader::new(registry, RemoteContextFetch::Deny, 30, 5, 1024, CacheLifetime::DEFAULT, 2_000_000).expect("loader builds")
    }

    /// An L2 cache that always hands back one stored payload, with the span the
    /// row has left before it lapses.
    #[derive(Debug)]
    struct FixedL2 {
        /// The stored `@context` document.
        payload: serde_json::Value,
        /// What the fixture reports is left of the row's lifetime.
        residual: ResidualLifetime,
    }

    #[async_trait::async_trait]
    impl ContextCache for FixedL2 {
        async fn get(&self, _url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
            Ok(Some(CachedContext {
                payload: self.payload.clone(), // clone: the fixture keeps its payload for the next read
                residual: self.residual,
            }))
        }
        async fn put(&self, _url: &IriBuf, _payload: &serde_json::Value, _lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn an_l2_cache_hit_loads_without_a_spurious_invalid_context_error() {
        // A stored payload is converted straight through `from_serde_json`; the
        // old `to_string`/`parse_str` round trip fell back to an empty string on
        // a (serde-impossible) failure and then reported an unrelated "invalid
        // context" error. Even with remote fetching denied, an L2 hit resolves.
        let l2 = Arc::new(FixedL2 {
            payload: json!({ "@context": { "name": "https://example.com/name" } }),
            residual: ResidualLifetime::Bounded(CacheLifetime::from_seconds(30).expect("thirty seconds is a positive lifetime")),
        });
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
}
