//! JSON-LD processing engine for NGSI-LD.
//!
//! [`JsonLdEngine`] provides expand and compact operations.  Use
//! [`JsonLdEngineBuilder`] to configure and construct an engine instance.

use crate::{
    Error,
    context::{self, ContextSpec, CoreContextPolicy},
    core_context::{
        DATA_INTEGRITY_CONTEXT,
        DATA_INTEGRITY_CONTEXT_URL,
        NGSI_LD_CORE_CONTEXT_MAP,
        NGSI_LD_CORE_CONTEXT_URL,
        NGSI_LD_CORE_CONTEXT_URL_LIST,
        is_ngsi_ld_core_context_url,
    },
    loader::{ContextCache, ContextRegistry, HttpContextLoader},
};
use contextual::WithContext;
use json_ld::{
    JsonLdProcessor,
    RemoteContextReference,
    RemoteDocument,
    syntax::{Context as JsonLdContext, TryFromJson},
};
use json_syntax::Parse;
use moka::future::Cache;
use rdf_types::vocabulary::{BlankIdIndex, IndexVocabulary, IriIndex};
use std::{collections::HashMap, fs, sync::Arc, time::Duration};
use tracing::info;

// ---------------------------------------------------------------------------
// CompactOptions
// ---------------------------------------------------------------------------

/// Options for JSON-LD compaction.
#[derive(Debug, Clone, Copy)]
pub struct CompactOptions {
    /// When `true`, strip `@context` from the compacted output.
    pub strip_context: bool,
}

impl Default for CompactOptions {
    fn default() -> Self {
        Self { strip_context: true }
    }
}

// ---------------------------------------------------------------------------
// JsonLdEngine
// ---------------------------------------------------------------------------

/// JSON-LD processing engine.
///
/// Provides [`expand`](Self::expand) and [`compact`](Self::compact) operations
/// backed by a configurable context loader.
///
/// Cloning is cheap — the loader and caches are shared via `Arc`.
#[derive(Clone)]
pub struct JsonLdEngine {
    loader: Arc<HttpContextLoader>,
    /// Cache for expanded type/term IRIs to avoid creating synthetic documents
    /// on every call.  Keyed by `"{type|term}:{short_name}:{context_key}"`.
    term_cache: Cache<String, String>,
}

impl JsonLdEngine {
    /// Create an engine with default settings (remote contexts enabled, no L2
    /// cache, no preloaded contexts).
    pub fn new() -> Result<Self, Error> {
        Self::builder().build()
    }

