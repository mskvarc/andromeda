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
    vocab,
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

/// Per-batch JSON-LD expansion handle.
///
/// Captures the resolved `@context` value for a batch (the caller's
/// [`ContextSpec`] post-merge with core policy) so that individual entity
/// expansions can skip the `resolve_context_value` rebuild on every call.
///
/// When the batch's effective context resolves to the NGSI-LD core + Andromeda
/// core only (no user-supplied contexts), the handle also carries a
/// precomputed [`CoreTermMap`] that enables an in-process fastpath expander.
/// That fastpath walks the `serde_json::Value` DOM once and synthesises the
/// expanded form directly, skipping the
/// `serde_json → json_syntax → json-ld expand → serde_json` round-trip.
///
/// Entities whose inline `@context` diverges from the batch's resolved value
/// fall back to the full per-entity [`JsonLdEngine::expand`] path, so the
/// spec rule allowing per-entity context overrides (NGSI-LD 5.5.7) is
/// preserved byte-for-byte.
#[derive(Clone, Debug)]
pub struct BatchContextHandle {
    spec: ContextSpec,
    resolved_context: Option<serde_json::Value>,
    core_term_map: Option<Arc<CoreTermMap>>,
}

/// Hardcoded NGSI-LD core context term → IRI mapping used by
/// [`JsonLdEngine::expand_with_handle`]'s in-process fastpath.
///
/// Only populated on [`BatchContextHandle`]s whose spec resolves to the
/// NGSI-LD core + Andromeda core bundle (no user-supplied link or body
/// contexts). Non-core specs keep the handle's term map as `None`, which
/// disables the fastpath entirely.
#[derive(Debug)]
struct CoreTermMap {
    /// `@vocab` prefix applied to unknown terms (e.g. a user's
    /// `speed` attribute becomes `<vocab>speed`). Always the NGSI-LD
    /// default-context prefix when this map is populated.
    vocab: &'static str,
}

/// NGSI-LD default-context prefix (`@vocab` in the core context).
const NGSI_LD_DEFAULT_CONTEXT_PREFIX: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

impl CoreTermMap {
    /// Expand a short NGSI-LD term to its full IRI using the compile-time
    /// generated lookup table, falling back to the `@vocab` prefix for
    /// user-defined terms.
    fn expand_iri(&self, short: &str) -> String {
        if is_full_iri(short) {
            return short.to_string();
        }
        if let Some(iri) = vocab::compact_to_expanded(short) {
            return iri.to_string();
        }
        format!("{}{}", self.vocab, short)
    }

    /// Return the `@type` coercion IRI for a core context term, if one is
    /// declared in the embedded NGSI-LD core context
    /// (e.g. `observedAt → DateTime`, `datasetId → @id`). `None` means the
    /// term carries no `@type` coercion.
    fn coercion(short: &str) -> Option<TermCoercion> {
        match short {
            "observedAt" | "createdAt" | "modifiedAt" | "expiresAt" | "deletedAt" | "startAt" | "endAt" | "timeAt" | "instanceTimeAt" | "previousTime"
            | "timestamp" | "notifiedAt" | "lastNotification" | "lastSuccess" | "lastFailure" => Some(TermCoercion::DateTime),
            "datasetId" | "instanceId" => Some(TermCoercion::Id),
            _other => None,
        }
    }
}

