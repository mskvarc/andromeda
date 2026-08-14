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
    engine::{options::RemoteContextFetch, processor::JsonLdEngine, term_lookup::TermCache, term_map::vocabulary::CoreVocabulary},
    error::JsonLdError,
    hit_counter::HitCounter,
    loader::http::HttpContextLoader,
};
use iri_rs::{Iri, IriBuf, iri};
use jsonld_syntax::Parse;
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
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
/// construction path does not depend on a configuration framework — the
/// [`crate::config::JsonLdConfig`] that reads them from the environment is a
/// separate concern.
#[derive(Debug)]
pub struct JsonLdEngineBuilder {
    /// The registry the built engine resolves from first.
    registry: Arc<ContextRegistry>,
    /// The durable cache, when the caller supplies one.
    l2_cache: Option<Arc<dyn ContextCache>>,
    /// Whether the engine may fetch a context it does not hold.
    allow_remote: RemoteContextFetch,
    /// HTTP request timeout, in seconds.
    timeout_seconds: u64,
    /// How many redirects a context fetch may follow.
    max_redirects: u32,
    /// Entry bound shared by the context cache and the term cache.
    cache_capacity: u64,
    /// Entry lifetime shared by the context cache and the term cache.
    cache_ttl_seconds: u64,
    /// The size beyond which a remote context is refused.
    max_context_bytes: u64,
    /// Contexts to read from disk at build time, by URL.
    preload_contexts: Option<HashMap<Url, PathBuf>>,
    /// The released core context a versionless reference expands against
    /// (clause 4.4), or `None` for the versionless core context.
    default_core_url: Option<IriBuf>,
}

impl JsonLdEngineBuilder {
    /// A builder carrying the defaults.
    pub fn new() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder {
            registry: Arc::new(ContextRegistry::new()),
            l2_cache: None,
            allow_remote: RemoteContextFetch::Allow,
            timeout_seconds: 30,
            max_redirects: 5,
            cache_capacity: 1024,
            cache_ttl_seconds: 3600,
            max_context_bytes: 2_000_000,
            preload_contexts: None,
            default_core_url: None,
        }
    }

    /// Pin which released core context a versionless reference expands against.
    ///
    /// Clause 4.3.6.8 admits any released core context; a deployment picks one,
    /// and the fastpath term table is derived from it. Left unset, a versionless
    /// reference expands against the versionless core context.
    #[must_use]
    pub fn default_core_url(mut self, url: IriBuf) -> JsonLdEngineBuilder {
        self.default_core_url = Some(url);
        self
    }

    /// Whether the engine may fetch contexts over HTTP.
    #[must_use]
    pub fn allow_remote(mut self, allow: impl Into<RemoteContextFetch>) -> JsonLdEngineBuilder {
        self.allow_remote = allow.into();
        self
    }

    /// The HTTP request timeout, in seconds.
    #[must_use]
    pub const fn timeout_seconds(mut self, seconds: u64) -> JsonLdEngineBuilder {
        self.timeout_seconds = seconds;
        self
    }

    /// How many redirects a context fetch may follow.
    #[must_use]
    pub const fn max_redirects(mut self, max: u32) -> JsonLdEngineBuilder {
        self.max_redirects = max;
        self
    }

    /// The entry bound of the in-process caches.
    #[must_use]
    pub const fn cache_capacity(mut self, capacity: u64) -> JsonLdEngineBuilder {
        self.cache_capacity = capacity;
        self
    }

    /// The entry lifetime of the in-process caches, in seconds.
    #[must_use]
    pub const fn cache_ttl_seconds(mut self, ttl: u64) -> JsonLdEngineBuilder {
        self.cache_ttl_seconds = ttl;
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

    /// Build the engine.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::EmbeddedContextInvalid`] if an embedded context
    /// will not parse, [`JsonLdError::ContextLoad`] if a preloaded file
    /// cannot be read, [`JsonLdError::ContextInvalid`] if one will not parse,
    /// and [`JsonLdError::ClientBuild`] if the HTTP client cannot be built.
    pub fn build(self) -> Result<JsonLdEngine, JsonLdError> {
        for url in NGSI_LD_CORE_CONTEXT_URL_LIST {
            self.registry.register(IriBuf::from(url), parse_core_context(url)?);
        }
        self.registry.register(
            IriBuf::from(DATA_INTEGRITY_CONTEXT_URL),
            parse_embedded(DATA_INTEGRITY_STUB, DATA_INTEGRITY_CONTEXT_URL.as_ref())?,
        );

        let preloaded = register_preloaded(self.preload_contexts.as_ref(), &self.registry)?;
        if preloaded.is_empty() {
            info!("JSON-LD preloaded contexts: none configured");
        } else {
            info!("JSON-LD preloaded contexts registered ({}): {:?}", preloaded.len(), preloaded);
        }

        let mut loader = HttpContextLoader::new(
            Arc::clone(&self.registry), // clone: Arc ref-count bump
            self.allow_remote,
            self.timeout_seconds,
            self.max_redirects,
            self.cache_capacity,
            self.cache_ttl_seconds,
            self.max_context_bytes as usize,
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

        // The pin defaults to the versionless core context, which the registry
        // resolves to whatever version this deployment embedded.
        let default_core_url = self
            .default_core_url
            .unwrap_or_else(|| IriBuf::from(iri!("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context.jsonld")));
        let core_vocabulary = CoreVocabulary::try_from_generated()?;
        Ok(JsonLdEngine::assembled(
            loader,
            TermCache::new(self.cache_capacity, self.cache_ttl_seconds),
            hit_counter,
            default_core_url,
            core_vocabulary,
        ))
    }
}

impl Default for JsonLdEngineBuilder {
    fn default() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
    }
}