    /// Start building an engine with custom configuration.
    pub fn builder() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
    }

    /// Expand a JSON-LD document to its canonical (expanded) form.
    ///
    /// Short property names are converted to full IRIs.  The result is always
    /// a JSON array of node objects.
    pub async fn expand(&self, document: serde_json::Value, context: &ContextSpec) -> Result<serde_json::Value, Error> {
        let had_explicit_core_context = has_explicit_core_context(&document, context);
        let mut document = context::apply_context(document, context)?;
        resolve_relative_context_urls(&mut document);
        let applied_context = extract_applied_context(&document);
        let context_urls = extract_context_urls(&document);
        let normalized_context_urls = normalize_context_urls_for_logging(&context_urls);
        let core_appended_by_resolver = context.core_policy() == CoreContextPolicy::Append
            && !had_explicit_core_context
            && normalized_context_urls.iter().any(|url| url == NGSI_LD_CORE_CONTEXT_URL);
        let syntax_value = json_syntax::Value::from_serde_json(document);
        let remote_doc: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, syntax_value);

        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();

        let expanded = match remote_doc.expand_with(&mut vocabulary, &*self.loader).await {
            Ok(expanded) => expanded,
            Err(err) => {
                tracing::warn!(
                    "JSON-LD expansion failed. core_appended_by_resolver={}; applied_context={:?}; context_urls={:?}; normalized_context_urls={:?}; error={:?}",
                    core_appended_by_resolver,
                    applied_context,
                    context_urls,
                    normalized_context_urls,
                    err
                );
                return Err(err.into());
            }
        };

        use json_ld::Print;
        let expanded_json_str = expanded.with(&vocabulary).compact_print().to_string();
        let expanded_json: serde_json::Value = serde_json::from_str(&expanded_json_str)?;
        Ok(expanded_json)
    }

    /// Compact a JSON-LD document using a context.
    ///
    /// Full IRIs are converted back to short property names.  If compaction
    /// fails due to a protected-term redefinition the engine falls back to
    /// compacting with the core context only.
    pub async fn compact(&self, document: serde_json::Value, context: &ContextSpec, options: CompactOptions) -> Result<serde_json::Value, Error> {
        let syntax_value = json_syntax::Value::from_serde_json(document.clone());
        let remote_doc: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, syntax_value);

        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();
        let context_ref = build_context_reference(context)?;

        let compacted = match remote_doc.compact_with(&mut vocabulary, context_ref.clone(), &*self.loader).await {
            Ok(result) => result,
            Err(e) => {
                let error_msg = e.to_string();
                if error_msg.contains("Protected term redefinition") || error_msg.contains("protected") {
                    tracing::warn!(
                        "Compaction failed with protected term error: {}. \
                         Falling back to core-only context (user-provided context will be dropped). \
                         This may produce unexpected short names in the output. \
                         Consider reviewing the context for protected term conflicts.",
                        error_msg
                    );
                    let core_context = ContextSpec::core_only();
                    let core_context_ref = build_context_reference(&core_context)?;
                    let mut vocabulary2: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();

                    let syntax_value2 = json_syntax::Value::from_serde_json(document.clone());
                    let remote_doc2: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, syntax_value2);

                    remote_doc2.compact_with(&mut vocabulary2, core_context_ref, &*self.loader).await.map_err(|e| {
                        tracing::error!("Compaction failed even with core context: {}", e);
                        e
                    })?
                } else {
                    tracing::error!("Compaction failed: {}. Document: {}", e, document);
                    return Err(e.into());
                }
            }
        };

        let mut compacted_json = json_syntax::Value::into_serde_json(compacted);

        if options.strip_context {
            strip_context(&mut compacted_json);
        }

        Ok(compacted_json)
    }

    /// Expand a single type term to its full IRI.
    ///
    /// Results are cached per `(term, context)` pair to avoid constructing
    /// and expanding a synthetic JSON-LD document on every call.
    pub async fn expand_type(&self, type_term: &str, context: &ContextSpec) -> Result<String, Error> {
        if is_full_iri(type_term) {
            return Ok(type_term.to_string());
        }

        let cache_key = term_cache_key("type", type_term, context);
        if let Some(cached) = self.term_cache.get(&cache_key).await {
            return Ok(cached);
        }

        let mut doc = serde_json::json!({
            "id": "urn:ngsi-ld:_:temp",
            "type": type_term
        });

        if let Some(ctx) = context::resolve_context_value(None, context) {
            if let Some(obj) = doc.as_object_mut() {
                obj.insert("@context".to_string(), ctx);
            }
        }

        let expanded = self.expand(doc, context).await?;
        let result = expanded
            .get(0)
            .and_then(|n| n.get("@type"))
            .and_then(|t| t.get(0))
            .and_then(|t| t.as_str())
            .map(String::from);

        tracing::trace!("Expanded type '{}' using context to '{:?}'", type_term, result);

        let iri = result.ok_or_else(|| Error::invalid_iri(format!("Failed to expand type: {type_term}")))?;
        self.term_cache.insert(cache_key, iri.clone()).await;
        Ok(iri)
    }

    /// Expand a comma-separated list of type terms to their full IRIs.
    pub async fn expand_types(&self, types: &str, context: &ContextSpec) -> Result<Vec<String>, Error> {
        let mut result = Vec::new();
        for type_term in types.split(',').map(str::trim) {
            if !type_term.is_empty() {
                result.push(self.expand_type(type_term, context).await?);
            }
        }
        Ok(result)
    }

    /// Expand a single attribute / property term to its full IRI.
    ///
    /// Results are cached per `(term, context)` pair to avoid constructing
    /// and expanding a synthetic JSON-LD document on every call.
    pub async fn expand_term(&self, term: &str, context: &ContextSpec) -> Result<String, Error> {
        if is_full_iri(term) {
            return Ok(term.to_string());
        }

        let cache_key = term_cache_key("term", term, context);
        if let Some(cached) = self.term_cache.get(&cache_key).await {
            return Ok(cached);
        }

        let mut doc = serde_json::json!({
            term: {"@id": "urn:dummy"}
        });

        if let Some(ctx) = context::resolve_context_value(None, context) {
            if let Some(obj) = doc.as_object_mut() {
                obj.insert("@context".to_string(), ctx);
            }
        }

        let expanded = self.expand(doc, context).await?;

        let iri = if let Some(obj) = expanded.get(0).and_then(|v| v.as_object()) {
            obj.keys()
                .find(|key| *key != "@id" && *key != "@type")
                .cloned()
                .unwrap_or_else(|| format!("https://uri.etsi.org/ngsi-ld/default-context/{}", term))
        } else {
            format!("https://uri.etsi.org/ngsi-ld/default-context/{}", term)
        };

        self.term_cache.insert(cache_key, iri.clone()).await;
        Ok(iri)
    }

    /// Reference to the underlying loader (for advanced use).
    pub fn loader(&self) -> &Arc<HttpContextLoader> {
        &self.loader
    }

    /// Resolve a context URL and return its payload as a serde_json::Value.
    ///
    /// This loads the context through the normal loader pipeline (L1 cache,
    /// registry, L2 cache, remote fetch) and converts the result.
    pub async fn resolve_context(&self, url: &str) -> Result<serde_json::Value, Error> {
        let value = self.loader.load_value(url).await?;
        Ok(json_syntax::Value::into_serde_json((*value).clone()))
    }

    /// Fetch a context directly from the remote source, bypassing all caches.
    ///
    /// Use this when the caller needs to verify that the original source is
    /// still reachable (e.g. delete-with-reload).
    pub async fn fetch_remote_context(&self, url: &str) -> Result<serde_json::Value, Error> {
        let value = self.loader.fetch_remote_value(url).await.map_err(|e| Error::from_loader_error(&e))?;
        Ok(json_syntax::Value::into_serde_json((*value).clone()))
    }
}

