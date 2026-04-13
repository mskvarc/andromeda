//! JSON-LD processing engine for NGSI-LD.
//!
//! [`JsonLdEngine`] provides expand and compact operations.  Use
//! [`JsonLdEngineBuilder`] to configure and construct an engine instance.

use crate::jsonld::{
    Error,
    context::{self, ContextSpec, CoreContextPolicy},
    core_context::{
        ANDROMEDA_CORE_CONTEXT,
        ANDROMEDA_CORE_CONTEXT_URL,
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
    syntax::{Context as JsonLdContext, IntoJson, TryFromJson},
};
use json_syntax::Parse;
use moka::future::Cache;
use rdf_types::vocabulary::{BlankIdIndex, IndexVocabulary, IriIndex};
use std::{
    collections::HashMap,
    fs,
    hash::Hash,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tracing::info;

// ---------------------------------------------------------------------------
// ContextStripping
// ---------------------------------------------------------------------------

/// Controls whether `@context` is stripped from compacted output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, strum::Display)]
pub enum ContextStripping {
    /// Remove `@context` from the compacted output.
    #[default]
    Strip,
    /// Keep `@context` in the compacted output.
    Keep,
}

// ---------------------------------------------------------------------------
// RemoteContextFetch
// ---------------------------------------------------------------------------

/// Controls whether the engine is allowed to fetch remote contexts over HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
pub enum RemoteContextFetch {
    /// Allow fetching remote JSON-LD contexts.
    Allow,
    /// Deny all remote context fetches.
    Deny,
}

impl From<bool> for RemoteContextFetch {
    /// `true` → [`Allow`](RemoteContextFetch::Allow),
    /// `false` → [`Deny`](RemoteContextFetch::Deny).
    fn from(allow: bool) -> RemoteContextFetch {
        if allow { RemoteContextFetch::Allow } else { RemoteContextFetch::Deny }
    }
}

// ---------------------------------------------------------------------------
// CompactOptions
// ---------------------------------------------------------------------------

/// Options for JSON-LD compaction.
#[derive(Debug, Clone, Copy, Default)]
pub struct CompactOptions {
    /// Whether to strip `@context` from the compacted output.
    pub strip_context: ContextStripping,
}

// ---------------------------------------------------------------------------
// JsonLdEngine
// ---------------------------------------------------------------------------

/// JSON-LD processing engine.
///
/// Provides [`expand`](Self::expand) and [`compact`](Self::compact) operations
/// backed by a configurable context loader with multi-tier caching.
///
/// Cloning is cheap — the loader and caches are shared via `Arc`.
#[derive(Clone, Debug)]
pub struct JsonLdEngine {
    loader: Arc<HttpContextLoader>,
    /// Cache for expanded type/term IRIs to avoid creating synthetic documents
    /// on every call.  Keyed by `"{type|term}:{short_name}:{context_hash}"`.
    term_cache: Cache<String, String>,
    term_cache_hits: Arc<AtomicU64>,
    term_cache_misses: Arc<AtomicU64>,
}

impl JsonLdEngine {
    /// Start building an engine with custom configuration.
    pub fn builder() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
    }

    /// Expand a JSON-LD document to its canonical (expanded) form.
    ///
    /// Short property names are converted to full IRIs.  The result is always
    /// a JSON array of node objects.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Expansion`] on expansion failure, or a context-loading
    /// error variant if a required context could not be fetched.
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

        let expanded_json = json_syntax::Value::into_serde_json(expanded.into_iter().collect::<Vec<_>>().into_with(&vocabulary).into_json());
        Ok(expanded_json)
    }

    /// Compact a JSON-LD document using a context.
    ///
    /// Full IRIs are converted back to short property names.  If compaction
    /// fails due to a protected-term redefinition the engine falls back to
    /// compacting with the core context only.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Compaction`] on compaction failure, or a
    /// context-loading error if a required context could not be fetched.
    pub async fn compact(&self, document: serde_json::Value, context: &ContextSpec, options: CompactOptions) -> Result<serde_json::Value, Error> {
        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();
        let context_ref = build_context_reference(context)?;

        // Extract doc_id before conversion — used only for error logging on the
        // non-protected-term error path, avoiding the need to keep the full
        // document alive just for diagnostics.
        let doc_id_for_logging = document
            .as_object()
            .and_then(|o| o.get("@id").or(o.get("id")))
            .and_then(|v| v.as_str())
            .map(str::to_owned);

        // One clone is unavoidable: compact_with consumes RemoteDocument, but
        // the protected-term fallback path needs the original document to retry
        // with the core context.
        let syntax_value = json_syntax::Value::from_serde_json(document.clone());
        let remote_doc: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, syntax_value);

        let compacted = match remote_doc.compact_with(&mut vocabulary, context_ref, &*self.loader).await {
            Ok(result) => result,
            Err(e) => {
                if Error::is_protected_term_error(&e) {
                    tracing::warn!(
                        "Compaction failed with protected term error: {}. \
                         Falling back to core-only context (user-provided context will be dropped). \
                         This may produce unexpected short names in the output. \
                         Consider reviewing the context for protected term conflicts.",
                        e
                    );
                    let core_context = ContextSpec::core_only();
                    let core_context_ref = build_context_reference(&core_context)?;
                    let mut fallback_vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();
                    // Consume the original document here — no further clones needed.
                    let fallback_remote_doc: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, json_syntax::Value::from_serde_json(document));
                    fallback_remote_doc
                        .compact_with(&mut fallback_vocabulary, core_context_ref, &*self.loader)
                        .await
                        .map_err(|e| {
                            tracing::error!("Compaction failed even with core context: {}", e);
                            e
                        })?
                } else {
                    tracing::error!(
                        error = %e,
                        document_id = doc_id_for_logging.as_deref().unwrap_or("<unknown>"),
                        "Compaction failed"
                    );
                    return Err(e.into());
                }
            }
        };

        let mut compacted_json = json_syntax::Value::into_serde_json(compacted);

        if options.strip_context == ContextStripping::Strip {
            strip_context(&mut compacted_json);
        }

        Ok(compacted_json)
    }

    /// Expand a single type term to its full IRI.
    ///
    /// Results are cached per `(term, context)` pair to avoid constructing
    /// and expanding a synthetic JSON-LD document on every call.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidIri`] if the type term cannot be expanded.
    pub async fn expand_type(&self, type_term: &str, context: &ContextSpec) -> Result<String, Error> {
        if is_full_iri(type_term) {
            return Ok(type_term.to_string());
        }

        let cache_key = term_cache_key("type", type_term, context);
        if let Some(cached) = self.term_cache.get(&cache_key).await {
            self.record_term_cache_hit();
            return Ok(cached);
        }
        self.record_term_cache_miss();

        let mut doc = serde_json::json!({
            "id": "urn:ngsi-ld:_:temp",
            "type": type_term
        });

        if let Some(ctx) = context::resolve_context_value(None, context)
            && let Some(obj) = doc.as_object_mut()
        {
            obj.insert("@context".to_string(), ctx);
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

    /// Expand a single attribute / property term to its full IRI.
    ///
    /// Results are cached per `(term, context)` pair to avoid constructing
    /// and expanding a synthetic JSON-LD document on every call.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidIri`] if the term cannot be resolved to a
    /// full IRI through the context.
    pub async fn expand_term(&self, term: &str, context: &ContextSpec) -> Result<String, Error> {
        if is_full_iri(term) {
            return Ok(term.to_string());
        }

        let cache_key = term_cache_key("term", term, context);
        if let Some(cached) = self.term_cache.get(&cache_key).await {
            self.record_term_cache_hit();
            return Ok(cached);
        }
        self.record_term_cache_miss();

        let mut doc = serde_json::json!({
            term: {"@id": "urn:dummy"}
        });

        if let Some(ctx) = context::resolve_context_value(None, context)
            && let Some(obj) = doc.as_object_mut()
        {
            obj.insert("@context".to_string(), ctx);
        }

        let expanded = self.expand(doc, context).await?;

        let iri = expanded
            .get(0)
            .and_then(|v| v.as_object())
            .and_then(|obj| obj.keys().find(|key| *key != "@id" && *key != "@type").cloned())
            .ok_or_else(|| Error::invalid_iri(format!("Failed to expand term: {term}")))?;

        self.term_cache.insert(cache_key, iri.clone()).await;
        Ok(iri)
    }

    fn record_term_cache_hit(&self) {
        let hits = self.term_cache_hits.fetch_add(1, Ordering::Relaxed) + 1;
        if hits.is_multiple_of(1000) {
            self.log_term_cache_stats();
        }
    }

    fn record_term_cache_miss(&self) {
        let misses = self.term_cache_misses.fetch_add(1, Ordering::Relaxed) + 1;
        if misses.is_multiple_of(1000) {
            self.log_term_cache_stats();
        }
    }

    fn log_term_cache_stats(&self) {
        let hits = self.term_cache_hits.load(Ordering::Relaxed);
        let misses = self.term_cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            return;
        }
        let hit_rate = (hits as f64 / total as f64) * 100.0;
        tracing::debug!(hits, misses, total, hit_rate = format_args!("{hit_rate:.2}%"), "JSON-LD term cache statistics");
    }

    /// Resolve a context URL and return its payload as a `serde_json::Value`.
    ///
    /// This loads the context through the normal loader pipeline (L1 cache,
    /// registry, L2 cache, remote fetch) and converts the result.
    ///
    /// # Errors
    ///
    /// Returns a context-loading error if the URL cannot be resolved.
    pub async fn resolve_context(&self, url: &str) -> Result<serde_json::Value, Error> {
        let value = self.loader.load_value(url).await?;
        Ok(json_syntax::Value::into_serde_json(Arc::unwrap_or_clone(value)))
    }

    /// Fetch a context directly from the remote source, bypassing all caches.
    ///
    /// Use this when the caller needs to verify that the original source is
    /// still reachable (e.g. delete-with-reload).
    ///
    /// # Errors
    ///
    /// Returns a context-loading error if the remote fetch fails.
    pub async fn fetch_remote_context(&self, url: &str) -> Result<serde_json::Value, Error> {
        let value = self.loader.fetch_remote_value(url).await.map_err(|e| Error::from_loader_error(&e))?;
        Ok(json_syntax::Value::into_serde_json(Arc::unwrap_or_clone(value)))
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
    allow_remote: RemoteContextFetch,
    timeout_seconds: u64,
    max_redirects: u32,
    cache_capacity: u64,
    cache_ttl_seconds: u64,
    max_context_bytes: u64,
    preload_contexts: Option<HashMap<String, String>>,
}

impl JsonLdEngineBuilder {
    /// Create a builder with sensible defaults.
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

    /// Whether remote HTTP context fetching is allowed.
    #[must_use]
    pub fn allow_remote(mut self, allow: impl Into<RemoteContextFetch>) -> JsonLdEngineBuilder {
        self.allow_remote = allow.into();
        self
    }

    /// HTTP request timeout in seconds.
    #[must_use]
    pub const fn timeout_seconds(mut self, seconds: u64) -> JsonLdEngineBuilder {
        self.timeout_seconds = seconds;
        self
    }

    /// Maximum number of HTTP redirects to follow.
    #[must_use]
    pub const fn max_redirects(mut self, max: u32) -> JsonLdEngineBuilder {
        self.max_redirects = max;
        self
    }

    /// Maximum number of entries in the in-memory (L1) context cache.
    #[must_use]
    pub const fn cache_capacity(mut self, capacity: u64) -> JsonLdEngineBuilder {
        self.cache_capacity = capacity;
        self
    }

    /// Time-to-live for L1 cache entries, in seconds.
    #[must_use]
    pub const fn cache_ttl_seconds(mut self, ttl: u64) -> JsonLdEngineBuilder {
        self.cache_ttl_seconds = ttl;
        self
    }

    /// Maximum size (in bytes) of a single remote context document.
    #[must_use]
    pub const fn max_context_bytes(mut self, max: u64) -> JsonLdEngineBuilder {
        self.max_context_bytes = max;
        self
    }

    /// Attach an L2 context cache (e.g. database-backed).
    #[must_use]
    pub fn with_l2_cache(mut self, cache: Arc<dyn ContextCache>) -> JsonLdEngineBuilder {
        self.l2_cache = Some(cache);
        self
    }

    /// Supply a map of `URL → file-path` for contexts to preload from disk.
    #[must_use]
    pub fn preload_contexts(mut self, contexts: HashMap<String, String>) -> JsonLdEngineBuilder {
        self.preload_contexts = Some(contexts);
        self
    }

    /// Build the engine.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if embedded core contexts cannot be parsed or if
    /// preloaded context files are missing / invalid.
    pub fn build(self) -> Result<JsonLdEngine, Error> {
        for url in NGSI_LD_CORE_CONTEXT_URL_LIST {
            let embedded_core = load_embedded_core_context(url)?;

            self.registry.register(url.to_string(), embedded_core.clone());
        }

        let andromeda_core = load_embedded_context(ANDROMEDA_CORE_CONTEXT, ANDROMEDA_CORE_CONTEXT_URL)?;
        self.registry.register(ANDROMEDA_CORE_CONTEXT_URL.to_string(), andromeda_core);

        let data_integrity = load_embedded_context(DATA_INTEGRITY_CONTEXT, DATA_INTEGRITY_CONTEXT_URL)?;
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
            term_cache_hits: Arc::new(AtomicU64::new(0)),
            term_cache_misses: Arc::new(AtomicU64::new(0)),
        })
    }
}