/// `@type` coercion applied to attribute metadata values by the NGSI-LD
/// core context.
#[derive(Debug, Clone, Copy)]
enum TermCoercion {
    /// Value is emitted as `{"@type": "<DateTime IRI>", "@value": "<ISO>"}`.
    DateTime,
    /// Value is emitted as `{"@id": "<value>"}`.
    Id,
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
        let document = context::apply_context(document, context)?;
        self.expand_prepared(document, context.core_policy()).await
    }

    /// Build a per-batch expansion handle for a shared `@context`.
    ///
    /// The caller typically constructs one handle per batch and hands it to
    /// [`Self::expand_with_handle`] for every entity in the batch.
    pub fn batch_context_handle(&self, context: &ContextSpec) -> BatchContextHandle {
        BatchContextHandle {
            resolved_context: context::resolve_context_value(None, context),
            core_term_map: build_core_term_map(context),
            spec: context.clone(),
        }
    }

    /// Expand one entity of a batch using a shared [`BatchContextHandle`].
    ///
    /// If the handle carries a precomputed core term map, the engine first
    /// attempts an in-process fastpath that walks the `serde_json::Value`
    /// DOM once and emits the expanded form directly. Entities the fastpath
    /// cannot prove equivalent (inline `@context` override, unsupported
    /// NGSI-LD attribute shape, user-supplied contexts) fall through to
    /// [`Self::expand`] so NGSI-LD 5.5.7 semantics are preserved
    /// byte-for-byte.
    pub async fn expand_with_handle(&self, mut document: serde_json::Value, handle: &BatchContextHandle) -> Result<serde_json::Value, Error> {
        if let Some(term_map) = handle.core_term_map.as_ref()
            && let Some(expanded) = fast_expand_entity_document(&document, term_map)
        {
            return Ok(expanded);
        }
        match handle_fast_path_apply(&mut document, handle) {
            FastPath::Hit => self.expand_prepared(document, handle.spec.core_policy()).await,
            FastPath::Miss => self.expand(document, &handle.spec).await,
        }
    }

    async fn expand_prepared(
        &self,
        mut document: serde_json::Value,
        core_policy: crate::jsonld::context::CoreContextPolicy,
    ) -> Result<serde_json::Value, Error> {
        resolve_relative_context_urls(&mut document);

        // Diagnostic-only fields (applied_context, context_urls, normalized_context_urls)
        // are expensive recursive traversals with repeated clones. Keep the happy
        // path zero-alloc for diagnostics; only clone the document's `@context`
        // for the error branch, since `from_serde_json` consumes the full doc.
        let diag_context = document.get("@context").cloned();
        let syntax_value = json_syntax::Value::from_serde_json(document);
        let remote_doc: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, syntax_value);

        let mut vocabulary: IndexVocabulary<IriIndex, BlankIdIndex> = IndexVocabulary::new();

        let expanded = match remote_doc.expand_with(&mut vocabulary, &*self.loader).await {
            Ok(expanded) => expanded,
            Err(err) => {
                let context_urls = diag_context.as_ref().map(extract_context_urls_from_ctx).unwrap_or_default();
                let normalized_context_urls = normalize_context_urls_for_logging(&context_urls);
                tracing::warn!(
                    "JSON-LD expansion failed. core_policy={:?}; applied_context={:?}; context_urls={:?}; normalized_context_urls={:?}; error={:?}",
                    core_policy,
                    diag_context,
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

/// Collect context URLs directly from a `@context` node (used on the error
/// path, when the container document has already been consumed).
fn extract_context_urls_from_ctx(ctx: &serde_json::Value) -> Vec<String> {
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
    collect(ctx, &mut urls);
    urls
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

/// Outcome of attempting to apply the shared batch handle to an entity.
enum FastPath {
    /// The entity had no inline `@context` or its context matches the batch
    /// handle exactly — the precomputed context was injected in place.
    Hit,
    /// The entity carries a divergent inline `@context` — caller must run
    /// the per-entity resolution path.
    Miss,
}

fn handle_fast_path_apply(document: &mut serde_json::Value, handle: &BatchContextHandle) -> FastPath {
    match document {
        serde_json::Value::Object(obj) => apply_handle_to_object(obj, handle),
        serde_json::Value::Array(items) => {
            for item in items {
                if let serde_json::Value::Object(obj) = item
                    && matches!(apply_handle_to_object(obj, handle), FastPath::Miss)
                {
                    return FastPath::Miss;
                }
            }
            FastPath::Hit
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => FastPath::Miss,
    }
}

fn apply_handle_to_object(obj: &mut serde_json::Map<String, serde_json::Value>, handle: &BatchContextHandle) -> FastPath {
    match obj.get("@context") {
        None => {
            if let Some(resolved) = handle.resolved_context.as_ref() {
                obj.insert("@context".to_string(), resolved.clone());
            }
            FastPath::Hit
        }
        Some(existing) if Some(existing) == handle.resolved_context.as_ref() => FastPath::Hit,
        Some(_) => FastPath::Miss,
    }
}

/// Build the precomputed core term map if the spec's effective `@context`
/// resolves to exactly the NGSI-LD core + Andromeda core bundle with no
/// user-supplied contexts. Any user-supplied link or body context disables
/// the fastpath because term IRIs might be overridden.
fn build_core_term_map(spec: &ContextSpec) -> Option<Arc<CoreTermMap>> {
    if spec.body_context().is_some() {
        return None;
    }
    // Only the Append / AppendIfNoExplicit / Only policies can yield a
    // safe fastpath; Omit leaves terms unresolved.
    match spec.core_policy() {
        CoreContextPolicy::Only | CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit => {}
        CoreContextPolicy::Omit => return None,
    }
    for url in spec.link_contexts() {
        if !is_ngsi_ld_core_context_url(url) && url != ANDROMEDA_CORE_CONTEXT_URL {
            return None;
        }
    }
    Some(Arc::new(CoreTermMap {
        vocab: NGSI_LD_DEFAULT_CONTEXT_PREFIX,
    }))
}

/// Produce the expanded JSON-LD form for a whole document (single entity or
/// an array of entities) using only the precomputed core term map.
///
/// Returns `None` the moment any substructure the fastpath cannot prove
/// byte-equivalent to [`json_ld::JsonLdProcessor::expand`] is encountered,
/// so the caller falls back to the full expander.
fn fast_expand_entity_document(document: &serde_json::Value, term_map: &CoreTermMap) -> Option<serde_json::Value> {
    match document {
        serde_json::Value::Object(obj) => {
            let expanded = fast_expand_entity(obj, term_map)?;
            Some(serde_json::Value::Array(vec![expanded]))
        }
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let obj = item.as_object()?;
                out.push(fast_expand_entity(obj, term_map)?);
            }
            Some(serde_json::Value::Array(out))
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => None,
    }
}

/// Produce the expanded JSON-LD object for a single NGSI-LD entity.
///
/// Only a conservative subset of shapes is supported; any unexpected key
/// or attribute form returns `None` to trigger the fallback expander.
fn fast_expand_entity(obj: &serde_json::Map<String, serde_json::Value>, term_map: &CoreTermMap) -> Option<serde_json::Value> {
    // Per-entity `@context` is only acceptable when it matches the NGSI-LD
    // core context family; anything else must fall back.
    if let Some(ctx) = obj.get("@context")
        && !context_matches_core_only(ctx)
    {
        return None;
    }

    let mut expanded = serde_json::Map::with_capacity(obj.len());

    for (key, value) in obj {
        match key.as_str() {
            "@context" => {}
            "id" | "@id" => {
                let id = value.as_str()?;
                expanded.insert("@id".to_string(), serde_json::Value::String(id.to_owned()));
            }
            "type" | "@type" => {
                let iris = match value {
                    serde_json::Value::String(s) => vec![serde_json::Value::String(term_map.expand_iri(s))],
                    serde_json::Value::Array(arr) => {
                        let mut out = Vec::with_capacity(arr.len());
                        for item in arr {
                            let s = item.as_str()?;
                            out.push(serde_json::Value::String(term_map.expand_iri(s)));
                        }
                        out
                    }
                    serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Object(_) => return None,
                };
                expanded.insert("@type".to_string(), serde_json::Value::Array(iris));
            }
            "scope" => {
                let scope_values = fast_expand_scope(value)?;
                expanded.insert(vocab::expanded::SCOPE.as_str().to_owned(), scope_values);
            }
            other if other.starts_with('@') => return None,
            attr_name => {
                let iri = term_map.expand_iri(attr_name);
                let attr_expanded = fast_expand_attribute(value, term_map)?;
                expanded.insert(iri, attr_expanded);
            }
        }
    }

    Some(serde_json::Value::Object(expanded))
}

/// Accept `@context` whose value is a single NGSI-LD core URL or an array
/// of core URLs. Everything else disables the fastpath for the entity.
fn context_matches_core_only(ctx: &serde_json::Value) -> bool {
    match ctx {
        serde_json::Value::String(url) => is_ngsi_ld_core_context_url(url) || url == ANDROMEDA_CORE_CONTEXT_URL,
        serde_json::Value::Array(items) => items.iter().all(|item| {
            item.as_str()
                .is_some_and(|url| is_ngsi_ld_core_context_url(url) || url == ANDROMEDA_CORE_CONTEXT_URL)
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Object(_) => false,
    }
}

/// Expand the NGSI-LD `scope` property. Supported shapes: a single string
/// scope path or an array of string scope paths.
fn fast_expand_scope(value: &serde_json::Value) -> Option<serde_json::Value> {
    let items = match value {
        serde_json::Value::String(_) => std::slice::from_ref(value),
        serde_json::Value::Array(arr) => arr.as_slice(),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Object(_) => return None,
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let s = item.as_str()?;
        let mut v = serde_json::Map::with_capacity(1);
        v.insert("@value".to_owned(), serde_json::Value::String(s.to_owned()));
        out.push(serde_json::Value::Object(v));
    }
    Some(serde_json::Value::Array(out))
}

/// Expand a single attribute's value to its JSON-LD `[...]`-wrapped
/// expanded form. Accepts either a single instance object or an array of
/// instance objects (multi-dataset attribute).
fn fast_expand_attribute(value: &serde_json::Value, term_map: &CoreTermMap) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Object(instance) => {
            let expanded = fast_expand_attribute_instance(instance, term_map)?;
            Some(serde_json::Value::Array(vec![expanded]))
        }
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let instance = item.as_object()?;
                out.push(fast_expand_attribute_instance(instance, term_map)?);
            }
            Some(serde_json::Value::Array(out))
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => None,
    }
}

/// Expand a single attribute instance object into its JSON-LD node form
/// (without the enclosing array).
fn fast_expand_attribute_instance(instance: &serde_json::Map<String, serde_json::Value>, term_map: &CoreTermMap) -> Option<serde_json::Value> {
    let attr_type = instance.get("type")?.as_str()?;
    let kind = AttrKind::from_type_name(attr_type)?;

    let mut out = serde_json::Map::with_capacity(instance.len());
    out.insert(
        "@type".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::String(kind.iri().to_owned())]),
    );

    let value_emitted = fast_emit_attribute_value(&mut out, instance, kind, term_map)?;
    if !value_emitted {
        return None;
    }

    for (key, value) in instance {
        if key == "type" || kind.is_value_key(key) {
            continue;
        }
        let expanded = fast_expand_attribute_metadata(key, value, term_map)?;
        let (iri, node) = expanded;
        out.insert(iri, node);
    }

    Some(serde_json::Value::Object(out))
}

/// Supported NGSI-LD attribute kinds for the fastpath.
#[derive(Clone, Copy)]
enum AttrKind {
    Property,
    Relationship,
    GeoProperty,
    LanguageProperty,
    VocabProperty,
    JsonProperty,
}

impl AttrKind {
    fn from_type_name(name: &str) -> Option<AttrKind> {
        match name {
            "Property" => Some(AttrKind::Property),
            "Relationship" => Some(AttrKind::Relationship),
            "GeoProperty" => Some(AttrKind::GeoProperty),
            "LanguageProperty" => Some(AttrKind::LanguageProperty),
            "VocabProperty" => Some(AttrKind::VocabProperty),
            "JsonProperty" => Some(AttrKind::JsonProperty),
            _other => None,
        }
    }

    fn iri(self) -> &'static str {
        match self {
            AttrKind::Property => vocab::expanded::PROPERTY.as_str(),
            AttrKind::Relationship => vocab::expanded::RELATIONSHIP.as_str(),
            AttrKind::GeoProperty => vocab::expanded::GEO_PROPERTY.as_str(),
            AttrKind::LanguageProperty => vocab::expanded::LANGUAGE_PROPERTY.as_str(),
            AttrKind::VocabProperty => vocab::expanded::VOCAB_PROPERTY.as_str(),
            AttrKind::JsonProperty => vocab::expanded::JSON_PROPERTY.as_str(),
        }
    }

    fn is_value_key(self, key: &str) -> bool {
        match self {
            AttrKind::Property | AttrKind::GeoProperty => key == "value",
            AttrKind::Relationship => key == "object",
            AttrKind::LanguageProperty => key == "languageMap",
            AttrKind::VocabProperty => key == "vocab",
            AttrKind::JsonProperty => key == "json",
        }
    }
}