// ---------------------------------------------------------------------------
// Builder
// ---------------------------------------------------------------------------

/// Builder for [`JsonLdEngine`].
///
/// Configuration values are passed individually through builder methods
/// rather than via a configuration struct, keeping the library free of
/// external config-framework dependencies.
pub struct JsonLdEngineBuilder {
    registry: Arc<ContextRegistry>,
    l2_cache: Option<Arc<dyn ContextCache>>,
    allow_remote: bool,
    timeout_seconds: u64,
    max_redirects: u32,
    cache_capacity: u64,
    cache_ttl_seconds: u64,
    max_context_bytes: u64,
    preload_contexts: Option<HashMap<String, String>>,
}

impl JsonLdEngineBuilder {
    /// Create a builder with sensible defaults.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(ContextRegistry::new()),
            l2_cache: None,
            allow_remote: true,
            timeout_seconds: 30,
            max_redirects: 5,
            cache_capacity: 1024,
            cache_ttl_seconds: 3600,
            max_context_bytes: 2_000_000,
            preload_contexts: None,
        }
    }

    /// Whether remote HTTP context fetching is allowed.
    pub fn allow_remote(mut self, allow: bool) -> Self {
        self.allow_remote = allow;
        self
    }

    /// HTTP request timeout in seconds.
    pub fn timeout_seconds(mut self, seconds: u64) -> Self {
        self.timeout_seconds = seconds;
        self
    }

    /// Maximum number of HTTP redirects to follow.
    pub fn max_redirects(mut self, max: u32) -> Self {
        self.max_redirects = max;
        self
    }

    /// Maximum number of entries in the in-memory (L1) context cache.
    pub fn cache_capacity(mut self, capacity: u64) -> Self {
        self.cache_capacity = capacity;
        self
    }

    /// Time-to-live for L1 cache entries, in seconds.
    pub fn cache_ttl_seconds(mut self, ttl: u64) -> Self {
        self.cache_ttl_seconds = ttl;
        self
    }

    /// Maximum size (in bytes) of a single remote context document.
    pub fn max_context_bytes(mut self, max: u64) -> Self {
        self.max_context_bytes = max;
        self
    }

    /// Replace the context registry.
    pub fn with_registry(mut self, registry: Arc<ContextRegistry>) -> Self {
        self.registry = registry;
        self
    }

    /// Attach an L2 context cache (e.g. database-backed).
    pub fn with_l2_cache(mut self, cache: Arc<dyn ContextCache>) -> Self {
        self.l2_cache = Some(cache);
        self
    }

    /// Register a single context document at build time.
    pub fn register_context(self, url: impl Into<String>, value: json_syntax::Value) -> Self {
        self.registry.register(url.into(), value);
        self
    }

    /// Supply a map of `URL → file-path` for contexts to preload from disk.
    pub fn preload_contexts(mut self, contexts: HashMap<String, String>) -> Self {
        self.preload_contexts = Some(contexts);
        self
    }

    /// Build the engine.
    pub fn build(self) -> Result<JsonLdEngine, Error> {
        for url in NGSI_LD_CORE_CONTEXT_URL_LIST {
            let embedded_core = load_embedded_core_context(url)?;

            self.registry.register(url.to_string(), embedded_core.clone());
        }

        let data_integrity = load_embedded_data_integrity_context()?;
        self.registry.register(DATA_INTEGRITY_CONTEXT_URL.to_string(), data_integrity);

        let preloaded = register_preloaded_contexts(&self.preload_contexts, &self.registry)?;
        if preloaded.is_empty() {
            info!("JSON-LD preloaded contexts: none configured");
        } else {
            info!("JSON-LD preloaded contexts registered ({}): {:?}", preloaded.len(), preloaded);
        }

        let mut loader = HttpContextLoader::new(
            self.registry.clone(),
            self.allow_remote,
            self.timeout_seconds,
            self.max_redirects,
            self.cache_capacity,
            self.cache_ttl_seconds,
            self.max_context_bytes as usize,
        )?;
        if let Some(l2) = self.l2_cache {
            loader = loader.with_l2_cache(l2);
        }
        let term_cache = Cache::builder()
            .max_capacity(self.cache_capacity)
            .time_to_live(Duration::from_secs(self.cache_ttl_seconds))
            .build();

        Ok(JsonLdEngine {
            loader: Arc::new(loader),
            term_cache,
        })
    }
}

