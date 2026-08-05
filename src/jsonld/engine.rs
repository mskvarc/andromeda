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
        NGSI_LD_CORE_CONTEXT_MAP,
        NGSI_LD_CORE_CONTEXT_URL,
        NGSI_LD_CORE_CONTEXT_URL_LIST,
        is_ngsi_ld_core_context_url,
    },
    loader::{ContextCache, ContextRegistry, HitCounter, HttpContextLoader},
};
use andromeda_ngsi_ld_vocabulary::{COMPACT_TO_EXPANDED, compact_to_expanded, expanded::classes as expanded_classes};
use contextual::WithContext;
use dashmap::DashMap;
use jsonld::{
    JsonLdProcessor,
    RemoteContextReference,
    RemoteDocument,
    syntax::{Context as JsonLdContext, IntoJson, TryFromJson},
};
use jsonld_syntax::Parse;
use moka::future::Cache;
use rdfx::vocabulary::{BlankIdIndex, IndexVocabulary, IriIndex};
use std::{
    borrow::Cow,
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

/// Pre-allocated `String`s for the JSON-LD expanded-form IRI keys the fastpath
/// emits on every entity. Calling `.clone()` on these reuses the cached
/// `String` shape (capacity == length == known IRI length) instead of running
/// `.as_str().to_owned()` on a `&'static iref::Iri` which dereferences the IRI
/// pointer and recomputes the UTF-8 length on every entity.
mod static_keys {
    use andromeda_ngsi_ld_vocabulary::expanded::{classes as expanded_classes, properties as expanded_properties};
    use std::sync::LazyLock;

    pub(super) static PROPERTY: LazyLock<String> = LazyLock::new(|| expanded_classes::PROPERTY.as_str().to_owned());
    pub(super) static RELATIONSHIP: LazyLock<String> = LazyLock::new(|| expanded_classes::RELATIONSHIP.as_str().to_owned());
    pub(super) static GEO_PROPERTY: LazyLock<String> = LazyLock::new(|| expanded_classes::GEO_PROPERTY.as_str().to_owned());
    pub(super) static LANGUAGE_PROPERTY: LazyLock<String> = LazyLock::new(|| expanded_classes::LANGUAGE_PROPERTY.as_str().to_owned());
    pub(super) static VOCAB_PROPERTY: LazyLock<String> = LazyLock::new(|| expanded_classes::VOCAB_PROPERTY.as_str().to_owned());
    pub(super) static JSON_PROPERTY: LazyLock<String> = LazyLock::new(|| expanded_classes::JSON_PROPERTY.as_str().to_owned());
    pub(super) static VALUE: LazyLock<String> = LazyLock::new(|| expanded_properties::VALUE.as_str().to_owned());
    pub(super) static OBJECT: LazyLock<String> = LazyLock::new(|| expanded_properties::OBJECT.as_str().to_owned());
    pub(super) static LANGUAGE_MAP: LazyLock<String> = LazyLock::new(|| expanded_properties::LANGUAGE_MAP.as_str().to_owned());
    pub(super) static VOCAB: LazyLock<String> = LazyLock::new(|| expanded_properties::VOCAB.as_str().to_owned());
    pub(super) static JSON: LazyLock<String> = LazyLock::new(|| expanded_properties::JSON.as_str().to_owned());
    pub(super) static SCOPE: LazyLock<String> = LazyLock::new(|| expanded_properties::SCOPE.as_str().to_owned());
    pub(super) static DATE_TIME: LazyLock<String> = LazyLock::new(|| expanded_classes::DATE_TIME.as_str().to_owned());
    pub(super) static COORDINATES: LazyLock<String> = LazyLock::new(|| expanded_properties::COORDINATES.as_str().to_owned());
    pub(super) static BBOX: LazyLock<String> = LazyLock::new(|| expanded_properties::BBOX.as_str().to_owned());
}

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
    hit_counter: Option<Arc<HitCounter>>,
    /// Per-URL parsed user term tables. Built lazily the first time a
    /// batch references a non-core context URL; reused across batches for
    /// the lifetime of the engine.
    user_term_table_cache: Arc<DashMap<String, Arc<UserTermTable>>>,
}

impl JsonLdEngine {
    /// Returns the in-memory hit counter, if one is attached.
    ///
    /// Wired by the broker startup path so a periodic flush task can drain
    /// pending deltas into the L2 store. Absent when the engine was built
    /// without an L2 cache.
    pub fn hit_counter(&self) -> Option<Arc<HitCounter>> {
        self.hit_counter.clone() // clone: Arc ref-count bump
    }
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
/// `serde_json → jsonld_syntax → jsonld expand → serde_json` round-trip.
///
/// Entities whose inline `@context` diverges from the batch's resolved value
/// fall back to the full per-entity [`JsonLdEngine::expand`] path, so the
/// spec rule allowing per-entity context overrides (NGSI-LD 5.5.7) is
/// preserved byte-for-byte.
#[derive(Clone, Debug)]
pub struct BatchContextHandle {
    spec: ContextSpec,
    resolved_context: Option<Arc<serde_json::Value>>,
    term_map: Arc<BatchTermMap>,
    relative_url_policy: RelativeUrlPolicy,
}

impl BatchContextHandle {
    /// Whether the handle's term map admits the in-process fastpath. When
    /// `false`, every entity in the batch routes to the asynchronous slow
    /// path regardless of per-entity `@context` shape.
    pub fn fast_eligible(&self) -> bool {
        self.term_map.is_eligible()
    }

    /// Back-compat alias for [`Self::fast_eligible`]. Both names are
    /// supported because Step 4's batch service still uses the original
    /// `has_core_term_map` accessor for handle-level routing.
    pub fn has_core_term_map(&self) -> bool {
        self.fast_eligible()
    }

    /// Borrow the [`ContextSpec`] this handle was built from. Required by
    /// the batch service's slow pool to dispatch per-entity expansion when
    /// an inline `@context` diverges from the handle.
    pub const fn spec(&self) -> &ContextSpec {
        &self.spec
    }
}

/// Whether the resolved batch `@context` may contain relative URLs that
/// `expand_prepared` must rewrite via [`resolve_relative_context_urls`].
///
/// Computed once at handle build time so per-entity expansion can skip the
/// recursive walk when the batch context is provably composed of absolute
/// IRIs only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelativeUrlPolicy {
    /// All `@context` strings are absolute IRIs — skip the rewriter.
    Absent,
    /// At least one string is non-absolute or unknown — run the rewriter.
    MaybePresent,
}

/// NGSI-LD default-context prefix (`@vocab` in the core context).
const NGSI_LD_DEFAULT_CONTEXT_PREFIX: &str = "https://uri.etsi.org/ngsi-ld/default-context/";
/// Standard XSD namespace, used as a built-in compile-time prefix for `xsd:`.
const XSD_PREFIX: &str = "http://www.w3.org/2001/XMLSchema#";

/// Generalised term resolution table covering the NGSI-LD core context plus
/// any user-supplied link/body contexts.
///
/// Built once per batch from [`JsonLdEngine::batch_context_handle`]; consumed
/// by the synchronous fastpath in [`fast_normalize_and_expand_entity`] and
/// friends. Carries enough information (active `@vocab`, prefix table sorted
/// longest-first per JSON-LD 1.1 §4.4, term lookups with optional `@type`
/// coercion, scoped-context flags, and an explicit eligibility verdict) so
/// the fastpath can both (a) decide at handle level whether to even attempt
/// fast expansion, and (b) per-entity reject documents whose attribute keys
/// resolve to scoped sub-contexts the fastpath does not support.
#[derive(Debug, Clone)]
pub struct BatchTermMap {
    /// Term name → resolved IRI plus coercion + scoped-context flag.
    terms: HashMap<Box<str>, ResolvedTerm>,
    /// Active `@vocab` IRI prefix; defaults to the NGSI-LD core
    /// default-context, overridden by any user-supplied `@vocab`.
    vocab: Box<str>,
    /// Prefix definitions (`prefix → expansion`) sorted longest-first so
    /// `exFoo:bar` matches `exFoo:` before `ex:` per JSON-LD §4.4.
    prefix_table: Vec<(Box<str>, Box<str>)>,
    /// Verdict on whether the fastpath may attempt to expand entities
    /// against this term map.
    eligibility: FastpathEligibility,
}

