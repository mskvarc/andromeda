//! Configuring an engine and registering what it can resolve offline.
//!
//! Building an engine registers every released NGSI-LD core context, the one
//! companion context the v1.9 core artefact still references, and whatever the
//! operator preloaded from disk. After that the engine resolves the entire
//! specification without a network.

use crate::{
    context_cache::ContextCache,
    context_registry::ContextRegistry,
    core_context::{NGSI_LD_CORE_CONTEXT_URL_LIST, embedded_core_context as embedded_core_context_payload, is_ngsi_ld_core_context_url},
    engine::{options::RemoteContextFetch, processor::JsonLdEngine, term_lookup::TermCache},
    error::JsonLdError,
    hit_counter::HitCounter,
    hosted_context_recognizer::{HostedContextRecognizer, NoHostedContexts},
    loader::{cache_lifetime::CacheLifetime, context_lifetimes::ContextLifetimes, http::HttpContextLoader, loader_settings::LoaderSettings},
};
use andromeda_json_literal::JsonLiteral;
use andromeda_outbound_http::address_policy::AddressPolicy;
use iri_rs::{Iri, IriBuf, iri};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc, time::Duration};
use tracing::info;
use url::Url;

/// The W3C Data Integrity companion context the ETSI v1.9 core artefact names
/// as the scoped context of its `ngsildproof` term.
///
/// The broker implements no proof verification, but the term is in the
/// verbatim ETSI document and its scoped context must resolve or expansion
/// fails, so an empty stub is registered for it.
const DATA_INTEGRITY_CONTEXT_URL: Iri<&'static str> = iri!("https://w3id.org/security/data-integrity/v2");
/// The stub registered for [`DATA_INTEGRITY_CONTEXT_URL`].
const DATA_INTEGRITY_STUB: &str = r#"{"@context":{}}"#;

/// Configuration for one [`JsonLdEngine`].
///
/// Values arrive one method at a time rather than as a struct so the crate's
/// construction path does not depend on a configuration framework. The
/// [`andromeda_configuration::schema::jsonld::JsonLdConfig`] that reads them from the environment is a
/// separate concern.
#[derive(Debug)]
pub struct JsonLdEngineBuilder {
    /// The registry the built engine resolves from first.
    registry: Arc<ContextRegistry>,
    /// The durable cache, when the caller supplies one.
    l2_cache: Option<Arc<dyn ContextCache>>,
    /// Whether the engine may fetch a context it does not hold.
    allow_remote: RemoteContextFetch,
    /// How long one context fetch may take.
    timeout: Duration,
    /// How many redirects a context fetch may follow.
    max_redirects: u32,
    /// Which resolved addresses a context fetch may reach.
    addresses: AddressPolicy,
    /// Entry bound shared by the context cache and the term cache.
    cache_capacity: u64,
    /// The term cache's entry lifetime, and the heuristic lifetime a context
    /// falls back to when its response states no expiration time.
    cache_ttl: CacheLifetime,
    /// How long an `@context` the broker stores itself is used from memory
    /// before its row is read again.
    hosted_context_revalidation: CacheLifetime,
    /// The size beyond which a remote context is refused.
    max_context_bytes: u64,
    /// Contexts to read from disk at build time, by URL.
    preload_contexts: Option<HashMap<Url, PathBuf>>,
    /// Recognises references to the tenant's own stored `@context`s.
    hosted_contexts: Arc<dyn HostedContextRecognizer>,
}