impl Default for JsonLdEngineBuilder {
    fn default() -> JsonLdEngineBuilder {
        JsonLdEngineBuilder::new()
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn load_embedded_core_context(url: &str) -> Result<json_syntax::Value, Error> {
    let context = NGSI_LD_CORE_CONTEXT_MAP
        .get(url)
        .ok_or(Error::EmbeddedContextInvalid("Unable to obtain embedded context".to_string()))?;

    let (value, _) = json_syntax::Value::parse_str(context).map_err(|e| Error::EmbeddedContextInvalid(e.to_string()))?;
    Ok(value)
}

/// Parse an embedded context payload into a [`json_syntax::Value`].
fn load_embedded_context(payload: &str, url: &str) -> Result<json_syntax::Value, Error> {
    let (value, _) = json_syntax::Value::parse_str(payload).map_err(|e| Error::EmbeddedContextInvalid(format!("{}: {}", url, e)))?;
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
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => {}
    }
}

fn term_cache_key(prefix: &str, term: &str, context: &ContextSpec) -> String {
    use std::hash::Hasher;
    let ctx_hash = context::resolve_context_value(None, context).map_or(0u64, |v| {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        v.to_string().hash(&mut hasher);
        hasher.finish()
    });
    format!("{prefix}:{term}:{ctx_hash:x}")
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
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
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
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => None,
    }
}