/// Emit the core value of an attribute into the pre-built expanded node
/// object. Returns `Some(true)` when the value key is present and the
/// shape is supported, and `None` when the shape cannot be expanded
/// safely (caller falls back to the slow path).
fn fast_emit_attribute_value(
    out: &mut serde_json::Map<String, serde_json::Value>,
    instance: &serde_json::Map<String, serde_json::Value>,
    kind: AttrKind,
    term_map: &CoreTermMap,
) -> Option<bool> {
    match kind {
        AttrKind::Property => {
            let value = instance.get("value")?;
            let wrapped = fast_wrap_property_value(value)?;
            out.insert(vocab::expanded::VALUE.as_str().to_owned(), wrapped);
            Some(true)
        }
        AttrKind::Relationship => {
            let object = instance.get("object")?.as_str()?;
            let mut node = serde_json::Map::with_capacity(1);
            node.insert("@id".to_owned(), serde_json::Value::String(object.to_owned()));
            out.insert(
                vocab::expanded::OBJECT.as_str().to_owned(),
                serde_json::Value::Array(vec![serde_json::Value::Object(node)]),
            );
            Some(true)
        }
        AttrKind::GeoProperty => {
            let value = instance.get("value")?.as_object()?;
            let wrapped = fast_expand_geojson_node(value)?;
            out.insert(vocab::expanded::VALUE.as_str().to_owned(), serde_json::Value::Array(vec![wrapped]));
            Some(true)
        }
        AttrKind::LanguageProperty => {
            let map = instance.get("languageMap")?.as_object()?;
            let wrapped = fast_expand_language_map(map)?;
            out.insert(vocab::expanded::LANGUAGE_MAP.as_str().to_owned(), wrapped);
            Some(true)
        }
        AttrKind::VocabProperty => {
            let vocab_value = instance.get("vocab")?;
            let wrapped = fast_expand_vocab_value(vocab_value, term_map)?;
            out.insert(vocab::expanded::VOCAB.as_str().to_owned(), wrapped);
            Some(true)
        }
        AttrKind::JsonProperty => {
            let json_value = instance.get("json")?;
            let mut node = serde_json::Map::with_capacity(2);
            node.insert("@type".to_owned(), serde_json::Value::String("@json".to_owned()));
            node.insert("@value".to_owned(), json_value.clone());
            out.insert(
                vocab::expanded::JSON.as_str().to_owned(),
                serde_json::Value::Array(vec![serde_json::Value::Object(node)]),
            );
            Some(true)
        }
    }
}