impl JsonLdEngineBuilder {
    /// A builder carrying the defaults.
    #[must_use]
    pub fn new() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder {
            registry: Arc::new(ContextRegistry::new()),
            l2_cache: None,
            allow_remote: RemoteContextFetch::Allow,
            timeout: Duration::from_secs(10),
            max_redirects: 5,
            addresses: AddressPolicy::default(),
            cache_capacity: 1024,
            cache_ttl: CacheLifetime::DEFAULT,
            hosted_context_revalidation: CacheLifetime::HOSTED_REVALIDATION_DEFAULT,
            max_context_bytes: 2_000_000,
            preload_contexts: None,
            hosted_contexts: Arc::new(NoHostedContexts),
        }
    }

    /// Whether the engine may fetch contexts over HTTP.
    #[must_use]
    pub fn allow_remote(mut self, allow: impl Into<RemoteContextFetch>) -> JsonLdEngineBuilder {
        self.allow_remote = allow.into();
        self
    }

    /// How long one context fetch may take.
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> JsonLdEngineBuilder {
        self.timeout = timeout;
        self
    }

    /// How many redirects a context fetch may follow.
    #[must_use]
    pub const fn max_redirects(mut self, max: u32) -> JsonLdEngineBuilder {
        self.max_redirects = max;
        self
    }

    /// Which resolved addresses a context fetch, and every redirect and JSON-LD
    /// alternate it follows, may reach. The default admits every address.
    #[must_use]
    pub fn addresses(mut self, addresses: AddressPolicy) -> JsonLdEngineBuilder {
        self.addresses = addresses;
        self
    }

    /// The entry bound of the in-process caches.
    #[must_use]
    pub const fn cache_capacity(mut self, capacity: u64) -> JsonLdEngineBuilder {
        self.cache_capacity = capacity;
        self
    }

    /// The term cache's entry lifetime, and the heuristic lifetime assigned to
    /// a downloaded `@context` whose response states none.
    ///
    /// Clause 6.3.16 makes a response's own `Expires` field and `max-age` or
    /// `s-maxage` directives authoritative over how long the `@context` it
    /// carries may be reused, so this is a fallback rather than a cap.
    #[must_use]
    pub const fn cache_ttl(mut self, ttl: CacheLifetime) -> JsonLdEngineBuilder {
        self.cache_ttl = ttl;
        self
    }

    /// How long an `@context` the broker stores itself (clause 5.13.1
    /// `"Hosted"` and `"ImplicitlyCreated"`) is used from memory before its row
    /// is read again.
    ///
    /// No origin server can make such a document stale, but another broker
    /// process sharing the database can delete it, so the in-process copy is
    /// revalidated on this span rather than kept for the life of the process.
    #[must_use]
    pub const fn hosted_context_revalidation(mut self, revalidation: CacheLifetime) -> JsonLdEngineBuilder {
        self.hosted_context_revalidation = revalidation;
        self
    }

    /// The size beyond which a remote context is refused.
    #[must_use]
    pub const fn max_context_bytes(mut self, max: u64) -> JsonLdEngineBuilder {
        self.max_context_bytes = max;
        self
    }

    /// Attach a durable context cache.
    #[must_use]
    pub fn with_l2_cache(mut self, cache: Arc<dyn ContextCache>) -> JsonLdEngineBuilder {
        self.l2_cache = Some(cache);
        self
    }

    /// Contexts to read from disk at build time, as URL to file path.
    #[must_use]
    pub fn preload_contexts(mut self, contexts: HashMap<Url, PathBuf>) -> JsonLdEngineBuilder {
        self.preload_contexts = Some(contexts);
        self
    }

    /// The recogniser of the tenant's own stored `@context`s (clause 5.13.1),
    /// which the loader resolves from storage rather than over HTTP.
    ///
    /// An engine no tenant stands behind keeps the default, which recognises
    /// nothing.
    #[must_use]
    pub fn hosted_contexts(mut self, recognizer: Arc<dyn HostedContextRecognizer>) -> JsonLdEngineBuilder {
        self.hosted_contexts = recognizer;
        self
    }

    /// Build the engine.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::EmbeddedContextMissing`] or
    /// [`JsonLdError::EmbeddedContextInvalid`] if an embedded context is absent
    /// or will not parse, [`JsonLdError::PreloadedContextUrl`] if a preloaded
    /// URL is not an IRI, [`JsonLdError::PreloadedContextUnreadable`] if a
    /// preloaded file cannot be read, [`JsonLdError::PreloadedContextInvalid`]
    /// if one will not parse, and [`JsonLdError::ClientBuild`] if the HTTP
    /// client cannot be built.
    pub fn build(self) -> Result<JsonLdEngine, JsonLdError> {
        for url in NGSI_LD_CORE_CONTEXT_URL_LIST {
            register_embedded(&self.registry, url, &parse_core_context(url)?)?;
        }
        register_embedded(
            &self.registry,
            DATA_INTEGRITY_CONTEXT_URL.as_ref(),
            &parse_embedded(DATA_INTEGRITY_STUB, DATA_INTEGRITY_CONTEXT_URL.as_ref())?,
        )?;

        let preloaded = register_preloaded(self.preload_contexts.as_ref(), &self.registry)?;
        if preloaded.is_empty() {
            info!("JSON-LD preloaded contexts: none configured");
        } else {
            info!("JSON-LD preloaded contexts registered ({}): {:?}", preloaded.len(), preloaded);
        }

        // A configured limit wider than the address space saturates rather
        // than failing the build: no response that large could be held in
        // memory, so `usize::MAX` refuses exactly the same set of documents.
        let max_context_bytes = usize::try_from(self.max_context_bytes).unwrap_or(usize::MAX);
        // Clause 6.3.16: the configured TTL is the heuristic expiration time
        // assigned when a response states none, not the lifetime of every entry.
        let settings = LoaderSettings::builder()
            .allow_remote(self.allow_remote)
            .timeout(self.timeout)
            .max_redirects(self.max_redirects)
            .addresses(self.addresses)
            .cache_capacity(self.cache_capacity)
            .lifetimes(ContextLifetimes {
                heuristic: self.cache_ttl,
                hosted_revalidation: self.hosted_context_revalidation,
            })
            .max_context_bytes(max_context_bytes)
            .build();
        let mut loader = HttpContextLoader::new(
            Arc::clone(&self.registry), // clone: Arc ref-count bump
            settings,
            self.hosted_contexts,
        )?;

        // Counting accesses is only worth doing when there is a durable cache
        // whose rows the flush can update.
        let hit_counter = self.l2_cache.as_ref().map(|_durable| Arc::new(HitCounter::new()));
        if let Some(ref counter) = hit_counter {
            loader = loader.with_hit_counter(Arc::clone(counter)); // clone: Arc ref-count bump
        }
        if let Some(durable) = self.l2_cache {
            loader = loader.with_l2_cache(durable);
        }

        Ok(JsonLdEngine::assembled(
            loader,
            TermCache::new(self.cache_capacity, self.cache_ttl),
            hit_counter,
        ))
    }
}