/// Whether the fastpath is allowed to attempt expansion at the handle
/// level. A non-`Eligible` value means every entity in the batch is
/// routed to the slow path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FastpathEligibility {
    /// Fastpath may attempt expansion. Per-entity rules in
    /// [`fast_normalize_and_expand_entity`] still apply.
    Eligible,
    /// Fastpath unsafe at the handle level — slow path only.
    Ineligible(IneligibilityReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IneligibilityReason {
    /// A user context redefined a core term with a different IRI.
    UserOverridesCoreTerm,
    /// At least one term in a user context carries a scoped `@context`,
    /// which the fastpath does not unfold.
    HasScopedContexts,
    /// User context referenced a blank node identifier.
    #[allow(dead_code)]
    ContainsBlankNodeContext,
    /// User context URL could not be resolved through the loader pipeline.
    ContextLoadFailed,
}

/// One term mapping inside [`BatchTermMap::terms`].
#[derive(Debug, Clone)]
struct ResolvedTerm {
    iri: Box<str>,
    coercion: Option<TermCoercion>,
    has_scoped_context: bool,
}

/// Parsed user-context shape cached per URL on the engine. Only IRI
/// expansions, `@vocab`, and scoped-context flags are retained — anything
/// the fastpath cannot honour conservatively forces ineligibility.
#[derive(Debug, Clone)]
struct UserTermTable {
    terms: HashMap<Box<str>, ResolvedTerm>,
    vocab: Option<Box<str>>,
    has_scoped_contexts: bool,
}

impl BatchTermMap {
    /// `true` iff the handle's eligibility verdict is `Eligible`.
    pub const fn is_eligible(&self) -> bool {
        matches!(self.eligibility, FastpathEligibility::Eligible)
    }

    /// Expand a short term to its full IRI using (in priority order):
    /// the compile-time core lookup, the user-context term map, the
    /// user-defined prefix table sorted longest-first, the built-in `xsd:`
    /// prefix, or the active `@vocab` fallback.
    fn expand_iri<'a>(&'a self, short: &'a str) -> Cow<'a, str> {
        if is_full_iri(short) {
            return Cow::Borrowed(short);
        }
        if let Some(term) = self.terms.get(short) {
            return Cow::Borrowed(&term.iri);
        }
        if let Some((prefix, suffix)) = short.split_once(':') {
            for (p, expansion) in &self.prefix_table {
                if prefix == p.as_ref() {
                    return Cow::Owned(format!("{expansion}{suffix}"));
                }
            }
            if prefix == "xsd" {
                return Cow::Owned(format!("{XSD_PREFIX}{suffix}"));
            }
            if prefix == "ngsi-ld" {
                return Cow::Owned(format!("https://uri.etsi.org/ngsi-ld/{suffix}"));
            }
            if prefix == "geojson" {
                return Cow::Owned(format!("https://purl.org/geojson/vocab#{suffix}"));
            }
        }
        Cow::Owned(format!("{}{}", self.vocab, short))
    }

    /// Coercion declared for a term, if any. Looks up the term name in the
    /// merged `terms` map (which combines core + user contexts).
    fn coercion(&self, short: &str) -> Option<TermCoercion> {
        self.terms.get(short).and_then(|t| t.coercion.clone())
    }

    /// Whether the supplied term name is declared with a scoped `@context`
    /// in any of the contributing contexts.
    fn term_has_scoped_context(&self, short: &str) -> bool {
        self.terms.get(short).is_some_and(|t| t.has_scoped_context)
    }
}

/// `@type` coercion applied to attribute metadata values.
///
/// Distinguishes the NGSI-LD core context's hard-coded `DateTime`/`@id`
/// shapes from user-supplied custom IRIs declared on a term in a body or
/// link context. The custom IRI is stored as the raw user string and
/// resolved through [`BatchTermMap::expand_iri`] at emit time so user
/// prefix declarations apply consistently with the slow path.
#[derive(Debug, Clone)]
enum TermCoercion {
    /// Value is emitted as `{"@type": "<NGSI-LD DateTime IRI>", "@value": "<ISO>"}`.
    DateTime,
    /// Value is emitted as `{"@id": "<value>"}`.
    Id,
    /// Value is emitted as `{"@type": "<expanded user IRI>", "@value": "<s>"}`.
    TypeIri(Box<str>),
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
        self.expand_prepared(document, context.core_policy(), RelativeUrlPolicy::MaybePresent).await
    }

    /// Build a per-batch expansion handle for a shared `@context`.
    ///
    /// Asynchronous because user-supplied link contexts are resolved
    /// through [`HttpContextLoader::load_value`], which may need the L1
    /// moka cache, the registry, the L2 cache, or a remote fetch. The
    /// resulting [`BatchTermMap`] caches the parsed shape so subsequent
    /// handles for the same URL pay only a `DashMap` lookup.
    pub async fn batch_context_handle(&self, context: &ContextSpec) -> BatchContextHandle {
        let resolved_context = context::resolve_context_value(None, context);
        let relative_url_policy = match resolved_context.as_ref() {
            Some(ctx) => detect_relative_url_policy(ctx),
            None => RelativeUrlPolicy::Absent,
        };
        let term_map = self.build_batch_term_map(context).await;
        BatchContextHandle {
            resolved_context: resolved_context.map(Arc::new),
            term_map: Arc::new(term_map),
            spec: context.clone(),
            relative_url_policy,
        }
    }

    /// Resolve a user-context URL to its parsed [`UserTermTable`], caching
    /// the result on the engine for the process lifetime. The first call
    /// per URL fetches through the loader (registry → L2 → remote);
    /// subsequent calls hit the in-memory `DashMap`.
    async fn user_term_table(&self, url: &str) -> Result<Arc<UserTermTable>, Error> {
        if let Some(cached) = self.user_term_table_cache.get(url) {
            return Ok(Arc::clone(cached.value()));
        }
        let value = self.loader.load_value(url).await?;
        let serde_value = jsonld_syntax::Value::into_serde_json((*value).clone()); // clone: Arc -> owned for conversion
        let table = parse_user_context_value(&serde_value);
        let arc = Arc::new(table);
        self.user_term_table_cache.insert(url.to_owned(), Arc::clone(&arc));
        Ok(arc)
    }

    async fn build_batch_term_map(&self, spec: &ContextSpec) -> BatchTermMap {
        // Seed terms from the compile-time NGSI-LD core lookup.
        let mut terms: HashMap<Box<str>, ResolvedTerm> = HashMap::new();
        for (compact, expanded) in COMPACT_TO_EXPANDED {
            terms.insert(
                Box::<str>::from(*compact),
                ResolvedTerm {
                    iri: Box::<str>::from(*expanded),
                    coercion: core_term_coercion(compact),
                    has_scoped_context: false,
                },
            );
        }

        let mut vocab = Box::<str>::from(NGSI_LD_DEFAULT_CONTEXT_PREFIX);
        let mut prefix_table: Vec<(Box<str>, Box<str>)> = Vec::new();
        let mut eligibility = FastpathEligibility::Eligible;

        match spec.core_policy() {
            CoreContextPolicy::Only | CoreContextPolicy::Append | CoreContextPolicy::AppendIfNoExplicit => {}
            CoreContextPolicy::Omit => {
                eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextLoadFailed);
            }
        }

        // User link contexts (skip core URLs which were seeded above).
        for url in spec.link_contexts() {
            if is_ngsi_ld_core_context_url(url) || url == ANDROMEDA_CORE_CONTEXT_URL {
                continue;
            }
            match self.user_term_table(url).await {
                Ok(table) => {
                    merge_user_table(&mut terms, &mut vocab, &mut prefix_table, &table, &mut eligibility);
                }
                Err(_err) => {
                    eligibility = FastpathEligibility::Ineligible(IneligibilityReason::ContextLoadFailed);
                }
            }
        }

        // Inline body context.
        if let Some(body) = spec.body_context() {
            let table = parse_user_context_value(body);
            merge_user_table(&mut terms, &mut vocab, &mut prefix_table, &table, &mut eligibility);
        }

        // Sort prefix_table by length descending so longest match wins
        // (JSON-LD 1.1 §4.4 IRI compaction prefix order).
        prefix_table.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));

        BatchTermMap {
            terms,
            vocab,
            prefix_table,
            eligibility,
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
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn expand_with_handle(&self, mut document: serde_json::Value, handle: &BatchContextHandle) -> Result<serde_json::Value, Error> {
        if handle.term_map.is_eligible()
            && let Some(expanded) = fast_normalize_and_expand_entity(&document, &handle.term_map)
        {
            return Ok(expanded);
        }
        match handle_fast_path_apply(&mut document, handle) {
            FastPath::Hit => self.expand_prepared(document, handle.spec.core_policy(), handle.relative_url_policy).await,
            FastPath::Miss => self.expand(document, &handle.spec).await,
        }
    }

    /// Expand many independently-prepared documents in a single
    /// `json-ld` call.
    ///
    /// All inputs must already be normalized and carry their effective
    /// `@context`. The bulk call wraps them in a single `Value::Array` so
    /// the expander runs once with one [`IndexVocabulary`] and one loader
    /// handshake; the result is split back into per-entity values aligned
    /// with the input order.
    ///
    /// On whole-batch expansion failure (e.g. one malformed entity poisons
    /// the bulk call) the engine falls back to per-entity expansion to
    /// attribute the error to the offending input. Worst case is 2x
    /// latency on malformed inputs; the happy path stays a single call.
    #[cfg_attr(not(test), allow(dead_code))]
    pub async fn expand_prepared_array(&self, documents: Vec<serde_json::Value>, core_policy: CoreContextPolicy) -> Vec<Result<serde_json::Value, Error>> {
        if documents.is_empty() {
            return Vec::new();
        }
        let len = documents.len();

        // Cheap-clone path: only deep-clone if we need to fall back.
        let bulk_doc = serde_json::Value::Array(documents.clone()); // clone: fallback retry needs originals
        match self.expand_prepared(bulk_doc, core_policy, RelativeUrlPolicy::MaybePresent).await {
            Ok(serde_json::Value::Array(items)) if items.len() == len => {
                // Re-wrap each node object as a single-element array so the
                // per-entity shape matches `expand_prepared`'s contract.
                items.into_iter().map(|item| Ok(serde_json::Value::Array(vec![item]))).collect()
            }
            Ok(_other) => {
                // Bulk shape did not match input cardinality (graph
                // expansion or unexpected reshape). Fall back to per-entity.
                tracing::debug!("Bulk expansion shape mismatch for {} inputs; retrying per-entity", len);
                self.expand_per_entity(documents, core_policy).await
            }
            Err(bulk_err) => {
                tracing::warn!(error = %bulk_err, "Bulk JSON-LD expansion failed; retrying per-entity to attribute error");
                self.expand_per_entity(documents, core_policy).await
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    async fn expand_per_entity(&self, documents: Vec<serde_json::Value>, core_policy: CoreContextPolicy) -> Vec<Result<serde_json::Value, Error>> {
        let mut results = Vec::with_capacity(documents.len());
        for doc in documents {
            results.push(self.expand_prepared(doc, core_policy, RelativeUrlPolicy::MaybePresent).await);
        }
        results
    }

    /// Synchronous CPU-only fastpath entry point used by
    /// [`crate::service::entity::EntityService`]'s batch fast pool.
    ///
    /// Returns the expanded form when the handle carries a core term map and
    /// the document fits the supported subset; returns `None` so the caller
    /// can demote the entity to the asynchronous slow path. Does no I/O and
    /// is safe to call from `tokio::task::spawn_blocking`.
    pub fn try_fast_expand(&self, document: &serde_json::Value, handle: &BatchContextHandle) -> Option<serde_json::Value> {
        if !handle.term_map.is_eligible() {
            return None;
        }
        fast_normalize_and_expand_entity(document, &handle.term_map)
    }

    /// Per-entity check whether the entity's inline `@context` (if any)
    /// matches the batch handle's resolved context. Used by the batch
    /// classifier to route entities between the synchronous fastpath pool
    /// and the asynchronous slow pool without touching the document DOM.
    pub fn entity_context_matches_handle(&self, document: &serde_json::Value, handle: &BatchContextHandle) -> bool {
        check_handle_context_compatibility(document, handle)
    }

    async fn expand_prepared(
        &self,
        mut document: serde_json::Value,
        core_policy: CoreContextPolicy,
        relative_url_policy: RelativeUrlPolicy,
    ) -> Result<serde_json::Value, Error> {
        if relative_url_policy == RelativeUrlPolicy::MaybePresent {
            resolve_relative_context_urls(&mut document);
        }

        // Diagnostic-only fields (applied_context, context_urls, normalized_context_urls)
        // are expensive recursive traversals with repeated clones. Keep the happy
        // path zero-alloc for diagnostics; only clone the document's `@context`
        // for the error branch, since `from_serde_json` consumes the full doc.
        let diag_context = document.get("@context").cloned();
        let syntax_value = jsonld_syntax::Value::from_serde_json(document);
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

        let expanded_json = jsonld_syntax::Value::into_serde_json(expanded.into_iter().collect::<Vec<_>>().into_with(&vocabulary).into_json());
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
        let syntax_value = jsonld_syntax::Value::from_serde_json(document.clone());
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
                    let fallback_remote_doc: RemoteDocument<IriIndex> = RemoteDocument::new(None, None, jsonld_syntax::Value::from_serde_json(document));
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

        let mut compacted_json = jsonld_syntax::Value::into_serde_json(compacted);

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
        Ok(jsonld_syntax::Value::into_serde_json(Arc::unwrap_or_clone(value)))
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
        Ok(jsonld_syntax::Value::into_serde_json(Arc::unwrap_or_clone(value)))
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

        // The ETSI v1.9 core context references the W3C Data Integrity
        // companion context as a scoped @context for the `ngsildproof`
        // term. The broker no longer implements proof verification, but
        // the term itself remains in the verbatim ETSI artefact, so its
        // scoped context must still resolve during expansion. Register
        // an empty stub so JSON-LD processing does not fail.
        const DATA_INTEGRITY_CONTEXT_URL: &str = "https://w3id.org/security/data-integrity/v2";
        const DATA_INTEGRITY_STUB: &str = r#"{"@context":{}}"#;
        let data_integrity_stub = load_embedded_context(DATA_INTEGRITY_STUB, DATA_INTEGRITY_CONTEXT_URL)?;
        self.registry.register(DATA_INTEGRITY_CONTEXT_URL.to_string(), data_integrity_stub);

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
        let hit_counter = if self.l2_cache.is_some() {
            let counter = Arc::new(HitCounter::new());
            loader = loader.with_hit_counter(Arc::clone(&counter)); // clone: Arc ref-count bump
            Some(counter)
        } else {
            None
        };
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
            hit_counter,
            user_term_table_cache: Arc::new(DashMap::new()),
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

fn load_embedded_core_context(url: &str) -> Result<jsonld_syntax::Value, Error> {
    let context = NGSI_LD_CORE_CONTEXT_MAP
        .get(url)
        .ok_or(Error::EmbeddedContextInvalid("Unable to obtain embedded context".to_string()))?;

    let (value, _) = jsonld_syntax::Value::parse_str(context).map_err(|e| Error::EmbeddedContextInvalid(e.to_string()))?;
    Ok(value)
}

/// Parse an embedded context payload into a [`jsonld_syntax::Value`].
fn load_embedded_context(payload: &str, url: &str) -> Result<jsonld_syntax::Value, Error> {
    let (value, _) = jsonld_syntax::Value::parse_str(payload).map_err(|e| Error::EmbeddedContextInvalid(format!("{}: {}", url, e)))?;
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
        let (value, _) = jsonld_syntax::Value::parse_str(&raw).map_err(|e| Error::ContextInvalid {
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
            let syntax_value = jsonld_syntax::Value::from_serde_json(value);
            JsonLdContext::try_from_json(&syntax_value).map_err(|e| Error::ContextInvalid {
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

/// Non-mutating equivalent of [`handle_fast_path_apply`] used during batch
/// classification. Returns `true` when the document's inline `@context`
/// (if any) matches the handle's resolved context, so attaching the handle
/// to the document for slow-path expansion would still be a fastpath hit.
fn check_handle_context_compatibility(document: &serde_json::Value, handle: &BatchContextHandle) -> bool {
    fn check(obj: &serde_json::Map<String, serde_json::Value>, handle: &BatchContextHandle) -> bool {
        match obj.get("@context") {
            None => true,
            Some(existing) => Some(existing) == handle.resolved_context.as_deref(),
        }
    }
    match document {
        serde_json::Value::Object(obj) => check(obj, handle),
        serde_json::Value::Array(items) => items.iter().all(|item| match item {
            serde_json::Value::Object(obj) => check(obj, handle),
            serde_json::Value::Null
            | serde_json::Value::Bool(_)
            | serde_json::Value::Number(_)
            | serde_json::Value::String(_)
            | serde_json::Value::Array(_) => false,
        }),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => false,
    }
}

fn apply_handle_to_object(obj: &mut serde_json::Map<String, serde_json::Value>, handle: &BatchContextHandle) -> FastPath {
    match obj.get("@context") {
        None => {
            if let Some(resolved) = handle.resolved_context.as_ref() {
                // Cold path: entity has no `@context`; deep-clone the shared
                // resolved value once into the per-entity document.
                obj.insert("@context".to_string(), (**resolved).clone());
            }
            FastPath::Hit
        }
        Some(existing) if Some(existing) == handle.resolved_context.as_deref() => FastPath::Hit,
        Some(_) => FastPath::Miss,
    }
}

/// Build the precomputed core term map if the spec's effective `@context`
/// resolves to exactly the NGSI-LD core + Andromeda core bundle with no
/// user-supplied contexts. Any user-supplied link or body context disables
/// the fastpath because term IRIs might be overridden.
/// Hardcoded `@type` coercions defined by the NGSI-LD core context.
/// Mirrors the embedded core context's `@type` declarations (DateTime,
/// `@id`) for the term names listed in NGSI-LD 1.9.1 §B.
fn core_term_coercion(short: &str) -> Option<TermCoercion> {
    match short {
        "observedAt" | "createdAt" | "modifiedAt" | "expiresAt" | "deletedAt" | "startAt" | "endAt" | "timeAt" | "instanceTimeAt" | "previousTime"
        | "timestamp" | "notifiedAt" | "lastNotification" | "lastSuccess" | "lastFailure" => Some(TermCoercion::DateTime),
        "datasetId" | "instanceId" => Some(TermCoercion::Id),
        _other => None,
    }
}

/// Parse a user-supplied `@context` value (string URL refs are filtered out
/// by the caller; this only consumes inline objects/arrays/wrapping).
fn parse_user_context_value(value: &serde_json::Value) -> UserTermTable {
    let mut table = UserTermTable {
        terms: HashMap::new(),
        vocab: None,
        has_scoped_contexts: false,
    };
    parse_user_context_into(value, &mut table);
    table
}

fn parse_user_context_into(value: &serde_json::Value, table: &mut UserTermTable) {
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                parse_user_context_into(item, table);
            }
        }
        serde_json::Value::Object(map) => {
            // The outer JSON-LD context document wraps the actual context in
            // a "@context" key. Recurse into that if present.
            if let Some(inner) = map.get("@context") {
                parse_user_context_into(inner, table);
                return;
            }
            for (key, val) in map {
                if key == "@vocab" {
                    if let Some(s) = val.as_str() {
                        table.vocab = Some(Box::<str>::from(s));
                    }
                } else if key.starts_with('@') {
                    // Other JSON-LD keywords ignored conservatively.
                } else {
                    match val {
                        serde_json::Value::String(iri) => {
                            table.terms.insert(
                                Box::<str>::from(key.as_str()),
                                ResolvedTerm {
                                    iri: Box::<str>::from(iri.as_str()),
                                    coercion: None,
                                    has_scoped_context: false,
                                },
                            );
                        }
                        serde_json::Value::Object(obj) => {
                            let id = obj.get("@id").and_then(|v| v.as_str());
                            let type_str = obj.get("@type").and_then(|v| v.as_str());
                            let has_scoped = obj.contains_key("@context");
                            if has_scoped {
                                table.has_scoped_contexts = true;
                            }
                            if let Some(id_iri) = id {
                                let coercion = match type_str {
                                    Some("@id") => Some(TermCoercion::Id),
                                    Some(t) if t == expanded_classes::DATE_TIME.as_str() => Some(TermCoercion::DateTime),
                                    Some("DateTime") => Some(TermCoercion::DateTime),
                                    Some(other) => Some(TermCoercion::TypeIri(Box::<str>::from(other))),
                                    None => None,
                                };
                                table.terms.insert(
                                    Box::<str>::from(key.as_str()),
                                    ResolvedTerm {
                                        iri: Box::<str>::from(id_iri),
                                        coercion,
                                        has_scoped_context: has_scoped,
                                    },
                                );
                            }
                        }
                        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Array(_) => {
                            // Other shapes (null, arrays in term values) are not
                            // part of the standard JSON-LD context grammar; the
                            // fastpath is conservative and forwards parsing
                            // responsibility to the slow path.
                        }
                    }
                }
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => {}
    }
}

/// Merge a parsed user term table into the accumulating batch term map.
/// Sets `eligibility` to an `Ineligible` variant if the merge would
/// require canonical-equivalence checks the fastpath does not perform.
fn merge_user_table(
    terms: &mut HashMap<Box<str>, ResolvedTerm>,
    vocab: &mut Box<str>,
    prefix_table: &mut Vec<(Box<str>, Box<str>)>,
    table: &UserTermTable,
    eligibility: &mut FastpathEligibility,
) {
    if matches!(*eligibility, FastpathEligibility::Ineligible(_)) {
        return;
    }
    if table.has_scoped_contexts {
        *eligibility = FastpathEligibility::Ineligible(IneligibilityReason::HasScopedContexts);
        return;
    }
    if let Some(new_vocab) = &table.vocab {
        *vocab = new_vocab.clone();
    }
    for (term_name, resolved) in &table.terms {
        if let Some(existing) = terms.get(term_name.as_ref())
            && existing.iri != resolved.iri
        {
            *eligibility = FastpathEligibility::Ineligible(IneligibilityReason::UserOverridesCoreTerm);
            return;
        }
        // Treat term as a prefix definition iff its IRI ends in `/` or `#`
        // (the conventional JSON-LD §4.4 idiom for a namespace prefix).
        if resolved.iri.ends_with('/') || resolved.iri.ends_with('#') {
            prefix_table.push((term_name.clone(), resolved.iri.clone()));
        }
        terms.insert(term_name.clone(), resolved.clone());
    }
}

/// Walk a resolved `@context` value and report whether it contains any
/// non-absolute string. Used by [`JsonLdEngine::batch_context_handle`] to
/// decide whether per-entity expansion can skip
/// [`resolve_relative_context_urls`].
fn detect_relative_url_policy(ctx: &serde_json::Value) -> RelativeUrlPolicy {
    fn has_relative(value: &serde_json::Value) -> bool {
        match value {
            serde_json::Value::String(s) => !is_absolute_iri(s),
            serde_json::Value::Array(items) => items.iter().any(has_relative),
            serde_json::Value::Object(map) => map.values().any(has_relative),
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => false,
        }
    }
    if has_relative(ctx) {
        RelativeUrlPolicy::MaybePresent
    } else {
        RelativeUrlPolicy::Absent
    }
}

/// Conservative absolute-IRI check used by [`detect_relative_url_policy`].
/// Accepts the schemes the broker is actually willing to emit in a
/// resolved batch context.
fn is_absolute_iri(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://") || s.starts_with("urn:")
}

/// Fused NGSI-LD attribute normalization + JSON-LD expansion in a single
/// recursive walk.
///
/// Equivalent in observable behaviour to running
/// [`crate::service::entity::normalize_entity_attributes`] followed by
/// [`jsonld::JsonLdProcessor::expand`], but produces the expanded form in
/// one descent without the intermediate normalized DOM.
///
/// Returns `None` whenever any substructure cannot be proven canonically
/// equivalent to the slow path; the caller then falls back to the full
/// expander.
fn fast_normalize_and_expand_entity(document: &serde_json::Value, term_map: &BatchTermMap) -> Option<serde_json::Value> {
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
fn fast_expand_entity(obj: &serde_json::Map<String, serde_json::Value>, term_map: &BatchTermMap) -> Option<serde_json::Value> {
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
                    serde_json::Value::String(s) => vec![serde_json::Value::String(term_map.expand_iri(s).into_owned())],
                    serde_json::Value::Array(arr) => {
                        let mut out = Vec::with_capacity(arr.len());
                        for item in arr {
                            let s = item.as_str()?;
                            out.push(serde_json::Value::String(term_map.expand_iri(s).into_owned()));
                        }
                        out
                    }
                    serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::Object(_) => return None,
                };
                expanded.insert("@type".to_string(), serde_json::Value::Array(iris));
            }
            "scope" => {
                let scope_values = fast_expand_scope(value)?;
                expanded.insert(static_keys::SCOPE.clone(), scope_values);
            }
            "expiresAt" | "createdAt" | "modifiedAt" | "deletedAt" => {
                // NGSI-LD §5.2.4 entity-level temporal metadata: the core
                // context coerces these to xsd:dateTime via @type=DateTime.
                // They are *not* attribute slots, so the fused walker
                // emits the expanded form directly without going through
                // the implicit-Property wrap rule.
                let iri = term_map.expand_iri(key).into_owned();
                let timestamp = value.as_str()?;
                let mut node = serde_json::Map::with_capacity(2);
                node.insert("@type".to_owned(), serde_json::Value::String(static_keys::DATE_TIME.clone()));
                node.insert("@value".to_owned(), serde_json::Value::String(timestamp.to_owned()));
                expanded.insert(iri, serde_json::Value::Array(vec![serde_json::Value::Object(node)]));
            }
            other if other.starts_with('@') => return None,
            attr_name => {
                // Per-entity divergence rule: attributes whose term carries
                // a scoped `@context` are not safe for the fastpath.
                if term_map.term_has_scoped_context(attr_name) {
                    return None;
                }
                let iri = term_map.expand_iri(attr_name).into_owned();
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
/// expanded form. Accepts either a single instance object, an array of
/// instance objects (multi-dataset attribute), or a bare scalar (which
/// the NGSI-LD concise representation rule wraps as an implicit Property
/// per §5.2.4).
fn fast_expand_attribute(value: &serde_json::Value, term_map: &BatchTermMap) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::Object(instance) => {
            let expanded = fast_expand_attribute_instance(instance, term_map)?;
            Some(serde_json::Value::Array(vec![expanded]))
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return None;
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let instance = item.as_object()?;
                out.push(fast_expand_attribute_instance(instance, term_map)?);
            }
            Some(serde_json::Value::Array(out))
        }
        serde_json::Value::String(_) | serde_json::Value::Number(_) | serde_json::Value::Bool(_) => fast_implicit_property_attribute(value),
        serde_json::Value::Null => None,
    }
}

/// Emit the JSON-LD expanded form for an implicit Property whose value is
/// a JSON scalar. Equivalent to wrapping the input as
/// `{"type": "Property", "value": <scalar>}` and running it through
/// [`fast_expand_attribute_instance`], but synthesised in place.
fn fast_implicit_property_attribute(scalar: &serde_json::Value) -> Option<serde_json::Value> {
    let mut value_node = serde_json::Map::with_capacity(1);
    value_node.insert("@value".to_owned(), scalar.clone());
    let mut instance = serde_json::Map::with_capacity(2);
    instance.insert(
        "@type".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::String(static_keys::PROPERTY.clone())]),
    );
    instance.insert(
        static_keys::VALUE.clone(),
        serde_json::Value::Array(vec![serde_json::Value::Object(value_node)]),
    );
    Some(serde_json::Value::Array(vec![serde_json::Value::Object(instance)]))
}

/// Expand a single attribute instance object into its JSON-LD node form
/// (without the enclosing array).
fn fast_expand_attribute_instance(instance: &serde_json::Map<String, serde_json::Value>, term_map: &BatchTermMap) -> Option<serde_json::Value> {
    let attr_type = instance.get("type")?.as_str()?;
    let kind = AttrKind::from_type_name(attr_type)?;

    let mut out = serde_json::Map::with_capacity(instance.len());
    out.insert("@type".to_owned(), serde_json::Value::Array(vec![serde_json::Value::String(kind.iri_string())]));

    fast_emit_attribute_value(&mut out, instance, kind, term_map)?;

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

    /// Owned IRI for the attribute kind, sourced from the precomputed
    /// [`static_keys`] cache so the per-entity insertion does not pay for an
    /// `as_str().to_owned()` round-trip on every call.
    fn iri_string(self) -> String {
        match self {
            AttrKind::Property => static_keys::PROPERTY.clone(),
            AttrKind::Relationship => static_keys::RELATIONSHIP.clone(),
            AttrKind::GeoProperty => static_keys::GEO_PROPERTY.clone(),
            AttrKind::LanguageProperty => static_keys::LANGUAGE_PROPERTY.clone(),
            AttrKind::VocabProperty => static_keys::VOCAB_PROPERTY.clone(),
            AttrKind::JsonProperty => static_keys::JSON_PROPERTY.clone(),
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
/// object. Returns `Some(())` when the value key is present and the shape
/// is supported, and `None` when the shape cannot be expanded safely
/// (caller falls back to the slow path).
fn fast_emit_attribute_value(
    out: &mut serde_json::Map<String, serde_json::Value>,
    instance: &serde_json::Map<String, serde_json::Value>,
    kind: AttrKind,
    term_map: &BatchTermMap,
) -> Option<()> {
    match kind {
        AttrKind::Property => {
            let value = instance.get("value")?;
            let wrapped = fast_wrap_property_value(value)?;
            out.insert(static_keys::VALUE.clone(), wrapped);
            Some(())
        }
        AttrKind::Relationship => {
            let object = instance.get("object")?.as_str()?;
            let mut node = serde_json::Map::with_capacity(1);
            node.insert("@id".to_owned(), serde_json::Value::String(object.to_owned()));
            out.insert(static_keys::OBJECT.clone(), serde_json::Value::Array(vec![serde_json::Value::Object(node)]));
            Some(())
        }
        AttrKind::GeoProperty => {
            let value = instance.get("value")?.as_object()?;
            let wrapped = fast_expand_geojson_node(value)?;
            out.insert(static_keys::VALUE.clone(), serde_json::Value::Array(vec![wrapped]));
            Some(())
        }
        AttrKind::LanguageProperty => {
            let map = instance.get("languageMap")?.as_object()?;
            let wrapped = fast_expand_language_map(map)?;
            out.insert(static_keys::LANGUAGE_MAP.clone(), wrapped);
            Some(())
        }
        AttrKind::VocabProperty => {
            let vocab_value = instance.get("vocab")?;
            let wrapped = fast_expand_vocab_value(vocab_value, term_map)?;
            out.insert(static_keys::VOCAB.clone(), wrapped);
            Some(())
        }
        AttrKind::JsonProperty => {
            let json_value = instance.get("json")?;
            let mut node = serde_json::Map::with_capacity(2);
            node.insert("@type".to_owned(), serde_json::Value::String("@json".to_owned()));
            node.insert("@value".to_owned(), json_value.clone());
            out.insert(static_keys::JSON.clone(), serde_json::Value::Array(vec![serde_json::Value::Object(node)]));
            Some(())
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
    let type_iri = compact_to_expanded(geom_type)?;

    let mut out = serde_json::Map::with_capacity(obj.len());
    out.insert(
        "@type".to_owned(),
        serde_json::Value::Array(vec![serde_json::Value::String(type_iri.to_owned())]),
    );

    for (key, value) in obj {
        match key.as_str() {
            "type" => {}
            "coordinates" => {
                let list = fast_expand_geojson_coordinates(value)?;
                out.insert(static_keys::COORDINATES.clone(), serde_json::Value::Array(vec![list]));
            }
            "bbox" => {
                let list = fast_expand_geojson_coordinates(value)?;
                out.insert(static_keys::BBOX.clone(), serde_json::Value::Array(vec![list]));
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
fn fast_expand_vocab_value(value: &serde_json::Value, term_map: &BatchTermMap) -> Option<serde_json::Value> {
    match value {
        serde_json::Value::String(s) => {
            let iri = term_map.expand_iri(s).into_owned();
            let mut node = serde_json::Map::with_capacity(1);
            node.insert("@id".to_owned(), serde_json::Value::String(iri));
            Some(serde_json::Value::Array(vec![serde_json::Value::Object(node)]))
        }
        serde_json::Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let s = item.as_str()?;
                let iri = term_map.expand_iri(s).into_owned();
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
fn fast_expand_attribute_metadata(key: &str, value: &serde_json::Value, term_map: &BatchTermMap) -> Option<(String, serde_json::Value)> {
    // Per-entity divergence rule: metadata key with scoped @context bails.
    if term_map.term_has_scoped_context(key) {
        return None;
    }
    let iri = term_map.expand_iri(key).into_owned();
    let coercion = term_map.coercion(key);

    match value {
        serde_json::Value::String(s) => match coercion {
            Some(TermCoercion::DateTime) => {
                let mut node = serde_json::Map::with_capacity(2);
                node.insert("@type".to_owned(), serde_json::Value::String(static_keys::DATE_TIME.clone()));
                node.insert("@value".to_owned(), serde_json::Value::String(s.clone()));
                Some((iri, serde_json::Value::Array(vec![serde_json::Value::Object(node)])))
            }
            Some(TermCoercion::Id) => {
                let mut node = serde_json::Map::with_capacity(1);
                node.insert("@id".to_owned(), serde_json::Value::String(s.clone()));
                Some((iri, serde_json::Value::Array(vec![serde_json::Value::Object(node)])))
            }
            Some(TermCoercion::TypeIri(raw)) => {
                let resolved = term_map.expand_iri(&raw).into_owned();
                let mut node = serde_json::Map::with_capacity(2);
                node.insert("@type".to_owned(), serde_json::Value::String(resolved));
                node.insert("@value".to_owned(), serde_json::Value::String(s.clone()));
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

    async fn core_handle(engine: &JsonLdEngine) -> BatchContextHandle {
        let spec = ContextSpec::core_only();
        engine.batch_context_handle(&spec).await
    }

    #[tokio::test]
    async fn fastpath_simple_property_byte_equal_to_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine).await;
        assert!(handle.fast_eligible(), "core-only spec must enable fastpath");

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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
        let doc = json!({
            "id": "urn:ngsi-ld:Device:sc2",
            "type": "Device",
            "scope": ["root/a", "root/b"]
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    async fn fast_expand_or_none(engine: &JsonLdEngine, document: serde_json::Value) -> Option<serde_json::Value> {
        let handle = core_handle(engine).await;
        assert!(handle.fast_eligible(), "fastpath enabled");
        fast_normalize_and_expand_entity(&document, &handle.term_map)
    }

    #[tokio::test]
    async fn fastpath_hits_simple_property() {
        let engine = test_engine();
        let doc = json!({"id": "urn:x:1", "type": "X", "a": {"type": "Property", "value": 1}});
        assert!(fast_expand_or_none(&engine, doc).await.is_some());
    }

    #[tokio::test]
    async fn fastpath_hits_geoproperty() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:2", "type": "X",
            "loc": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.0, 2.0]}}
        });
        assert!(fast_expand_or_none(&engine, doc).await.is_some());
    }

    #[tokio::test]
    async fn fastpath_hits_language_property() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:3", "type": "X",
            "desc": {"type": "LanguageProperty", "languageMap": {"en": "hi"}}
        });
        assert!(fast_expand_or_none(&engine, doc).await.is_some());
    }

    #[tokio::test]
    async fn fastpath_hits_vocab_property() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:4", "type": "X",
            "cat": {"type": "VocabProperty", "vocab": "sensor"}
        });
        assert!(fast_expand_or_none(&engine, doc).await.is_some());
    }

    #[tokio::test]
    async fn fastpath_hits_json_property() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:5", "type": "X",
            "cfg": {"type": "JsonProperty", "json": {"k": 1}}
        });
        assert!(fast_expand_or_none(&engine, doc).await.is_some());
    }

    #[tokio::test]
    async fn fastpath_misses_on_inline_divergent_context() {
        let engine = test_engine();
        let doc = json!({
            "@context": {"@vocab": "https://example.com/"},
            "id": "urn:x:6", "type": "X"
        });
        assert!(fast_expand_or_none(&engine, doc).await.is_none());
    }

    #[tokio::test]
    async fn fastpath_misses_on_unknown_attribute_type() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:7", "type": "X",
            "weird": {"type": "SomethingElse", "value": 1}
        });
        assert!(fast_expand_or_none(&engine, doc).await.is_none());
    }

    #[tokio::test]
    async fn fastpath_property_with_nested_sub_property_matches_full_expand() {
        let engine = test_engine();
        let handle = core_handle(&engine).await;
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
        let handle = core_handle(&engine).await;
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

    #[tokio::test]
    async fn fastpath_misses_on_property_with_object_value() {
        let engine = test_engine();
        let doc = json!({
            "id": "urn:x:8", "type": "X",
            "complex": {"type": "Property", "value": {"nested": true}}
        });
        assert!(fast_expand_or_none(&engine, doc).await.is_none());
    }

    #[tokio::test]
    async fn fastpath_enabled_for_default_context_spec() {
        let engine = test_engine();
        let spec = ContextSpec::none();
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "default spec (core + andromeda only) must enable the fastpath");
    }

    #[tokio::test]
    async fn fastpath_enabled_for_link_header_containing_only_core_urls() {
        let engine = test_engine();
        let spec = ContextSpec::from_links(vec![NGSI_LD_CORE_CONTEXT_URL.to_string(), ANDROMEDA_CORE_CONTEXT_URL.to_string()]);
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "core-only Link header must enable the fastpath");
    }

    #[tokio::test]
    async fn fastpath_eligible_for_vocab_only_body_context() {
        // Step 5: a body context that only redefines `@vocab` keeps the
        // handle fast-eligible because every term still resolves against
        // an unambiguous IRI (the BatchTermMap routes attributes through
        // the new vocab consistently with the slow path).
        let engine = test_engine();
        let spec = ContextSpec::none().with_body_context(Some(json!({"@vocab": "https://example.com/vocab#"})));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "@vocab-only body context stays fast-eligible after Step 5");
    }

    #[tokio::test]
    async fn fastpath_disabled_when_body_context_overrides_core_term() {
        // Plan §5.2: user term IRI differs from the existing core mapping
        // for the same name -> Ineligible(UserOverridesCoreTerm).
        let engine = test_engine();
        let spec = ContextSpec::none().with_body_context(Some(json!({"observedAt": "https://example.com/own/observedAt"})));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible(), "user override of core term must disable the fastpath");
    }

    #[tokio::test]
    async fn fastpath_disabled_for_custom_link_context_spec() {
        let engine = test_engine();
        let spec = ContextSpec::from_links(vec!["https://example.com/custom.jsonld".to_string()]);
        let handle = engine.batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible(), "user link context must disable the fastpath");
    }

    #[tokio::test]
    async fn batch_term_map_is_send_sync() {
        // BatchTermMap holds Boxed strings + HashMap + Vec; it cannot be
        // Copy, but it must be Send + Sync so spawn_blocking workers can
        // hold a clone.
        const fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<BatchTermMap>();
        let engine = test_engine();
        let handle = core_handle(&engine).await;
        assert!(handle.fast_eligible(), "core-only handle must be fast-eligible");
    }

    #[tokio::test]
    async fn relative_url_policy_absent_for_canonical_core_only_spec() {
        let engine = test_engine();
        let spec = ContextSpec::core_only();
        let handle = engine.batch_context_handle(&spec).await;
        pretty_assertions::assert_eq!(handle.relative_url_policy, RelativeUrlPolicy::Absent);
    }

    #[tokio::test]
    async fn relative_url_policy_maybe_present_when_user_relative_url() {
        let engine = test_engine();
        let spec = ContextSpec::none().with_body_context(Some(json!(["foo"])));
        let handle = engine.batch_context_handle(&spec).await;
        pretty_assertions::assert_eq!(handle.relative_url_policy, RelativeUrlPolicy::MaybePresent);
    }

    #[tokio::test]
    async fn fused_walk_implicit_property_byte_match() {
        let engine = test_engine();
        let handle = core_handle(&engine).await;
        let raw = json!({
            "id": "urn:ngsi-ld:Device:fused1",
            "type": "Device",
            "speed": 42
        });
        let mut normalized = raw.clone();
        crate::service::entity::normalize_entity_attributes(&mut normalized);
        let slow = engine.expand(normalized, &handle.spec).await.expect("slow");
        let fast = engine.expand_with_handle(raw, &handle).await.expect("fast");
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fused_walk_explicit_property_with_subattrs_match() {
        let engine = test_engine();
        let handle = core_handle(&engine).await;
        let doc = json!({
            "id": "urn:ngsi-ld:Device:fused2",
            "type": "Device",
            "temperature": {
                "type": "Property",
                "value": 21,
                "providedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Sensor:s1"}
            }
        });
        let (fast, slow) = expand_both_paths(&engine, doc, &handle).await;
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn fused_walk_array_attr_normalises_per_instance() {
        let engine = test_engine();
        let handle = core_handle(&engine).await;
        let doc = json!({
            "id": "urn:ngsi-ld:Device:fused3",
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
    async fn fused_walk_reserved_keys_skipped() {
        let engine = test_engine();
        let handle = core_handle(&engine).await;
        let doc = json!({
            "id": "urn:ngsi-ld:Device:fused4",
            "type": "Device",
            "expiresAt": "2030-01-01T00:00:00Z",
            "scope": "root/a",
            "speed": 42
        });
        let raw_for_slow = doc.clone();
        let mut normalized = raw_for_slow;
        crate::service::entity::normalize_entity_attributes(&mut normalized);
        // Verify normalize did not wrap any of the reserved keys as Property.
        let entity_obj = normalized.as_object().expect("object");
        let expires = entity_obj.get("expiresAt").expect("expiresAt present");
        assert!(expires.is_string(), "expiresAt must remain a string after normalize");
        let scope = entity_obj.get("scope").expect("scope present");
        assert!(scope.is_string(), "scope must remain a string after normalize");
        let id = entity_obj.get("id").expect("id present");
        assert!(id.is_string(), "id must remain a string after normalize");
        let typ = entity_obj.get("type").expect("type present");
        assert!(typ.is_string(), "type must remain a string after normalize");

        let slow = engine.expand(normalized, &handle.spec).await.expect("slow");
        let fast = engine.expand_with_handle(doc, &handle).await.expect("fast");
        pretty_assertions::assert_eq!(fast, slow);
    }

    #[tokio::test]
    async fn resolved_context_arc_shared_across_entities() {
        let engine = test_engine();
        let handle = core_handle(&engine).await;
        let arc = handle.resolved_context.as_ref().expect("resolved context for core spec");
        let baseline = Arc::strong_count(arc);

        // Cloning the handle bumps the Arc refcount instead of deep-cloning
        // the resolved context value.
        let cloned_handles: Vec<BatchContextHandle> = (0..16).map(|_| handle.clone()).collect();
        let arc_after_clone = handle.resolved_context.as_ref().expect("still present");
        assert!(Arc::strong_count(arc_after_clone) >= baseline + cloned_handles.len());

        drop(cloned_handles);
        let arc_after_drop = handle.resolved_context.as_ref().expect("still present");
        pretty_assertions::assert_eq!(Arc::strong_count(arc_after_drop), baseline);
    }

    fn bulk_doc(idx: usize) -> serde_json::Value {
        json!({
            "@context": NGSI_LD_CORE_CONTEXT_URL,
            "id": format!("urn:ngsi-ld:Device:bulk-{idx}"),
            "type": "Device"
        })
    }

    #[tokio::test]
    async fn bulk_expand_n_entities_per_index_match() {
        let engine = test_engine();
        let docs: Vec<serde_json::Value> = (0..10).map(bulk_doc).collect();
        let bulk = engine.expand_prepared_array(docs.clone(), context::CoreContextPolicy::Append).await;
        pretty_assertions::assert_eq!(bulk.len(), docs.len());
        for (i, (bulk_result, original)) in bulk.iter().zip(docs.iter()).enumerate() {
            let bulk_value = bulk_result.as_ref().expect("bulk index ok");
            let solo = engine
                .expand_prepared(original.clone(), context::CoreContextPolicy::Append, RelativeUrlPolicy::MaybePresent)
                .await
                .expect("solo expand");
            pretty_assertions::assert_eq!(bulk_value, &solo, "bulk[{i}] must match individual expand");
        }
    }

    #[tokio::test]
    async fn bulk_expand_invalid_entity_attributes_to_index() {
        let engine = test_engine();
        let mut docs: Vec<serde_json::Value> = (0..10).map(bulk_doc).collect();
        // Replace entity 5 with an inline @context that points to an
        // unreachable URL (allow_remote=false); bulk and per-entity expand
        // should both surface the error specifically at index 5.
        docs[5] = json!({
            "@context": "https://example.com/missing-context.jsonld",
            "id": "urn:ngsi-ld:Device:bulk-5",
            "type": "Device"
        });
        let bulk = engine.expand_prepared_array(docs.clone(), context::CoreContextPolicy::Append).await;
        pretty_assertions::assert_eq!(bulk.len(), 10);
        assert!(bulk[5].is_err(), "entity 5 must report error");
        for (i, slot) in bulk.iter().enumerate() {
            if i == 5 {
                continue;
            }
            assert!(slot.is_ok(), "entity {i} must succeed; got {slot:?}");
        }
    }

    #[tokio::test]
    async fn bulk_expand_empty_input_returns_empty() {
        let engine = test_engine();
        let bulk = engine.expand_prepared_array(Vec::new(), context::CoreContextPolicy::Append).await;
        assert!(bulk.is_empty());
    }

    #[tokio::test]
    async fn bulk_expand_single_element_produces_correct_result() {
        let engine = test_engine();
        let docs = vec![bulk_doc(0)];
        let bulk = engine.expand_prepared_array(docs.clone(), context::CoreContextPolicy::Append).await;
        pretty_assertions::assert_eq!(bulk.len(), 1);
        let bulk_value = bulk[0].as_ref().expect("single ok");
        let solo = engine
            .expand_prepared(docs[0].clone(), context::CoreContextPolicy::Append, RelativeUrlPolicy::MaybePresent)
            .await
            .expect("solo");
        pretty_assertions::assert_eq!(bulk_value, &solo);
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

    // -----------------------------------------------------------------
    // Step 5 — generalised BatchTermMap test suite
    // -----------------------------------------------------------------

    /// Canonical-equivalence comparator: recursively sorts object keys
    /// before comparing so two expansions that differ only in JSON object
    /// member order register as equal. Adequate for the Step 5 user-context
    /// tests; deeper canonicalisation (blank-node relabelling, list multi-
    /// set semantics) is out of scope for this comparator.
    fn canonical(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(map) => {
                let mut entries: Vec<(String, serde_json::Value)> = map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                serde_json::Value::Object(entries.into_iter().collect())
            }
            serde_json::Value::Array(items) => serde_json::Value::Array(items.iter().map(canonical).collect()),
            other => other.clone(),
        }
    }

    fn assert_jsonld_canonically_equal(a: &serde_json::Value, b: &serde_json::Value) {
        pretty_assertions::assert_eq!(canonical(a), canonical(b));
    }

    fn body_spec(body: serde_json::Value) -> ContextSpec {
        ContextSpec::none().with_body_context(Some(body))
    }

    #[tokio::test]
    async fn gen_fastpath_inline_user_context_custom_term() {
        let engine = test_engine();
        let spec = body_spec(json!({"mySpeed": "http://ex.com/speed"}));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "custom term in body context stays fast-eligible");
        let doc = json!({
            "id": "urn:ngsi-ld:Device:gen1",
            "type": "X",
            "mySpeed": {"type": "Property", "value": 42}
        });
        let fast = engine.expand_with_handle(doc.clone(), &handle).await.expect("fast");
        let slow = engine.expand(doc, &spec).await.expect("slow");
        assert!(
            fast[0].as_object().expect("entity object").contains_key("http://ex.com/speed"),
            "expected user IRI in fastpath output"
        );
        assert_jsonld_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn gen_fastpath_url_user_context_custom_term() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time").as_nanos();
        let path = std::env::temp_dir().join(format!("engine-user-context-{unique}.jsonld"));
        fs::write(&path, r#"{"@context":{"mySpeed":"http://ex.com/speed"}}"#).expect("write user context");
        let url = format!("https://example.com/user-context-{unique}.jsonld");

        let mut preloads = HashMap::new();
        preloads.insert(url.clone(), path.to_string_lossy().to_string());
        let engine = JsonLdEngine::builder().allow_remote(false).preload_contexts(preloads).build().expect("engine");

        let spec = ContextSpec::from_links(vec![url]);
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "URL user context stays fast-eligible");

        let doc = json!({
            "id": "urn:ngsi-ld:Device:gen2",
            "type": "X",
            "mySpeed": {"type": "Property", "value": 42}
        });
        let fast = engine.expand_with_handle(doc.clone(), &handle).await.expect("fast");
        let slow = engine.expand(doc, &spec).await.expect("slow");
        assert_jsonld_canonically_equal(&fast, &slow);

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn gen_fastpath_user_context_type_coercion() {
        // Step 5 §5.5: user-declared coercion via `xsd:dateTime`. The
        // user context must declare the `xsd` prefix because JSON-LD does
        // not provide one by default — both fast and slow paths then
        // resolve the IRI consistently.
        let engine = test_engine();
        let spec = body_spec(json!({
            "xsd": "http://www.w3.org/2001/XMLSchema#",
            "timestamp": {"@id": "http://ex.com/ts", "@type": "xsd:dateTime"}
        }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible(), "DateTime coercion via declared xsd:dateTime stays eligible");
        let doc = json!({
            "id": "urn:ngsi-ld:Device:gen3",
            "type": "X",
            "ts": {"type": "Property", "value": "2025-01-01T00:00:00Z", "timestamp": "2025-01-02T00:00:00Z"}
        });
        let fast = engine.expand_with_handle(doc.clone(), &handle).await.expect("fast");
        let slow = engine.expand(doc, &spec).await.expect("slow");
        assert_jsonld_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn gen_fastpath_user_overrides_core_falls_back() {
        let engine = test_engine();
        let spec = body_spec(json!({"observedAt": "http://ex.com/own/observedAt"}));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible(), "user override of observedAt must mark Ineligible");
        let doc = json!({
            "id": "urn:ngsi-ld:Device:gen4",
            "type": "X",
            "speed": {"type": "Property", "value": 42, "observedAt": "2025-01-01T00:00:00Z"}
        });
        let fast = engine.expand_with_handle(doc.clone(), &handle).await.expect("fast");
        let slow = engine.expand(doc, &spec).await.expect("slow");
        assert_jsonld_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn gen_fastpath_scoped_property_context_falls_back() {
        let engine = test_engine();
        let spec = body_spec(json!({
            "sensor": {"@id": "http://ex.com/sensor", "@context": {"reading": "http://ex.com/reading"}}
        }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(!handle.fast_eligible(), "scoped @context on a term must mark Ineligible");
    }

    #[tokio::test]
    async fn gen_fastpath_xsd_prefix_expansion() {
        let engine = test_engine();
        let spec = body_spec(json!({
            "size": {"@id": "http://ex.com/size", "@type": "xsd:string"}
        }));
        let handle = engine.batch_context_handle(&spec).await;
        // xsd:string is not a coercion the fastpath models, but xsd: must
        // resolve via the built-in prefix.
        let expansion = handle.term_map.expand_iri("xsd:string");
        pretty_assertions::assert_eq!(expansion.as_ref(), "http://www.w3.org/2001/XMLSchema#string");
    }

    #[tokio::test]
    async fn gen_fastpath_per_entity_inline_context_falls_back() {
        let engine = test_engine();
        let handle = core_handle(&engine).await;
        let doc = json!({
            "@context": {"@vocab": "https://example.com/vocab#"},
            "id": "urn:ngsi-ld:Device:gen6",
            "type": "X"
        });
        let fast = engine.expand_with_handle(doc.clone(), &handle).await.expect("fast");
        let slow = engine.expand(doc, &handle.spec).await.expect("slow");
        assert_jsonld_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn gen_fastpath_prefix_table_longest_first() {
        // Two overlapping prefixes (`ex:` and `exFoo:`); longest must win.
        let engine = test_engine();
        let spec = body_spec(json!({
            "ex": "http://ex.com/short/",
            "exFoo": "http://ex.com/long/"
        }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible());
        let expanded = handle.term_map.expand_iri("exFoo:bar");
        pretty_assertions::assert_eq!(expanded.as_ref(), "http://ex.com/long/bar");
    }

    #[tokio::test]
    async fn gen_fastpath_user_term_with_at_id_only() {
        let engine = test_engine();
        let spec = body_spec(json!({
            "foo": {"@id": "http://ex.com/foo"}
        }));
        let handle = engine.batch_context_handle(&spec).await;
        assert!(handle.fast_eligible());
        let doc = json!({
            "id": "urn:ngsi-ld:Device:gen8",
            "type": "X",
            "foo": {"type": "Property", "value": 1}
        });
        let fast = engine.expand_with_handle(doc.clone(), &handle).await.expect("fast");
        let slow = engine.expand(doc, &spec).await.expect("slow");
        assert!(
            fast[0].as_object().expect("entity").contains_key("http://ex.com/foo"),
            "user IRI must appear in expanded form"
        );
        assert_jsonld_canonically_equal(&fast, &slow);
    }

    #[tokio::test]
    async fn term_table_caches_per_url() {
        // Build two handles for the same URL and assert the user term
        // table cache is hit (cache size grows by exactly one entry).
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("system time").as_nanos();
        let path = std::env::temp_dir().join(format!("engine-user-cache-{unique}.jsonld"));
        fs::write(&path, r#"{"@context":{"foo":"http://ex.com/foo"}}"#).expect("write");
        let url = format!("https://example.com/user-cache-{unique}.jsonld");

        let mut preloads = HashMap::new();
        preloads.insert(url.clone(), path.to_string_lossy().to_string());
        let engine = JsonLdEngine::builder().allow_remote(false).preload_contexts(preloads).build().expect("engine");

        let spec = ContextSpec::from_links(vec![url.clone()]);
        let _h1 = engine.batch_context_handle(&spec).await;
        pretty_assertions::assert_eq!(engine.user_term_table_cache.len(), 1, "first handle populates the cache");
        let _h2 = engine.batch_context_handle(&spec).await;
        pretty_assertions::assert_eq!(engine.user_term_table_cache.len(), 1, "second handle reuses the cache entry");

        let _ = fs::remove_file(path);
    }
}