/// Wrap a `Property.value` scalar for the expanded form. Scalars become
/// `[{"@value": <scalar>}]`. Objects and arrays bail the fastpath.
fn fast_wrap_property_value(value: &serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::String(_) | serde_json::Value::Number(_) | serde_json::Value::Bool(_) => {
            let mut node = serde_json::Map::with_capacity(1);
            node.insert("@value".to_owned(), value.clone());
            Some(serde_json::Value::Array(vec![serde_json::Value::Object(node)]))
        }
        serde_json::Value::Null | serde_json::Value::Object(_) | serde_json::Value::Array(_) => None,
    }
}

/// Expand a GeoJSON geometry node (e.g. Point, Polygon) from compact to
/// expanded JSON-LD form using the core context's geojson vocab prefix.
fn fast_expand_geojson_node(obj: &serde_json::Map<String, serde_json::Value>) -> Option<serde_json::Value> {
    let geom_type = obj.get("type")?.as_str()?;
    let type_iri = vocab::compact_to_expanded(geom_type)?;

    let mut out = serde_json::Map::with_capacity(obj.len());
    out.insert(
        "@type".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::String(type_iri.to_owned())]),
    );

    for (key, value) in obj {
        match key.as_str() {
            "type" => {}
            "coordinates" => {
                let coords_iri = vocab::compact_to_expanded("coordinates")?;
                let list = fast_expand_geojson_coordinates(value)?;
                out.insert(coords_iri.to_owned(), serde_json::Value::Array(vec![list]));
            }
            "bbox" => {
                let bbox_iri = vocab::compact_to_expanded("bbox")?;
                let list = fast_expand_geojson_coordinates(value)?;
                out.insert(bbox_iri.to_owned(), serde_json::Value::Array(vec![list]));
            }
            _other => return None,
        }
    }
    Some(serde_json::Value::Object(out))
}