impl Default for JsonLdEngineBuilder {
    fn default() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
    }
}

/// Register one embedded document under `url`.
fn register_embedded(registry: &ContextRegistry, url: Iri<&str>, payload: &JsonLiteral) -> Result<(), JsonLdError> {
    registry
        .register(IriBuf::from(url), payload)
        .map_err(|source| JsonLdError::EmbeddedContextInvalid {
            url: IriBuf::from(url),
            source,
        })
}

/// Parse the embedded payload of one core context URL.
fn parse_core_context(url: Iri<&str>) -> Result<JsonLiteral, JsonLdError> {
    let payload = embedded_core_context_payload(url).ok_or_else(|| JsonLdError::EmbeddedContextMissing { url: IriBuf::from(url) })?;
    parse_embedded(payload, url)
}

/// Parse an embedded context payload.
fn parse_embedded(payload: &str, url: Iri<&str>) -> Result<JsonLiteral, JsonLdError> {
    JsonLiteral::admit(payload).map_err(|source| JsonLdError::EmbeddedContextInvalid {
        url: IriBuf::from(url),
        source,
    })
}

/// Parse a configuration URL before it enters an internal registry.
fn iri_buf(url: &Url) -> Result<IriBuf, JsonLdError> {
    IriBuf::new(url.as_str().to_owned()).map_err(JsonLdError::PreloadedContextUrl)
}