impl Default for JsonLdEngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn load_embedded_core_context(url: &str) -> Result<json_syntax::Value, Error> {
    let context = NGSI_LD_CORE_CONTEXT_MAP
        .get(url)
        .ok_or(Error::EmbeddedContextInvalid("Unable to obtain embedded context".to_string()))?;

    let (value, _) = json_syntax::Value::parse_str(context.value()).map_err(|e| Error::EmbeddedContextInvalid(e.to_string()))?;
    Ok(value)
}

fn load_embedded_data_integrity_context() -> Result<json_syntax::Value, Error> {
    let (value, _) = json_syntax::Value::parse_str(DATA_INTEGRITY_CONTEXT).map_err(|e| Error::EmbeddedContextInvalid(e.to_string()))?;
    Ok(value)
}

fn register_preloaded_contexts(entries: &Option<HashMap<String, String>>, registry: &Arc<ContextRegistry>) -> Result<Vec<(String, String)>, Error> {
    let Some(entries) = entries else {
        return Ok(Vec::new());
    };

    let mut loaded = Vec::with_capacity(entries.len());
    for (url, file) in entries {
        let raw = fs::read_to_string(file).map_err(|e| Error::ContextLoad {
            url: url.clone(),
            message: format!("Failed reading preloaded context file '{}': {}", file, e),
        })?;
        let (value, _) = json_syntax::Value::parse_str(&raw).map_err(|e| Error::ContextInvalid {
            url: url.clone(),
            message: format!("Invalid preloaded context file '{}': {}", file, e),
        })?;
        registry.register(url.clone(), value);
        loaded.push((url.clone(), file.clone()));
    }

    Ok(loaded)
}