/// Parse the embedded payload of one core context URL.
fn parse_core_context(url: Iri<&str>) -> Result<jsonld_syntax::Value, JsonLdError> {
    let payload = embedded_core_context_payload(url).ok_or_else(|| JsonLdError::EmbeddedContextInvalid(format!("no embedded payload for {url}")))?;
    parse_embedded(payload, url)
}

/// Parse an embedded context payload.
fn parse_embedded(payload: &str, url: Iri<&str>) -> Result<jsonld_syntax::Value, JsonLdError> {
    let (value, _) = jsonld_syntax::Value::parse_str(payload).map_err(|error| JsonLdError::EmbeddedContextInvalid(format!("{url}: {error}")))?;
    Ok(value)
}

/// Parse a compile-time or configuration URL before it enters an internal registry.
fn iri_buf(url: &Url) -> Result<IriBuf, JsonLdError> {
    IriBuf::new(url.as_str().to_owned()).map_err(|error| JsonLdError::EmbeddedContextInvalid(format!("{url}: invalid IRI: {}", error.0)))
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
        let raw = fs::read_to_string(file).map_err(|error| JsonLdError::ContextLoad {
            url: typed_url.clone(),
            message: format!("Failed reading preloaded context file '{}': {error}", file.display()),
        })?;
        let (value, _) = jsonld_syntax::Value::parse_str(&raw).map_err(|error| JsonLdError::ContextInvalid {
            url: typed_url.clone(),
            message: format!("Invalid preloaded context file '{}': {error}", file.display()),
        })?;
        let iri = iri_buf(url)?;
        registry.register(iri, value);
        loaded.push((url.clone(), file.clone())); // clone: the configuration map is borrowed and stays valid
    }
    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        core_context::{NGSI_LD_CORE_CONTEXT_V16_URL, NGSI_LD_CORE_CONTEXT_V19_URL},
        engine::processor::JsonLdEngine,
        error::JsonLdError,
    };
    use andromeda_ngsi_ld_vocabulary::NGSI_LD_CORE_CONTEXT_URL;
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::{
        collections::HashMap,
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
        let path = std::env::temp_dir().join(format!("andromeda-jsonld-{label}-{unique}.jsonld"));
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

        assert!(engine.expand(document, &ContextSpec::none()).await.is_err());
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

        let expanded = engine
            .expand(
                json!({
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

        let expanded = engine
            .expand(
                json!({
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
        // `location` therefore expands to the Core's IRI, not the user's — the
        // Core wins — and the request is not rejected.
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

        let expanded = engine
            .expand(
                json!({
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

        assert!(matches!(built, Err(JsonLdError::ContextLoad { .. })));
    }

    #[test]
    fn a_preloaded_context_that_is_not_json_fails_the_build() {
        let path = preloaded_file("invalid", "{not-json");
        let mut preloads = HashMap::new();
        preloads.insert(context_url("https://example.com/context.jsonld"), path.clone());

        let built = JsonLdEngine::builder().preload_contexts(preloads).build();

        assert!(matches!(built, Err(JsonLdError::ContextInvalid { .. })));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_preload_cannot_replace_an_embedded_core_context() {
        let path = preloaded_file("protected-core", r#"{"@context":{"overridden":"https://example.com/override"}}"#);
        let mut preloads = HashMap::new();
        preloads.insert(context_url(NGSI_LD_CORE_CONTEXT_V19_URL.as_str()), path.clone());

        let built = JsonLdEngine::builder().preload_contexts(preloads).build();

        assert!(matches!(built, Err(JsonLdError::ProtectedContextPreload { .. })));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn an_engine_without_a_durable_cache_keeps_no_hit_counter() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        assert!(engine.hit_counter().is_none(), "there is nothing to flush accumulated hits into");
    }

    #[test]
    fn two_engines_carry_their_own_core_context_pin_independently() {
        // The pin lives on the engine, not a process global: two engines built
        // with different pins keep them both, where the removed `OnceLock` let
        // the first setter win for the whole process (clause 4.4).
        let older = JsonLdEngine::builder()
            .allow_remote(false)
            .default_core_url(IriBuf::new(NGSI_LD_CORE_CONTEXT_V16_URL.to_string()).expect("valid core URL"))
            .build()
            .expect("engine builds");
        let newer = JsonLdEngine::builder()
            .allow_remote(false)
            .default_core_url(IriBuf::new(NGSI_LD_CORE_CONTEXT_V19_URL.to_string()).expect("valid core URL"))
            .build()
            .expect("engine builds");

        pretty_assertions::assert_eq!(older.default_core_url().as_str(), NGSI_LD_CORE_CONTEXT_V16_URL.as_str());
        pretty_assertions::assert_eq!(newer.default_core_url().as_str(), NGSI_LD_CORE_CONTEXT_V19_URL.as_str());
    }

    #[test]
    fn an_engine_without_a_pin_defaults_to_the_versionless_core_context() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        pretty_assertions::assert_eq!(engine.default_core_url().as_str(), NGSI_LD_CORE_CONTEXT_URL.as_str());
    }
}
