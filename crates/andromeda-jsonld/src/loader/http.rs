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
//! A reference to one of the tenant's own stored `@context`s is resolved from
//! storage first, and which resolutions count toward clause 5.13.3.5's
//! `numberOfHits` is decided beside the pipeline.

use crate::{
    context_cache::{ContextCache, DownloadedContext},
    context_registry::ContextRegistry,
    context_tail::ContextTail,
    engine::{context_document::ContextDocument, options::RemoteContextFetch},
    error::JsonLdError,
    hit_counter::HitCounter,
    hosted_context_recognizer::{HostedContextRecognizer, HostedReference},
    loader::{
        cache_lifetime::CacheLifetime,
        context_lifetimes::ContextLifetimes,
        context_sources::note_source,
        durable_cache::{fetch_and_persist, load_from_durable_cache, persist_registry_context},
        error::{LoaderError, LoaderErrorKind},
        loaded_context::{ContextProvenance, EntryLifetime, LoadedContext, ResponseDrivenExpiry, registry_context},
        loader_settings::LoaderSettings,
        own_context::{CachedCopy, StoredLookup},
        remote_document::fetch_and_parse,
    },
};
use iri_rs::{Iri, IriBuf};
use jsonld::{LoadError, Loader, RemoteDocument};
use moka::future::Cache;
use reqwest::{Client, redirect::Policy};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Resolves a context URL to a context document.
///
/// Not `Clone`: wrap in `Arc` to share across tasks.
#[derive(Debug)]
pub struct HttpContextLoader {
    /// The HTTP client remote fetches go through.
    client: Client,
    /// The in-process cache in front of every other step.
    pub(super) cache: Cache<IriBuf, LoadedContext>,
    /// Embedded and preloaded documents, available without network access.
    registry: Arc<ContextRegistry>,
    /// The caller's durable cache, when one is attached.
    pub(super) l2_cache: Option<Arc<dyn ContextCache>>,
    /// Where clause 5.13.3.5 access counts accumulate, when one is attached.
    pub(super) hit_counter: Option<Arc<HitCounter>>,
    /// Whether configuration permits a remote fetch at all.
    pub(super) allow_remote: RemoteContextFetch,
    /// The lifetimes assigned where no response states one: the heuristic for
    /// a download (clause 6.3.16) and the revalidation span for a document the
    /// broker stores itself.
    pub(super) lifetimes: ContextLifetimes,
    /// The size beyond which a remote document is refused.
    max_context_bytes: usize,
    /// Recognises references to the tenant's own stored `@context`s: the
    /// tenant's recogniser for a broker engine, one that recognises nothing for
    /// an engine no tenant stands behind.
    hosted: Arc<dyn HostedContextRecognizer>,
}

/// How one reference is resolved, as the tenant's recogniser classifies it.
enum Resolution {
    /// A stored `@context` of the tenant, looked for under its identifier.
    Stored(IriBuf),
    /// Any other reference, resolved as the URL it is.
    Url,
}