/// Recursively convert a GeoJSON coordinate structure to nested `@list`
/// wrappers. Leaf numbers become `{"@value": <n>}`.
fn fast_expand_geojson_coordinates(value: &serde_json::Value) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Array(items) => {
            let mut list = Vec::with_capacity(items.len());
            for item in items {
                list.push(fast_expand_geojson_coordinates(item)?);
            }
            let mut node = serde_json::Map::with_capacity(1);
            node.insert("@list".to_owned(), serde_json::Value::Array(list));
            Some(serde_json::Value::Object(node))
        }
        serde_json::Value::Number(_) => {
            let mut node = serde_json::Map::with_capacity(1);
            node.insert("@value".to_owned(), value.clone());
            Some(serde_json::Value::Object(node))
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::String(_) | serde_json::Value::Object(_) => None,
    }
}

/// Expand a `LanguageProperty.languageMap` object to JSON-LD form with
/// `@language` tagging per entry.
fn fast_expand_language_map(map: &serde_json::Map<String, serde_json::Value>) -> Option<serde_json::Value> {
    let mut out = Vec::with_capacity(map.len());
    for (lang, value) in map {
        let text = value.as_str()?;
        let mut node = serde_json::Map::with_capacity(2);
        node.insert("@language".to_owned(), serde_json::Value::String(lang.clone()));
        node.insert("@value".to_owned(), serde_json::Value::String(text.to_owned()));
        out.push(serde_json::Value::Object(node));
    }
    Some(serde_json::Value::Array(out))
}

