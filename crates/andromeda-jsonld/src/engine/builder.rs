//! Configuring an engine and registering what it can resolve offline.
//!
//! Building an engine registers every released NGSI-LD core context, the
//! Andromeda context, the one companion context the v1.9 core artefact still
//! references, and whatever the operator preloaded from disk. After that the
//! engine resolves the entire specification without a network.

use crate::{
    context_cache::ContextCache,
    context_registry::ContextRegistry,
    core_context::{ANDROMEDA_CORE_CONTEXT, ANDROMEDA_CORE_CONTEXT_URL, NGSI_LD_CORE_CONTEXT_MAP, NGSI_LD_CORE_CONTEXT_URL_LIST},
    engine::{options::RemoteContextFetch, processor::JsonLdEngine, term_lookup::TermCache},
    error::JsonLdError,
    hit_counter::HitCounter,
    loader::http::HttpContextLoader,
};
use jsonld_syntax::Parse;
use std::{collections::HashMap, fs, sync::Arc};
use tracing::info;

/// The W3C Data Integrity companion context the ETSI v1.9 core artefact names
/// as the scoped context of its `ngsildproof` term.
///
/// The broker implements no proof verification, but the term is in the
/// verbatim ETSI document and its scoped context must resolve or expansion
/// fails, so an empty stub is registered for it.
const DATA_INTEGRITY_CONTEXT_URL: &str = "https://w3id.org/security/data-integrity/v2";
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
    preload_contexts: Option<HashMap<String, String>>,
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
        }
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
    pub fn preload_contexts(mut self, contexts: HashMap<String, String>) -> JsonLdEngineBuilder {
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
    /// and [`JsonLdError::ContextLoad`] if the HTTP client cannot be built.
    pub fn build(self) -> Result<JsonLdEngine, JsonLdError> {
        for url in NGSI_LD_CORE_CONTEXT_URL_LIST {
            self.registry.register(url.to_string(), embedded_core_context(url)?);
        }
        self.registry.register(
            ANDROMEDA_CORE_CONTEXT_URL.to_string(),
            parse_embedded(ANDROMEDA_CORE_CONTEXT, ANDROMEDA_CORE_CONTEXT_URL)?,
        );
        self.registry.register(
            DATA_INTEGRITY_CONTEXT_URL.to_string(),
            parse_embedded(DATA_INTEGRITY_STUB, DATA_INTEGRITY_CONTEXT_URL)?,
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

        Ok(JsonLdEngine::assembled(
            loader,
            TermCache::new(self.cache_capacity, self.cache_ttl_seconds),
            hit_counter,
        ))
    }
}

impl Default for JsonLdEngineBuilder {
    fn default() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
    }
}

/// Parse the embedded payload of one core context URL.
fn embedded_core_context(url: &str) -> Result<jsonld_syntax::Value, JsonLdError> {
    let payload = NGSI_LD_CORE_CONTEXT_MAP
        .get(url)
        .ok_or_else(|| JsonLdError::EmbeddedContextInvalid(format!("no embedded payload for {url}")))?;
    parse_embedded(payload, url)
}

/// Parse an embedded context payload.
fn parse_embedded(payload: &str, url: &str) -> Result<jsonld_syntax::Value, JsonLdError> {
    let (value, _) = jsonld_syntax::Value::parse_str(payload).map_err(|error| JsonLdError::EmbeddedContextInvalid(format!("{url}: {error}")))?;
    Ok(value)
}

/// Read the preloaded contexts from disk and register them.
fn register_preloaded(entries: Option<&HashMap<String, String>>, registry: &ContextRegistry) -> Result<Vec<(String, String)>, JsonLdError> {
    let Some(entries) = entries else {
        return Ok(Vec::new());
    };

    let mut loaded = Vec::with_capacity(entries.len());
    for (url, file) in entries {
        let raw = fs::read_to_string(file).map_err(|error| JsonLdError::ContextLoad {
            url: url.clone(), // clone: the configuration map is borrowed and stays valid
            message: format!("Failed reading preloaded context file '{file}': {error}"),
        })?;
        let (value, _) = jsonld_syntax::Value::parse_str(&raw).map_err(|error| JsonLdError::ContextInvalid {
            url: url.clone(), // clone: the configuration map is borrowed and stays valid
            message: format!("Invalid preloaded context file '{file}': {error}"),
        })?;
        registry.register(url.clone(), value); // clone: the configuration map is borrowed and stays valid
        loaded.push((url.clone(), file.clone())); // clone: the configuration map is borrowed and stays valid
    }
    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use crate::{context_spec::ContextSpec, engine::processor::JsonLdEngine, error::JsonLdError};
    use serde_json::json;
    use std::{
        collections::HashMap,
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

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
        preloads.insert("https://example.com/context.jsonld".to_string(), path.to_string_lossy().to_string());

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
        let path = preloaded_file(
            "core-importing",
            r#"{"@context":["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
               {"@vocab":"https://example.com/vocab#","Building":"https://example.com/vocab#Building",
                "name":"https://example.com/vocab#name"}]}"#,
        );
        let url = "https://example.com/core-importing-context.jsonld";
        let mut preloads = HashMap::new();
        preloads.insert(url.to_string(), path.to_string_lossy().to_string());

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
    async fn a_preloaded_context_redefining_a_core_term_expands_after_the_core_import_is_stripped() {
        let path = preloaded_file(
            "location-redefining",
            r#"{"@context":["https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
               {"@vocab":"https://example.com/vocab#","Building":"https://example.com/vocab#Building",
                "location":"https://example.com/vocab#location"}]}"#,
        );
        let url = "https://example.com/location-redefining-context.jsonld";
        let mut preloads = HashMap::new();
        preloads.insert(url.to_string(), path.to_string_lossy().to_string());

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
            .await;

        assert!(expanded.is_ok(), "expansion failed: {:?}", expanded.err());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn a_preloaded_context_whose_file_is_missing_fails_the_build() {
        let mut preloads = HashMap::new();
        preloads.insert(
            "https://example.com/context.jsonld".to_string(),
            "/nonexistent/andromeda-jsonld-missing.jsonld".to_string(),
        );

        let built = JsonLdEngine::builder().preload_contexts(preloads).build();

        assert!(matches!(built, Err(JsonLdError::ContextLoad { .. })));
    }

    #[test]
    fn a_preloaded_context_that_is_not_json_fails_the_build() {
        let path = preloaded_file("invalid", "{not-json");
        let mut preloads = HashMap::new();
        preloads.insert("https://example.com/context.jsonld".to_string(), path.to_string_lossy().to_string());

        let built = JsonLdEngine::builder().preload_contexts(preloads).build();

        assert!(matches!(built, Err(JsonLdError::ContextInvalid { .. })));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn an_engine_without_a_durable_cache_keeps_no_hit_counter() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        assert!(engine.hit_counter().is_none(), "there is nothing to flush accumulated hits into");
    }
}