/// Read the preloaded contexts from disk and register them.
fn register_preloaded(entries: Option<&HashMap<Url, PathBuf>>, registry: &ContextRegistry) -> Result<Vec<(Url, PathBuf)>, JsonLdError> {
    let Some(entries) = entries else {
        return Ok(Vec::new());
    };

    let mut loaded = Vec::with_capacity(entries.len());
    for (url, file) in entries {
        let typed_url = iri_buf(url)?;
        if is_ngsi_ld_core_context_url(typed_url.as_ref()) {
            return Err(JsonLdError::ProtectedContextPreload { url: typed_url });
        }
        let raw = fs::read_to_string(file).map_err(|source| JsonLdError::PreloadedContextUnreadable {
            path: file.clone(), // clone: the configuration map is borrowed and the failure outlives it
            source,
        })?;
        let preloaded = |source| JsonLdError::PreloadedContextInvalid {
            path: file.clone(), // clone: the configuration map is borrowed and the failure outlives it
            source,
        };
        let written = JsonLiteral::admit(&raw).map_err(preloaded)?;
        registry.register_preloaded(typed_url, written).map_err(preloaded)?;
        loaded.push((url.clone(), file.clone())); // clone: the configuration map is borrowed and stays valid
    }
    Ok(loaded)
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        core_context::{NGSI_LD_CORE_CONTEXT_UNVERSIONED_URL, NGSI_LD_CORE_CONTEXT_V16_URL},
        engine::processor::JsonLdEngine,
        error::JsonLdError,
        test_documents::expanded_document,
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use serde_json::json;
    use std::{
        collections::HashMap,
        env,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };
    use url::Url;

    fn context_url(raw: &str) -> Url {
        Url::parse(raw).expect("valid context URL")
    }

    /// A temporary file holding a context, named so parallel tests cannot
    /// collide.
    fn preloaded_file(label: &str, payload: &str) -> PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("a clock after the epoch").as_nanos();
        let path = env::temp_dir().join(format!("andromeda-jsonld-{label}-{unique}.jsonld"));
        fs::write(&path, payload).expect("the temporary directory is writable");
        path
    }

    #[tokio::test]
    async fn a_context_that_is_neither_embedded_nor_preloaded_does_not_resolve_offline() {
        let document = json!({
            "@context": "https://example.com/context.jsonld",
            "id": "urn:ngsi-ld:Device:001",
            "type": "Device",
            "temperature": 21
        });

        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");

        assert!(expanded_document(&engine, &document, &ContextSpec::none()).await.is_err());
    }

    #[tokio::test]
    async fn a_preloaded_context_resolves_offline() {
        let path = preloaded_file("vocab-only", r#"{"@context":{"@vocab":"https://example.com/vocab#"}}"#);
        let mut preloads = HashMap::new();
        preloads.insert(context_url("https://example.com/context.jsonld"), path.clone());

        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");

        let expanded = expanded_document(
            &engine,
            &json!({
                "@context": "https://example.com/context.jsonld",
                "id": "urn:ngsi-ld:Device:001",
                "type": "Device",
                "temperature": 21
            }),
            &ContextSpec::none(),
        )
        .await;

        assert!(expanded.is_ok(), "preloaded context must resolve: {:?}", expanded.err());
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_preloaded_context_that_imports_the_core_context_still_expands() {
        // Clause 5.5.7: the broker gives the Core @context absolute precedence
        // by appending it last, so a user context that redundantly imports the
        // Core still expands rather than failing on a protected-term clash with
        // the Core the broker adds.
        let path = preloaded_file(
            "core-importing",
            r#"{"@context":["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
               {"@vocab":"https://example.com/vocab#","Building":"https://example.com/vocab#Building",
                "name":"https://example.com/vocab#name"}]}"#,
        );
        let url = "https://example.com/core-importing-context.jsonld";
        let mut preloads = HashMap::new();
        preloads.insert(context_url(url), path.clone());

        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");

        let expanded = expanded_document(
            &engine,
            &json!({
                "@context": url,
                "id": "urn:ngsi-ld:Building:core-import",
                "type": "Building",
                "name": { "type": "Property", "value": "x" }
            }),
            &ContextSpec::none(),
        )
        .await;

        assert!(expanded.is_ok(), "expansion failed: {:?}", expanded.err());
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn a_user_context_redefining_a_core_term_resolves_to_the_core_definition() {
        // Clause 5.5.7: the Core @context "shall always [have] absolute
        // precedence, regardless of the position of the Core @context in the
        // @context array". A user context that redefines the core term
        // `location` therefore expands to the Core's IRI, not the user's:
        // the Core wins, and the request is not rejected.
        let path = preloaded_file(
            "location-redefining",
            r#"{"@context":["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
               {"@vocab":"https://example.com/vocab#","Building":"https://example.com/vocab#Building",
                "location":"https://example.com/vocab#location"}]}"#,
        );
        let url = "https://example.com/location-redefining-context.jsonld";
        let mut preloads = HashMap::new();
        preloads.insert(context_url(url), path.clone());

        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine builds");

        let expanded = expanded_document(
            &engine,
            &json!({
                "@context": url,
                "id": "urn:ngsi-ld:Building:location-redef",
                "type": "Building",
                "location": { "type": "GeoProperty", "value": { "type": "Point", "coordinates": [13.3986, 52.5547] } }
            }),
            &ContextSpec::none(),
        )
        .await
        .expect("a redefinition of a core term expands, with the core winning");

        let rendered = expanded.to_string();
        assert!(
            rendered.contains("https://uri.etsi.org/ngsi-ld/location"),
            "the core definition of `location` must win: {rendered}"
        );
        assert!(
            !rendered.contains("https://example.com/vocab#location"),
            "the user redefinition of `location` must not survive: {rendered}"
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_preloaded_context_whose_file_is_missing_fails_the_build() {
        let mut preloads = HashMap::new();
        preloads.insert(
            context_url("https://example.com/context.jsonld"),
            PathBuf::from("/nonexistent/andromeda-jsonld-missing.jsonld"),
        );

        let built = JsonLdEngine::builder().preload_contexts(preloads).build();

        assert!(matches!(built, Err(JsonLdError::PreloadedContextUnreadable { .. })));
    }

    #[test]
    fn a_preloaded_context_that_is_not_json_fails_the_build() {
        let path = preloaded_file("invalid", "{not-json");
        let mut preloads = HashMap::new();
        preloads.insert(context_url("https://example.com/context.jsonld"), path.clone());

        let built = JsonLdEngine::builder().preload_contexts(preloads).build();

        assert!(matches!(built, Err(JsonLdError::PreloadedContextInvalid { .. })));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_preload_cannot_replace_an_embedded_core_context() {
        let path = preloaded_file("protected-core", r#"{"@context":{"overridden":"https://example.com/override"}}"#);
        let mut preloads = HashMap::new();
        preloads.insert(context_url(NGSI_LD_CORE_CONTEXT_URL.as_str()), path.clone());

        let built = JsonLdEngine::builder().preload_contexts(preloads).build();

        assert!(matches!(built, Err(JsonLdError::ProtectedContextPreload { .. })));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn an_engine_without_a_durable_cache_keeps_no_hit_counter() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        assert!(engine.hit_counter().is_none(), "there is nothing to flush accumulated hits into");
    }

    /// Clause 4.4: every core reference, whatever release it names, is
    /// processed as the v1.9 core context, so a term only an older release
    /// defined falls to the core `@vocab` rather than to its older IRI.
    #[tokio::test]
    async fn every_release_of_the_core_context_expands_as_the_v1_9_core_context() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        for core in [NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_UNVERSIONED_URL, NGSI_LD_CORE_CONTEXT_URL] {
            let expanded = expanded_document(
                &engine,
                &json!({
                    "@context": core.as_str(),
                    "id": "urn:ngsi-ld:Thing:1",
                    "type": "Thing",
                    "attributeCount": { "type": "Property", "value": 1 }
                }),
                &ContextSpec::none(),
            )
            .await
            .expect("a core context expands")
            .to_string();

            assert!(expanded.contains("https://uri.etsi.org/ngsi-ld/attributeCount"), "{core}: {expanded}");
        }
    }
}