/// Expand a `VocabProperty.vocab` value. Strings resolve via the core term
/// map (with `@vocab` fallback) to an `@id` node. Arrays of strings emit
/// multiple nodes.
fn fast_expand_vocab_value(value: &serde_json::Value, term_map: &CoreTermMap) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::String(s) => {
            let iri = term_map.expand_iri(s);
            let mut node = serde_json::Map::with_capacity(1);
            node.insert("@id".to_owned(), serde_json::Value::String(iri));
            Some(serde_json::Value::Array(vec![serde_json::Value::Object(node)]))
        }
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let s = item.as_str()?;
                let iri = term_map.expand_iri(s);
                let mut node = serde_json::Map::with_capacity(1);
                node.insert("@id".to_owned(), serde_json::Value::String(iri));
                out.push(serde_json::Value::Object(node));
            }
            Some(serde_json::Value::Array(out))
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Object(_) => None,
    }
}

/// Expand an attribute-level metadata key such as `observedAt`, `datasetId`,
/// `unitCode`, or a nested sub-attribute (Property / Relationship /
/// GeoProperty / …). Unknown keys or unsupported shapes bail the fastpath.
fn fast_expand_attribute_metadata(key: &str, value: &serde_json::Value, term_map: &CoreTermMap) -> Option<(String, serde_json::Value)> {
    let iri = term_map.expand_iri(key);
    let coercion = CoreTermMap::coercion(key);

    match value {
        serde_json::Value::String(s) => match coercion {
            Some(TermCoercion::DateTime) => {
                let mut node = serde_json::Map::with_capacity(2);
                node.insert("@type".to_owned(), serde_json::Value::String(vocab::expanded::DATE_TIME.as_str().to_owned()));
                node.insert("@value".to_owned(), serde_json::Value::String(s.clone()));
                Some((iri, serde_json::Value::Array(vec![serde_json::Value::Object(node)])))
            }
            Some(TermCoercion::Id) => {
                let mut node = serde_json::Map::with_capacity(1);
                node.insert("@id".to_owned(), serde_json::Value::String(s.clone()));
                Some((iri, serde_json::Value::Array(vec![serde_json::Value::Object(node)])))
            }
            None => {
                let mut node = serde_json::Map::with_capacity(1);
                node.insert("@value".to_owned(), serde_json::Value::String(s.clone()));
                Some((iri, serde_json::Value::Array(vec![serde_json::Value::Object(node)])))
            }
        },
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) if coercion.is_none() => {
            let mut node = serde_json::Map::with_capacity(1);
            node.insert("@value".to_owned(), value.clone());
            Some((iri, serde_json::Value::Array(vec![serde_json::Value::Object(node)])))
        }
        serde_json::Value::Object(sub) => {
            // Nested sub-attribute: re-use the full attribute-instance
            // expander so Property / Relationship / GeoProperty / … nested
            // under another attribute expand correctly.
            let has_attr_type = sub.get("type").and_then(|v| v.as_str()).and_then(AttrKind::from_type_name).is_some();
            if !has_attr_type {
                return None;
            }
            let expanded = fast_expand_attribute_instance(sub, term_map)?;
            Some((iri, serde_json::Value::Array(vec![expanded])))
        }
        serde_json::Value::Array(items) => {
            // Nested sub-attribute with multiple datasets (array of
            // attribute instances).
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let sub = item.as_object()?;
                let has_attr_type = sub.get("type").and_then(|v| v.as_str()).and_then(AttrKind::from_type_name).is_some();
                if !has_attr_type {
                    return None;
                }
                out.push(fast_expand_attribute_instance(sub, term_map)?);
            }
            Some((iri, serde_json::Value::Array(out)))
        }
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) | serde_json::Value::Null => None,
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

    async fn expand_both_paths(engine: &JsonLdEngine, document: serde_json::Value, handle: &BatchContextHandle) -> (serde_json::Value, serde_json::Value) {
        let fast = engine.expand_with_handle(document.clone(), handle).await.expect("fastpath expand");
        let slow = engine.expand(document, &handle.spec).await.expect("slow expand");
        (fast, slow)
    }

    fn core_handle(engine: &JsonLdEngine) -> BatchContextHandle {
        let spec = ContextSpec::core_only();
        engine.batch_context_handle(&spec)
    }

    #[tokio::test]
    async fn fastpath_simple_property_byte_equal_to_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        assert!(handle.core_term_map.is_some(), "core-only spec must enable fastpath");

        let doc = json!({
            "id": "urn:ngsi-ld:Device:1",
            "type": "Device",
            "temperature": {"type": "Property", "value": 21}
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_property_with_observed_at_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:2",
            "type": "Device",
            "temperature": {
                "type": "Property",
                "value": 21.5,
                "observedAt": "2024-01-01T00:00:00Z",
                "unitCode": "CEL"
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_property_with_dataset_id_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:3",
            "type": "Device",
            "temperature": {
                "type": "Property",
                "value": 21,
                "datasetId": "urn:ngsi-ld:Dataset:sensor-a"
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_multi_instance_property_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:4",
            "type": "Device",
            "temperature": [
                {"type": "Property", "value": 21, "datasetId": "urn:ngsi-ld:Dataset:a"},
                {"type": "Property", "value": 22, "datasetId": "urn:ngsi-ld:Dataset:b"}
            ]
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_relationship_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:5",
            "type": "Device",
            "belongsTo": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Site:1",
                "observedAt": "2024-01-01T00:00:00Z"
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_multiple_types_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:6",
            "type": ["Device", "Sensor"],
            "temperature": {"type": "Property", "value": 21}
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_geoproperty_falls_back_to_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:7",
            "type": "Device",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.4, 52.5]}
            }
        });
        // Fastpath must return Miss for GeoProperty; expand_with_handle then
        // uses the full pipeline. Parity with engine.expand is enforced by
        // both going through the full expander on a bailout.
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_inline_context_divergence_falls_back_to_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        // Inline @context object overrides the batch handle; fastpath must
        // refuse and the fallback path must still produce the correct
        // expansion using the document's own context.
        let doc = json!({
            "@context": {"@vocab": "https://example.com/vocab#"},
            "id": "urn:ngsi-ld:Device:8",
            "type": "Thing"
        });
        let fast = engine.expand_with_handle(doc.clone(), &handle).await.expect("expand");
        let slow = engine.expand(doc, &handle.spec).await.expect("expand");
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_geoproperty_point_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:g1",
            "type": "Device",
            "location": {
                "type": "GeoProperty",
                "value": {"type": "Point", "coordinates": [13.4, 52.5]}
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_geoproperty_polygon_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:g2",
            "type": "Device",
            "footprint": {
                "type": "GeoProperty",
                "value": {"type": "Polygon", "coordinates": [[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]]}
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_language_property_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:lp1",
            "type": "Device",
            "description": {
                "type": "LanguageProperty",
                "languageMap": {"en": "hello", "de": "hallo"}
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_vocab_property_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:vp1",
            "type": "Device",
            "category": {
                "type": "VocabProperty",
                "vocab": "sensor"
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_json_property_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:jp1",
            "type": "Device",
            "config": {
                "type": "JsonProperty",
                "json": {"k1": 1, "k2": [true, false]}
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_scope_string_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:sc1",
            "type": "Device",
            "scope": "root/a"
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_scope_array_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:sc2",
            "type": "Device",
            "scope": ["root/a", "root/b"]
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    fn fast_expand_or_none(engine: &JsonLdEngine, document: serde_json::Value) -> Option<serde_json::Value> {
        let handle = core_handle(engine);
        let term_map = handle.core_term_map.as_ref().expect("fastpath enabled");
        fast_expand_entity_document(&document, term_map)
    }

    #[test]
    fn fastpath_hits_simple_property() {
        let engine = test_engine();
        let doc = json!({"id": "urn:x:1", "type": "X", "a": {"type": "Property", "value": 1}});
        assert!(fast_expand_or_none(&engine, doc).is_some());
    }

    #[test]
    fn fastpath_hits_geoproperty() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:2", "type": "X",
            "loc": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        });
        assert!(fast_expand_or_none(&engine, doc).is_some());
    }

    #[test]
    fn fastpath_hits_language_property() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:3", "type": "X",
            "desc": {"type": "LanguageProperty", "languageMap": {"en": "hi"}}
        });
        assert!(fast_expand_or_none(&engine, doc).is_some());
    }

    #[test]
    fn fastpath_hits_vocab_property() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:4", "type": "X",
            "cat": {"type": "VocabProperty", "vocab": "sensor"}
        });
        assert!(fast_expand_or_none(&engine, doc).is_some());
    }

    #[test]
    fn fastpath_hits_json_property() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:5", "type": "X",
            "cfg": {"type": "JsonProperty", "json": {"k": 1}}
        });
        assert!(fast_expand_or_none(&engine, doc).is_some());
    }

    #[test]
    fn fastpath_misses_on_inline_divergent_context() {
        let engine = test_engine();
        let doc = json!({
            "@context": {"@vocab": "https://example.com/"},
            "id": "urn:x:6", "type": "X"
        });
        assert!(fast_expand_or_none(&engine, doc).is_none());
    }

    #[test]
    fn fastpath_misses_on_unknown_attribute_type() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:7", "type": "X",
            "weird": {"type": "SomethingElse", "value": 1}
        });
        assert!(fast_expand_or_none(&engine, doc).is_none());
    }

    #[tokio::test]
    async fn fastpath_property_with_nested_sub_property_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:nested1",
            "type": "Device",
            "temperature": {
                "type": "Property",
                "value": 21,
                "accuracy": {"type": "Property", "value": 0.5}
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fastpath_relationship_with_nested_sub_relationship_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine);
        let doc = json!({
            "id": "urn:ngsi-ld:Device:nested2",
            "type": "Device",
            "belongsTo": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Site:1",
                "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Org:1"}
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[test]
    fn fastpath_misses_on_property_with_object_value() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:8", "type": "X",
            "complex": {"type": "Property", "value": {"nested": true}}
        });
        assert!(fast_expand_or_none(&engine, doc).is_none());
    }

    #[tokio::test]
    async fn fastpath_enabled_for_default_context_spec() {
        let engine = test_engine();
        let spec = ContextSpec::none();
        let handle = engine.batch_context_handle(&spec);
        assert!(handle.core_term_map.is_some(), "default spec (core + andromeda only) must enable the fastpath");
    }

    #[tokio::test]
    async fn fastpath_enabled_for_link_header_containing_only_core_urls() {
        let engine = test_engine();
        let spec = ContextSpec::from_links(vec![NGSI_LD_CORE_CONTEXT_URL.to_string(), ANDROMEDA_CORE_CONTEXT_URL.to_string()]);
        let handle = engine.batch_context_handle(&spec);
        assert!(handle.core_term_map.is_some(), "core-only Link header must enable the fastpath");
    }

    #[tokio::test]
    async fn fastpath_disabled_for_body_context_spec() {
        let engine = test_engine();
        let spec = ContextSpec::none().with_body_context(Some(json!({"@vocab": "https://example.com/vocab#"})));
        let handle = engine.batch_context_handle(&spec);
        assert!(handle.core_term_map.is_none(), "user body context must disable the fastpath");
    }

    #[tokio::test]
    async fn fastpath_disabled_for_custom_link_context_spec() {
        let engine = test_engine();
        let spec = ContextSpec::from_links(vec!["https://example.com/custom.jsonld".to_string()]);
        let handle = engine.batch_context_handle(&spec);
        assert!(handle.core_term_map.is_none(), "user link context must disable the fastpath");
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