fn build_context_reference(context: &ContextSpec) -> Result<RemoteContextReference<IriIndex>, Error> {
    let context_value = context.to_context_value();
    let jsonld_context = match context_value {
        Some(value) => {
            let syntax_value = json_syntax::Value::from_serde_json(value);
            JsonLdContext::try_from_json(syntax_value).map_err(|e| Error::ContextInvalid {
                url: "inline".to_string(),
                message: e.to_string(),
            })?
        }
        None => JsonLdContext::default(),
    };

    let remote_doc = RemoteDocument::new(None, None, jsonld_context);
    Ok(RemoteContextReference::Loaded(remote_doc))
}

fn strip_context(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(obj) => {
            obj.remove("@context");
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let serde_json::Value::Object(obj) = item {
                    obj.remove("@context");
                }
            }
        }
        _ => {}
    }
}

fn term_cache_key(prefix: &str, term: &str, context: &ContextSpec) -> String {
    let ctx_str = context::resolve_context_value(None, context).map(|v| v.to_string()).unwrap_or_default();
    format!("{prefix}:{term}:{ctx_str}")
}

fn is_full_iri(term: &str) -> bool {
    term.starts_with("http://") || term.starts_with("https://") || term.starts_with("urn:")
}

fn extract_context_urls(document: &serde_json::Value) -> Vec<String> {
    fn collect(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::String(url) => out.push(url.clone()),
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, out);
                }
            }
            serde_json::Value::Object(map) => {
                if let Some(ctx) = map.get("@context") {
                    collect(ctx, out);
                }
            }
            _ => {}
        }
    }

    let mut urls = Vec::new();
    if let Some(ctx) = document.get("@context") {
        collect(ctx, &mut urls);
    }
    urls
}

fn extract_applied_context(document: &serde_json::Value) -> Option<serde_json::Value> {
    match document {
        serde_json::Value::Object(map) => map.get("@context").cloned(),
        serde_json::Value::Array(items) => {
            let contexts: Vec<_> = items
                .iter()
                .filter_map(|item| item.as_object().and_then(|obj| obj.get("@context")).cloned())
                .collect();
            if contexts.is_empty() {
                None
            } else {
                Some(serde_json::Value::Array(contexts))
            }
        }
        _ => None,
    }
}

fn has_explicit_core_context(original_document: &serde_json::Value, spec: &ContextSpec) -> bool {
    if spec.link_contexts().iter().any(|url| is_ngsi_ld_core_context_url(url)) {
        return true;
    }

    if let Some(body_context) = spec.body_context() {
        if context_value_contains_core_url(body_context) {
            return true;
        }
    }

    original_document.get("@context").is_some_and(context_value_contains_core_url)
}

fn context_value_contains_core_url(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(url) => is_ngsi_ld_core_context_url(url),
        serde_json::Value::Array(items) => items.iter().any(context_value_contains_core_url),
        serde_json::Value::Object(map) => map.get("@context").is_some_and(context_value_contains_core_url),
        _ => false,
    }
}

