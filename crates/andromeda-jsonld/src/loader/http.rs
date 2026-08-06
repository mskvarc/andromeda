//! The four-step context resolution the JSON-LD processor drives.
//!
//! A URL is looked for in the in-process moka cache, then in the registry of
//! embedded and preloaded documents, then in the caller-supplied L2 cache,
//! and only then over HTTP. The L2 write happens once per URL, on the first
//! remote-fetch success.
//!
//! Access accounting is deliberately not part of that path. Clause 5.13.3.5
//! requires a `numberOfHits` counter but does not require it to be durable per
//! access, so a hit bumps an in-process
//! [`crate::hit_counter::HitCounter`] and a background flush folds
//! the deltas into the store — otherwise every expansion would serialise on
//! one `jsonld_contexts` row.

use crate::{
    context_cache::ContextCache,
    context_registry::ContextRegistry,
    core_context::is_ngsi_ld_core_context_url,
    engine::options::RemoteContextFetch,
    error::JsonLdError,
    hit_counter::HitCounter,
    loader::{
        error::{LoaderError, LoaderErrorKind},
        url_normalization::{normalize_loaded_context_document, normalize_local_hosted_context_url},
    },
};
use iri_rs::{Iri, IriBuf};
use jsonld::{LoadError, Loader, RemoteDocument};
use jsonld_syntax::Parse;
use mediatype::MediaTypeBuf;
use moka::future::Cache;
use reqwest::{Client, header::CONTENT_TYPE, redirect::Policy};
use std::{sync::Arc, time::Duration};
use tracing::trace;

/// What a context fetch asks the server for.
const ACCEPT_HEADER: &str = "application/ld+json, application/json";