impl HttpContextLoader {
    /// Build a loader over the given registry, recognising the tenant's own
    /// stored `@context`s with `hosted`.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::ClientBuild`] when the HTTP client cannot be
    /// built from the supplied timeout and redirect limit.
    pub(crate) fn new(
        registry: Arc<ContextRegistry>,
        settings: LoaderSettings,
        hosted: Arc<dyn HostedContextRecognizer>,
    ) -> Result<HttpContextLoader, JsonLdError> {
        let LoaderSettings {
            allow_remote,
            timeout,
            max_redirects,
            cache_capacity,
            lifetimes,
            max_context_bytes,
        } = settings;
        let client = Client::builder()
            .timeout(timeout)
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
            lifetimes,
            max_context_bytes,
            hosted,
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

    /// The recogniser of the tenant's own stored `@context`s.
    pub(crate) fn hosted_contexts(&self) -> &dyn HostedContextRecognizer {
        self.hosted.as_ref()
    }

    /// Drop the in-process cache entry for a context URL.
    ///
    /// The durable L2 cache is the repository's lifecycle to manage (a delete or
    /// a reload writes it directly), so this invalidates only the moka layer the
    /// loader owns.
    pub async fn invalidate(&self, url: &IriBuf) {
        self.cache.invalidate(url).await;
    }

    /// How long a document the broker stores itself is used from memory
    /// before its row is read again.
    pub(crate) const fn hosted_context_revalidation(&self) -> CacheLifetime {
        self.lifetimes.hosted_revalidation
    }

    /// The document the in-process cache holds for `url` right now, without
    /// resolving it and without counting an access.
    ///
    /// A copy derived from a document (a parsed term table) compares its source
    /// against this to tell whether the document has since lapsed and been read
    /// again: a changed or absent entry means the derived copy is stale. The
    /// entry is looked for under the key a resolution of `url` lands under -- a
    /// stored `@context` of the tenant under its identifier -- so the derived
    /// copy of an own context is reused like any other.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] a recognised identifier that is not an IRI
    /// is reported with.
    pub(crate) async fn held_document(&self, url: &IriBuf) -> Result<Option<Arc<ContextDocument>>, JsonLdError> {
        let held = match self.resolution_of(url).map_err(|error| JsonLdError::from(error.as_ref()))? {
            Resolution::Stored(identifier) => match self.cache.get(&identifier).await {
                Some(stored) if stored.provenance == ContextProvenance::Broker => Some(stored),
                // An own-shaped reference the tenant does not hold is resolved
                // as the URL it is, so its entry is under that URL.
                Some(_) | None => self.cache.get(url).await,
            },
            Resolution::Url => self.cache.get(url).await,
        };
        Ok(held.map(|loaded| loaded.document))
    }

    /// Resolve a context URL through the full pipeline.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] variant describing which step failed and
    /// why.
    pub async fn load_document(&self, url: &IriBuf) -> Result<Arc<ContextDocument>, JsonLdError> {
        self.load_document_raw(url).await.map_err(|error| JsonLdError::from(error.as_ref()))
    }

    /// [`Self::load_document`] yielding the raw [`LoaderError`].
    ///
    /// `Arc`-wrapped because `moka` requires a cached error to be
    /// `Clone + Send + Sync`. The [`Loader`] implementation needs this form,
    /// since the trait fixes the error type.
    pub(crate) async fn load_document_raw(&self, url: &IriBuf) -> Result<Arc<ContextDocument>, Arc<LoaderError>> {
        self.load_entry(url).await.map(|loaded| loaded.document)
    }

    /// What the context document at `url` leaves in force last (clause 4.4),
    /// resolved through the full pipeline exactly as processing resolves it.
    ///
    /// A response names what the request's contexts leave in force last, and
    /// that has to be the same answer whichever tier holds the document at the
    /// time: an entry evicted between processing and rendering is resolved
    /// again, not taken as unknown. The access falls inside the operation's hit
    /// scope ([`crate::operation_hit_scope`]), so it is not counted twice.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] the document's resolution fails with, which
    /// is `LdContextNotAvailable` for a document that cannot be loaded (clause
    /// 5.5.6), exactly as processing would have reported it.
    pub(crate) async fn load_tail(&self, url: &IriBuf) -> Result<ContextTail, JsonLdError> {
        self.load_entry(url)
            .await
            .map(|loaded| loaded.tail_from(url))
            .map_err(|error| JsonLdError::from(error.as_ref()))
    }

    /// How a resolution of `requested` proceeds, as the tenant's recogniser
    /// classifies it.
    ///
    /// # Errors
    ///
    /// Returns a [`LoaderErrorKind::StoredIdentifier`] failure when a
    /// recognised identifier is not an IRI, which the lexicon proves never
    /// happens for a `urn:ngsi-ld:JsonLdContext:` identifier.
    fn resolution_of(&self, requested: &IriBuf) -> Result<Resolution, Arc<LoaderError>> {
        match self.hosted.recognize(requested.as_iri_ref()) {
            HostedReference::Own(identifier) => identifier.to_iri().map(Resolution::Stored).map_err(|not_an_iri| {
                Arc::new(LoaderError {
                    url: requested.clone(), // clone: the failure reports the URL the request named
                    kind: LoaderErrorKind::StoredIdentifier(Arc::new(not_an_iri)),
                })
            }),
            HostedReference::Unrelated => Ok(Resolution::Url),
        }
    }

    /// Resolve a context URL to the in-process entry the pipeline holds for it.
    ///
    /// A reference to one of the tenant's own stored `@context`s that the
    /// tenant holds is resolved from storage by its identifier (clause 5.13.1
    /// serves it "on-demand"), never fetched over HTTP from this broker. One
    /// the tenant does not hold may be another broker's, so it is resolved as
    /// the URL it is, but its download is never recorded as a "Cached" copy:
    /// the broker holds no Cached row for a URL shaped like its own.
    async fn load_entry(&self, requested: &IriBuf) -> Result<LoadedContext, Arc<LoaderError>> {
        let loaded = match self.resolution_of(requested)? {
            Resolution::Stored(identifier) => match self.load_stored(&identifier).await? {
                StoredLookup::Held(loaded) => *loaded,
                StoredLookup::NotHeld => self.load_url(requested, CachedCopy::Forbidden).await?,
            },
            Resolution::Url => self.load_url(requested, CachedCopy::Recorded).await?,
        };
        note_source(requested, &loaded.document);
        Ok(loaded)
    }

    /// Resolve `url` through the in-process cache, the registry, the durable
    /// cache and the network, in that order.
    ///
    /// With [`CachedCopy::Forbidden`] the durable cache is neither read nor
    /// written, so the resolution leaves no "Cached" row behind.
    async fn load_url(&self, url: &IriBuf, cached_copy: CachedCopy) -> Result<LoadedContext, Arc<LoaderError>> {
        if let Some(loaded) = self.cache.get(url).await {
            self.record_cached_access(url, &loaded).await;
            return Ok(loaded);
        }
        let url_iri = url.clone(); // owned: the moka key and the cache-fill future both need it
        let registry = Arc::clone(&self.registry); // clone: Arc ref-count bump into the cache-fill future
        let client = self.client.clone(); // clone: reqwest::Client is an Arc handle
        let allow_remote = self.allow_remote;
        let lifetimes = self.lifetimes;
        let max_bytes = self.max_context_bytes;
        let l2_cache = match cached_copy {
            CachedCopy::Recorded => self.l2_cache.clone(), // clone: Arc ref-count bump into the cache-fill future
            CachedCopy::Forbidden => None,
        };
        let filled_here = Arc::new(AtomicBool::new(false));
        let fill_marker = Arc::clone(&filled_here); // clone: the cache-fill future records whether this caller filled the entry

        let result = self
            .cache
            .try_get_with(url_iri.clone(), async move {
                fill_marker.store(true, Ordering::Relaxed);
                if let Some(registered) = registry.get(&url_iri) {
                    // Clause 5.13.1: a preloaded @context is a local copy of a
                    // document whose authority is a URL, so the first resolution
                    // that reaches it records a "Cached" durable entry that
                    // clause 5.13.3 then lists. An embedded @context (the core
                    // contexts, the data-integrity stub) is registered by a path
                    // that never marks it preloaded, so it stays out of the cache.
                    if let Some(as_written) = registry.preloaded_document(&url_iri)
                        && let Some(ref cache) = l2_cache
                    {
                        return Ok(persist_registry_context(cache.as_ref(), &url_iri, registered, &as_written, lifetimes.heuristic).await);
                    }
                    return Ok(registry_context(registered));
                }

                let durable = match l2_cache {
                    Some(ref cache) => load_from_durable_cache(cache.as_ref(), &url_iri, allow_remote, lifetimes.hosted_revalidation).await?,
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
                fetch_and_persist(&client, &url_iri, max_bytes, lifetimes.heuristic, l2_cache.as_ref())
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
        self.settle_access(url, &loaded, &filled_here).await;
        Ok(loaded)
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
    pub(crate) async fn fetch_remote_value(&self, url: &IriBuf) -> Result<DownloadedContext, LoaderError> {
        if self.allow_remote == RemoteContextFetch::Deny {
            return Err(LoaderError::fetch_disabled(url.clone()));
        }
        let fetched = fetch_and_parse(&self.client, url, self.max_context_bytes, self.lifetimes.heuristic).await?;
        Ok(DownloadedContext {
            document: fetched.value,
            document_url: fetched.document_url,
        })
    }
}

impl Loader for HttpContextLoader {
    type Error = LoaderError;

    async fn load(&self, url: Iri<&str>) -> Result<RemoteDocument<IriBuf>, LoadError<Self::Error>> {
        let url_iri = IriBuf::from(url);
        let loaded = self.load_entry(&url_iri).await.map_err(|error| {
            LoadError::new(url_iri.clone(), (*error).clone()) // clone: the Arc is shared with the moka cache
        })?;
        // The document's base is the URL it was finally read from, after
        // redirects and an alternate hop; a copy held without one is based on
        // the URL the request named.
        let base = loaded.document_url(&url_iri).clone(); // clone: the remote document owns its URL
        Ok(loaded.document.remote_document(base))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_cache::{CachedContext, ContextCache, ContextCacheError, StoredDocumentSource},
        context_registry::ContextRegistry,
        context_tail::ContextTail,
        hit_counter::HitCounter,
        hosted_context_recognizer::NoHostedContexts,
        loader::{
            cache_lifetime::{CacheLifetime, ResidualLifetime},
            http::HttpContextLoader,
            loader_settings::LoaderSettings,
        },
        operation_hit_scope::run_in_operation_scope,
        test_documents::{document_value, literal},
    };
    use andromeda_json_literal::JsonLiteral;
    use iri_rs::IriBuf;
    use jsonld::Loader;
    use serde_json::json;
    use std::sync::{
        Arc,
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    fn registry_with(url: &str, context: &serde_json::Value) -> Arc<ContextRegistry> {
        let registry = Arc::new(ContextRegistry::new());
        registry
            .register(IriBuf::new(url.to_string()).expect("valid URL"), &literal(context))
            .expect("registered");
        registry
    }

    fn offline_loader(registry: Arc<ContextRegistry>) -> HttpContextLoader {
        HttpContextLoader::new(registry, LoaderSettings::offline(), Arc::new(NoHostedContexts)).expect("loader builds")
    }

    /// An L2 cache that always hands back one stored payload, with the span the
    /// row has left before it lapses.
    #[derive(Debug)]
    struct FixedL2 {
        /// The stored `@context` document.
        payload: JsonLiteral,
        /// What the fixture reports is left of the row's lifetime.
        residual: ResidualLifetime,
    }

    #[async_trait::async_trait]
    impl ContextCache for FixedL2 {
        async fn get(&self, url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
            // A row with no expiry is one the broker stores itself; any other
            // is a download of the URL it is read under.
            let source = match self.residual {
                ResidualLifetime::Unbounded => StoredDocumentSource::Broker,
                ResidualLifetime::Bounded(_) => StoredDocumentSource::Downloaded { document_url: url.clone() }, // clone: the hit owns its document URL
            };
            Ok(Some(CachedContext {
                payload: self.payload.clone(), // clone: the fixture keeps its payload for the next read
                residual: self.residual,
                canonical_url: None,
                source,
            }))
        }
        async fn put(&self, _url: &IriBuf, _document_url: &IriBuf, _payload: &JsonLiteral, _lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
            Ok(())
        }
    }

    /// An L2 cache that counts its reads and holds, for a URL it was given, one
    /// downloaded row of that URL; any other URL is a miss.
    #[derive(Debug)]
    struct CountingL2 {
        /// The URL the row is stored under.
        url: IriBuf,
        /// The stored `@context` document.
        payload: JsonLiteral,
        /// How many times the cache was read.
        reads: AtomicUsize,
    }

    #[async_trait::async_trait]
    impl ContextCache for CountingL2 {
        async fn get(&self, url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            Ok((url == &self.url).then(|| CachedContext {
                payload: self.payload.clone(), // clone: the fixture keeps its payload for the next read
                residual: ResidualLifetime::Bounded(CacheLifetime::from_seconds(30).expect("thirty seconds is a positive lifetime")),
                canonical_url: None,
                source: StoredDocumentSource::Downloaded {
                    document_url: url.clone(), // clone: the hit owns its document URL
                },
            }))
        }
        async fn put(&self, _url: &IriBuf, _document_url: &IriBuf, _payload: &JsonLiteral, _lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
            Ok(())
        }
    }

    /// A loader over a [`CountingL2`] holding `url`, counting hits.
    fn counting_loader(url: &IriBuf) -> (HttpContextLoader, Arc<CountingL2>, Arc<HitCounter>) {
        let l2 = Arc::new(CountingL2 {
            url: url.clone(), // clone: the fixture owns the URL it answers for
            payload: literal(&json!({ "@context": "https://example.com/terms.jsonld" })),
            reads: AtomicUsize::new(0),
        });
        let counter = Arc::new(HitCounter::new());
        let loader = offline_loader(Arc::new(ContextRegistry::new()))
            .with_l2_cache(Arc::clone(&l2) as Arc<dyn ContextCache>)
            .with_hit_counter(Arc::clone(&counter));
        (loader, l2, counter)
    }

    /// A response names what the request's contexts leave in force last
    /// whichever tier holds them: an entry evicted between processing and
    /// rendering is resolved again, and the operation counts the context once
    /// (clause 5.13.3.5).
    #[tokio::test]
    async fn an_evicted_entry_is_resolved_again_for_its_tail_and_counted_once_in_the_operation() {
        let url = IriBuf::new("https://example.com/wrapper.jsonld".to_owned()).expect("valid URL");
        let (loader, l2, counter) = counting_loader(&url);

        let tail = run_in_operation_scope(async {
            loader.load_document(&url).await.expect("processing resolves the document");
            loader.invalidate(&url).await;
            loader.load_tail(&url).await
        })
        .await
        .expect("the evicted entry is resolved again");

        pretty_assertions::assert_eq!(
            tail,
            ContextTail::Reference(IriBuf::new("https://example.com/terms.jsonld".to_owned()).expect("valid URL"))
        );
        pretty_assertions::assert_eq!(l2.reads.load(Ordering::Relaxed), 2, "the evicted entry is read from the durable cache again");
        pretty_assertions::assert_eq!(counter.pending(&url), 1, "one operation counts the context once");
    }

    /// Clause 5.5.6: a document whose tail cannot be read because it cannot be
    /// loaded is `LdContextNotAvailable`, as processing would have reported.
    #[tokio::test]
    async fn a_tail_that_cannot_be_loaded_is_a_context_not_available() {
        let held = IriBuf::new("https://example.com/wrapper.jsonld".to_owned()).expect("valid URL");
        let missing = IriBuf::new("https://example.com/missing.jsonld".to_owned()).expect("valid URL");
        let (loader, l2, counter) = counting_loader(&held);

        let error = loader.load_tail(&missing).await.expect_err("the document cannot be loaded");

        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
        pretty_assertions::assert_eq!(l2.reads.load(Ordering::Relaxed), 1, "the durable cache was consulted once");
        pretty_assertions::assert_eq!(counter.pending(&missing), 0, "a failed resolution is no hit");
    }

    #[tokio::test]
    async fn an_l2_cache_hit_loads_without_a_spurious_invalid_context_error() {
        // A stored payload converts straight to the processor's form, so an L2
        // hit resolves even with remote fetching denied and reports no
        // "invalid context" error.
        let l2 = Arc::new(FixedL2 {
            payload: literal(&json!({ "@context": { "name": "https://example.com/name" } })),
            residual: ResidualLifetime::Bounded(CacheLifetime::from_seconds(30).expect("thirty seconds is a positive lifetime")),
        });
        let loader = offline_loader(Arc::new(ContextRegistry::new())).with_l2_cache(l2);

        let url = IriBuf::new("https://example.com/from-l2.jsonld".to_string()).expect("valid URL");
        let value = loader.load_document(&url).await.expect("an L2 hit resolves");

        let json = document_value(&value);
        assert!(json.is_object() || json.is_array(), "expected a parsed context document, got {json:?}");
    }

    #[tokio::test]
    async fn a_registered_context_resolves_with_remote_fetch_denied() {
        let registry = registry_with(
            "https://example.com/context.jsonld",
            &json!({ "@context": { "name": "https://example.com/name" } }),
        );
        let loader = offline_loader(registry);
        let iri = IriBuf::new("https://example.com/context.jsonld".to_string()).expect("valid IRI");

        let document = loader.load(iri.as_ref()).await.expect("registry hit");

        assert!(document.document().is_object());
    }

    #[tokio::test]
    async fn an_unregistered_context_reports_that_fetching_is_disabled() {
        let loader = offline_loader(Arc::new(ContextRegistry::new()));

        let url = IriBuf::new("https://example.com/unknown.jsonld".to_string()).expect("valid URL");
        let error = loader.load_document(&url).await.expect_err("no registry entry");

        assert!(error.is_context_not_available(), "expected a loading failure, got {error}");
    }

    /// An L2 cache that records the URLs written to it and never reports a hit,
    /// so a resolution proceeds past it to the registry.
    #[derive(Debug, Default)]
    struct RecordingL2 {
        /// The URLs handed to `put`, in call order.
        puts: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl ContextCache for RecordingL2 {
        async fn get(&self, _url: &IriBuf) -> Result<Option<CachedContext>, ContextCacheError> {
            Ok(None)
        }
        async fn put(&self, url: &IriBuf, _document_url: &IriBuf, _payload: &JsonLiteral, _lifetime: CacheLifetime) -> Result<(), ContextCacheError> {
            self.puts.lock().expect("mutex not poisoned").push(url.to_string());
            Ok(())
        }
    }

    /// Clause 5.13.1 / 5.13.3: resolving a preloaded @context records it in the
    /// durable cache as a "Cached" entry, so the listing can serve it.
    #[tokio::test]
    async fn a_preloaded_context_is_recorded_in_the_durable_cache() {
        let registry = Arc::new(ContextRegistry::new());
        let url = IriBuf::new("https://forge.example.com/suite.jsonld".to_string()).expect("valid URL");
        registry
            .register_preloaded(url.clone(), literal(&json!({ "@context": { "name": "https://example.com/name" } })))
            .expect("registered");
        let recorder = Arc::new(RecordingL2::default());
        let loader = offline_loader(registry).with_l2_cache(Arc::clone(&recorder) as Arc<dyn ContextCache>);

        loader.load_document(&url).await.expect("a preloaded context resolves");

        assert_eq!(
            recorder.puts.lock().expect("mutex not poisoned").as_slice(),
            &[url.to_string()],
            "the preloaded context is written to the durable cache under its URL"
        );
    }

    /// An embedded @context (registered rather than preloaded) has no origin URL
    /// the broker resolved, so clause 5.13.1 does not make it "Cached" and the
    /// resolver leaves the durable cache untouched.
    #[tokio::test]
    async fn an_embedded_context_is_not_recorded_in_the_durable_cache() {
        let registry = registry_with(
            "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld",
            &json!({ "@context": { "name": "https://example.com/name" } }),
        );
        let recorder = Arc::new(RecordingL2::default());
        let loader = offline_loader(registry).with_l2_cache(Arc::clone(&recorder) as Arc<dyn ContextCache>);
        let url = IriBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld".to_string()).expect("valid URL");

        loader.load_document(&url).await.expect("an embedded context resolves");

        assert!(
            recorder.puts.lock().expect("mutex not poisoned").is_empty(),
            "an embedded context is not written to the durable cache"
        );
    }

    /// The durable write lands once, behind the in-process cache: a second
    /// resolution is served from moka and never reaches the registry again.
    #[tokio::test]
    async fn a_second_resolution_of_a_preloaded_context_does_not_write_again() {
        let registry = Arc::new(ContextRegistry::new());
        let url = IriBuf::new("https://forge.example.com/suite.jsonld".to_string()).expect("valid URL");
        registry
            .register_preloaded(url.clone(), literal(&json!({ "@context": { "name": "https://example.com/name" } })))
            .expect("registered");
        let recorder = Arc::new(RecordingL2::default());
        let loader = offline_loader(registry).with_l2_cache(Arc::clone(&recorder) as Arc<dyn ContextCache>);

        loader.load_document(&url).await.expect("first resolution");
        loader.load_document(&url).await.expect("second resolution");

        assert_eq!(
            recorder.puts.lock().expect("mutex not poisoned").len(),
            1,
            "the durable write happens once, behind the in-process cache"
        );
    }
}