/// Resolve relative context URLs in the document's `@context` before
/// expansion.  Paths like `/ngsi-ld/v1/jsonldContexts/{id}` are replaced
/// with the bare context identifier (a URN), which is a valid IRI that the
/// loader can resolve via the L2 cache.
fn resolve_relative_context_urls(document: &mut serde_json::Value) {
    const PREFIX: &str = "/ngsi-ld/v1/jsonldContexts/";

    fn resolve_entry(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(s) if s.starts_with(PREFIX) => {
                *s = s[PREFIX.len()..].to_string();
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    resolve_entry(item);
                }
            }
            _ => {}
        }
    }

    match document {
        serde_json::Value::Object(obj) => {
            if let Some(ctx) = obj.get_mut("@context") {
                resolve_entry(ctx);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let serde_json::Value::Object(obj) = item {
                    if let Some(ctx) = obj.get_mut("@context") {
                        resolve_entry(ctx);
                    }
                }
            }
        }
        _ => {}
    }
}

fn normalize_context_urls_for_logging(urls: &[String]) -> Vec<String> {
    use std::collections::HashSet;

    let mut seen = HashSet::new();
    let mut normalized = Vec::with_capacity(urls.len());
    for url in urls {
        let candidate = if is_ngsi_ld_core_context_url(url) {
            NGSI_LD_CORE_CONTEXT_URL.to_string()
        } else {
            url.clone()
        };
        if seen.insert(candidate.clone()) {
            normalized.push(candidate);
        }
    }
    normalized
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core_context::NGSI_LD_CORE_CONTEXT_V16_URL;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("build test engine")
    }

    #[tokio::test]
    async fn test_expand_basic() {
        let engine = test_engine();
        let input = json!({
            "@context": NGSI_LD_CORE_CONTEXT_URL,
            "id": "urn:ngsi-ld:Vehicle:001",
            "type": "Vehicle"
        });

        let result = engine.expand(input, &ContextSpec::none()).await;
        assert!(result.is_ok());
        let expanded = result.unwrap();
        assert!(expanded.is_array());
        assert_eq!(expanded[0].get("@id"), Some(&json!("urn:ngsi-ld:Vehicle:001")));
    }

    #[tokio::test]
    async fn test_expand_type() {
        let engine = test_engine();
        let iri = engine.expand_type("Vehicle", &ContextSpec::none()).await.unwrap();
        assert!(iri.contains("Vehicle"));
    }

    #[tokio::test]
    async fn test_expand_with_core_context_v16_alias() {
        let engine = test_engine();
        let input = json!({
            "@context": NGSI_LD_CORE_CONTEXT_V16_URL,
            "id": "urn:ngsi-ld:Vehicle:core-v16",
            "type": "Vehicle"
        });
        let result = engine.expand(input, &ContextSpec::none()).await;
        assert!(result.is_ok(), "expand error: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_preloaded_context_expands_without_remote_network() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time").as_nanos();
        let path = std::env::temp_dir().join(format!("engine-preload-{unique}.jsonld"));
        fs::write(&path, r#"{"@context":{"@vocab":"https://example.com/vocab#"}}"#).expect("write preloaded context");

        let input = json!({
            "@context": "https://example.com/context.jsonld",
            "id": "urn:ngsi-ld:Device:001",
            "type": "Device",
            "temperature": 21
        });

        let engine_without = JsonLdEngine::builder()
            .allow_remote(false)
            .build()
            .expect("build engine without preloaded context");
        let without = engine_without.expand(input.clone(), &ContextSpec::none()).await;
        assert!(without.is_err());

        let mut preloads = HashMap::new();
        preloads.insert("https://example.com/context.jsonld".to_string(), path.to_string_lossy().to_string());

        let engine_with = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("build engine with preloaded context");
        let with = engine_with.expand(input, &ContextSpec::none()).await;
        assert!(with.is_ok());

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn test_explicit_remote_context_that_imports_core_expands() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time").as_nanos();
        let path = std::env::temp_dir().join(format!("engine-core-importing-{unique}.jsonld"));
        fs::write(
            &path,
            r#"{
              "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                {
                  "@vocab": "https://example.com/vocab#",
                  "Building": "https://example.com/vocab#Building",
                  "name": "https://example.com/vocab#name"
                }
              ]
            }"#,
        )
        .expect("write core-importing context");

        let context_url = "https://example.com/core-importing-context.jsonld";
        let mut preloads = HashMap::new();
        preloads.insert(context_url.to_string(), path.to_string_lossy().to_string());

        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine with preloaded context");

        let input = json!({
            "@context": context_url,
            "id": "urn:ngsi-ld:Building:core-import",
            "type": "Building",
            "name": { "type": "Property", "value": "x" }
        });
        let result = engine.expand(input, &ContextSpec::none()).await;
        assert!(result.is_ok(), "expand error: {:?}", result.err());

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn test_external_context_redefining_location_expands_after_normalization() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time").as_nanos();
        let path = std::env::temp_dir().join(format!("engine-location-redefining-context-{unique}.jsonld"));
        fs::write(
            &path,
            r#"{
              "@context": [
                "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld",
                {
                  "@vocab": "https://example.com/vocab#",
                  "Building": "https://example.com/vocab#Building",
                  "location": "https://example.com/vocab#location"
                }
              ]
            }"#,
        )
        .expect("write location-redefining context");

        let context_url = "https://example.com/location-redefining-context.jsonld";
        let mut preloads = HashMap::new();
        preloads.insert(context_url.to_string(), path.to_string_lossy().to_string());

        let engine = JsonLdEngine::builder()
            .allow_remote(false)
            .preload_contexts(preloads)
            .build()
            .expect("engine with preloaded context");

        let input = json!({
            "@context": context_url,
            "id": "urn:ngsi-ld:Building:location-redef",
            "type": "Building",
            "location": {
                "type": "GeoProperty",
                "value": {
                    "type": "Point",
                    "coordinates": [13.3986, 52.5547]
                }
            }
        });
        let result = engine.expand(input, &ContextSpec::none()).await;
        assert!(result.is_ok(), "expand error: {:?}", result.err());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn test_preloaded_context_missing_file_fails_build() {
        let mut preloads = HashMap::new();
        preloads.insert(
            "https://example.com/context.jsonld".to_string(),
            "/tmp/engine-missing-context.jsonld".to_string(),
        );

        let result = JsonLdEngine::builder().preload_contexts(preloads).build();
        assert!(matches!(result, Err(Error::ContextLoad { .. })));
    }

    #[test]
    fn test_preloaded_context_invalid_json_fails_build() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time").as_nanos();
        let path = std::env::temp_dir().join(format!("engine-invalid-preload-{unique}.jsonld"));
        fs::write(&path, "{not-json").expect("write invalid context");

        let mut preloads = HashMap::new();
        preloads.insert("https://example.com/context.jsonld".to_string(), path.to_string_lossy().to_string());

        let result = JsonLdEngine::builder().preload_contexts(preloads).build();
        assert!(matches!(result, Err(Error::ContextInvalid { .. })));
        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn test_expand_types_comma_separated() {
        let engine = test_engine();
        let types = engine.expand_types("Vehicle, Building", &ContextSpec::none()).await.unwrap();
        assert_eq!(types.len(), 2);
    }

    #[tokio::test]
    async fn test_expand_already_iri() {
        let engine = test_engine();
        let iri = engine.expand_type("http://example.com/Type", &ContextSpec::none()).await.unwrap();
        assert_eq!(iri, "http://example.com/Type");
    }

    #[tokio::test]
    async fn test_expand_array_document() {
        let engine = test_engine();
        let input = json!([
            {
                "@context": NGSI_LD_CORE_CONTEXT_URL,
                "id": "urn:ngsi-ld:Vehicle:001",
                "type": "Vehicle"
            }
        ]);

        let result = engine.expand(input, &ContextSpec::none()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_array());
    }
}