/// Resolves a context URL to a context document.
///
/// Not `Clone`: wrap in `Arc` to share across tasks.
#[derive(Debug)]
pub struct HttpContextLoader {
    /// The HTTP client remote fetches go through.
    client: Client,
    /// The in-process cache in front of every other step.
    cache: Cache<String, Arc<jsonld_syntax::Value>>,
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
    /// Returns [`JsonLdError::ContextLoad`] when the HTTP client cannot be
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
            .map_err(|error| JsonLdError::ContextLoad {
                url: "client".to_string(),
                message: error.to_string(),
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
    /// loader owns. The URL is normalised the same way the load path keys it, so
    /// a self-referencing hosted URL is matched whichever prefix form it arrives
    /// in.
    pub async fn invalidate(&self, url: &str) {
        let url_string = normalize_local_hosted_context_url(url);
        self.cache.invalidate(&url_string).await;
    }

    /// Resolve a context URL through the full pipeline.
    ///
    /// # Errors
    ///
    /// Returns the [`JsonLdError`] variant describing which step failed and
    /// why.
    pub async fn load_value(&self, url: &str) -> Result<Arc<jsonld_syntax::Value>, JsonLdError> {
        self.load_value_raw(url).await.map_err(|error| JsonLdError::from(error.as_ref()))
    }

    /// [`Self::load_value`] yielding the raw [`LoaderError`].
    ///
    /// `Arc`-wrapped because `moka` requires a cached error to be
    /// `Clone + Send + Sync`. The [`Loader`] implementation needs this form,
    /// since the trait fixes the error type.
    pub(crate) async fn load_value_raw(&self, url: &str) -> Result<Arc<jsonld_syntax::Value>, Arc<LoaderError>> {
        let url_string = normalize_local_hosted_context_url(url);
        if url_string != url {
            trace!("Normalized hosted context URL for lookup: {} -> {}", url, url_string);
        }
        let registry = Arc::clone(&self.registry); // clone: Arc ref-count bump into the cache-fill future
        let client = self.client.clone(); // clone: reqwest::Client is an Arc handle
        let allow_remote = self.allow_remote;
        let max_bytes = self.max_context_bytes;
        let l2_cache = self.l2_cache.clone(); // clone: Arc ref-count bump into the cache-fill future

        let result = self
            .cache
            .try_get_with(url_string.clone(), async move {
                if let Some(value) = registry.get(&url_string) {
                    return Ok(value);
                }

                if let Some(ref cache) = l2_cache
                    && let Some(payload) = cache.get(&url_string).await
                {
                    trace!("L2 cache hit for context: {}", url_string);
                    // Convert the stored payload straight into the syntax value.
                    // The old `to_string` then `parse_str` round trip papered a
                    // serialisation failure over with an empty string, which then
                    // failed to parse with an unrelated "invalid context" error;
                    // `from_serde_json` is the direct, infallible inverse of the
                    // `into_serde_json` the write path used.
                    let parsed = jsonld_syntax::Value::from_serde_json(payload);
                    return Ok(Arc::new(normalize_loaded_context_document(&url_string, parsed)));
                }

                if allow_remote == RemoteContextFetch::Deny {
                    return Err(LoaderError::fetch_disabled(url_string.clone()));
                }

                match fetch_and_parse(&client, &url_string, max_bytes).await {
                    Ok(value) => {
                        let normalized = normalize_loaded_context_document(&url_string, value);
                        // Written to L2 only here, on the first remote-fetch
                        // success: the row carries no counter, because hit
                        // accounting is the HitCounter's job.
                        if let Some(ref cache) = l2_cache {
                            let serde_value = jsonld_syntax::Value::into_serde_json(normalized.clone()); // clone: the normalized document is also returned
                            cache.put(&url_string, serde_value).await;
                            trace!("Persisted context to L2 cache: {}", url_string);
                        }
                        Ok(Arc::new(normalized))
                    }
                    Err(error) => match registry.get(&url_string) {
                        Some(value) => Ok(value),
                        None => Err(error),
                    },
                }
            })
            .await;

        let value = result?;
        // Core contexts live in the registry and never have a stored row, so
        // counting a hit against one would accumulate a delta nothing reads.
        let counted_url = normalize_local_hosted_context_url(url);
        if let Some(ref counter) = self.hit_counter
            && !is_ngsi_ld_core_context_url(&counted_url)
        {
            counter.bump(&counted_url);
        }
        Ok(value)
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
    pub(crate) async fn fetch_remote_value(&self, url: &str) -> Result<Arc<jsonld_syntax::Value>, LoaderError> {
        let url_string = normalize_local_hosted_context_url(url);
        if self.allow_remote == RemoteContextFetch::Deny {
            return Err(LoaderError::fetch_disabled(url_string));
        }
        let value = fetch_and_parse(&self.client, &url_string, self.max_context_bytes).await?;
        Ok(Arc::new(normalize_loaded_context_document(&url_string, value)))
    }
}

impl Loader for HttpContextLoader {
    type Error = LoaderError;

    async fn load(&self, url: Iri<&str>) -> Result<RemoteDocument<IriBuf>, LoadError<Self::Error>> {
        let normalized_url = normalize_local_hosted_context_url(url.as_str());
        let value = self.load_value_raw(&normalized_url).await.map_err(|error| {
            let iri = IriBuf::new(normalized_url.clone()).unwrap_or_else(|_| fallback_iri()); // clone: the IRI is also needed below
            LoadError::new(iri, (*error).clone()) // clone: the Arc is shared with the moka cache
        })?;

        let iri = IriBuf::new(normalized_url.clone()).map_err(|error| {
            // clone: the URL is also reported in the error
            LoadError::new(
                fallback_iri(),
                LoaderError {
                    url: normalized_url,
                    kind: LoaderErrorKind::Invalid(format!("invalid IRI: {error}")),
                },
            )
        })?;

        Ok(RemoteDocument::new(Some(iri), Some(ld_json_media_type()), (*value).clone())) // clone: the document is shared with the moka cache
    }
}

/// The IRI a [`LoadError`] carries when the real one will not parse.
///
/// `urn:error` is a syntactically valid IRI per RFC 8141, so the failing
/// branch is unreachable; aborting rather than panicking keeps the crate free
/// of `unwrap`, `expect` and `panic!`.
fn fallback_iri() -> IriBuf {
    match IriBuf::new("urn:error".to_string()) {
        Ok(iri) => iri,
        Err(_) => std::process::abort(),
    }
}

/// The media type a [`RemoteDocument`] reports for a loaded context.
fn ld_json_media_type() -> MediaTypeBuf {
    jsonld_core::LD_JSON_MEDIA_TYPE.into()
}

/// Fetch a context over HTTP and parse it, refusing anything that is not
/// JSON-shaped or is larger than the configured limit.
async fn fetch_and_parse(client: &Client, url: &str, max_bytes: usize) -> Result<jsonld_syntax::Value, LoaderError> {
    let response = client.get(url).header("Accept", ACCEPT_HEADER).send().await.map_err(|error| LoaderError {
        url: url.to_string(),
        kind: LoaderErrorKind::Request(error.to_string()),
    })?;

    if !response.status().is_success() {
        return Err(LoaderError {
            url: url.to_string(),
            kind: LoaderErrorKind::Http(response.status()),
        });
    }

    if let Some(content_type) = response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok())
        && !is_json_like(content_type)
    {
        return Err(LoaderError {
            url: url.to_string(),
            kind: LoaderErrorKind::Invalid(format!("Unsupported context content type: {content_type}")),
        });
    }

    // Checked twice on purpose: the advertised length rejects an oversized
    // body before it is read, and the measured length catches a server that
    // did not advertise one.
    if let Some(len) = response.content_length()
        && len as usize > max_bytes
    {
        return Err(too_large(url.to_string(), len as usize, max_bytes));
    }

    let bytes = response.bytes().await.map_err(|error| LoaderError {
        url: url.to_string(),
        kind: LoaderErrorKind::Request(error.to_string()),
    })?;

    if bytes.len() > max_bytes {
        return Err(too_large(url.to_string(), bytes.len(), max_bytes));
    }

    let text = std::str::from_utf8(&bytes).map_err(|error| LoaderError {
        url: url.to_string(),
        kind: LoaderErrorKind::Invalid(error.to_string()),
    })?;

    let (value, _) = jsonld_syntax::Value::parse_str(text).map_err(|error| LoaderError {
        url: url.to_string(),
        kind: LoaderErrorKind::Invalid(error.to_string()),
    })?;
    Ok(value)
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
const fn too_large(url: String, size: usize, max: usize) -> LoaderError {
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
        loader::http::{HttpContextLoader, is_json_like},
    };
    use iri_rs::IriBuf;
    use jsonld::Loader;
    use serde_json::json;
    use std::sync::Arc;

    fn registry_with(url: &str, context: serde_json::Value) -> Arc<ContextRegistry> {
        let registry = Arc::new(ContextRegistry::new());
        registry.register(url, jsonld_syntax::Value::from_serde_json(context));
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
        async fn get(&self, _url: &str) -> Option<serde_json::Value> {
            Some(self.0.clone())
        }
        async fn put(&self, _url: &str, _payload: serde_json::Value) {}
    }

    #[tokio::test]
    async fn an_l2_cache_hit_loads_without_a_spurious_invalid_context_error() {
        // A stored payload is converted straight through `from_serde_json`; the
        // old `to_string`/`parse_str` round trip fell back to an empty string on
        // a (serde-impossible) failure and then reported an unrelated "invalid
        // context" error. Even with remote fetching denied, an L2 hit resolves.
        let l2 = Arc::new(FixedL2(json!({ "@context": { "name": "https://example.com/name" } })));
        let loader = offline_loader(Arc::new(ContextRegistry::new())).with_l2_cache(l2);

        let value = loader.load_value("https://example.com/from-l2.jsonld").await.expect("an L2 hit resolves");

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
    async fn a_doubled_api_prefix_still_finds_the_registry_entry_keyed_by_the_canonical_url() {
        let registry = registry_with(
            "http://localhost:1056/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc",
            json!({ "@context": { "A1": "https://example.com/A1" } }),
        );
        let loader = offline_loader(registry);
        let iri = IriBuf::new("http://localhost:1056/ngsi-ld/v1/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:abc".to_string()).expect("valid IRI");

        let document = loader.load(iri.as_ref()).await.expect("registry hit after normalization");

        assert!(document.document.is_object());
    }

    #[tokio::test]
    async fn an_unregistered_context_reports_that_fetching_is_disabled() {
        let loader = offline_loader(Arc::new(ContextRegistry::new()));

        let error = loader.load_value("https://example.com/unknown.jsonld").await.expect_err("no registry entry");

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
}