fn has_explicit_core_context(original_document: &serde_json::Value, spec: &ContextSpec) -> bool {
    if spec.link_contexts().iter().any(|url| is_ngsi_ld_core_context_url(url)) {
        return true;
    }

    if let Some(body_context) = spec.body_context()
        && context_value_contains_core_url(body_context)
    {
        return true;
    }

    original_document.get("@context").is_some_and(context_value_contains_core_url)
}

fn context_value_contains_core_url(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(url) => is_ngsi_ld_core_context_url(url),
        serde_json::Value::Array(items) => items.iter().any(context_value_contains_core_url),
        serde_json::Value::Object(map) => map.get("@context").is_some_and(context_value_contains_core_url),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => false,
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
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_)
            | serde_json::Value::Object(_) => {}
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
                if let serde_json::Value::Object(obj) = item
                    && let Some(ctx) = obj.get_mut("@context")
                {
                    resolve_entry(ctx);
                }
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => {}
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
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::jsonld::core_context::NGSI_LD_CORE_CONTEXT_V16_URL;
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
        pretty_assertions::assert_eq!(expanded[0].get("@id"), Some(&json!("urn:ngsi-ld:Vehicle:001")));
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
    async fn test_expand_already_iri() {
        let engine = test_engine();
        let iri = engine.expand_type("http://example.com/Type", &ContextSpec::none()).await.unwrap();
        pretty_assertions::assert_eq!(iri, "http://example.com/Type");
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
