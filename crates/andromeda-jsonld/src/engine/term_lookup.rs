//! Expanding a single term, and the cache that makes it cheap.
//!
//! A `q` filter, an `orderBy` clause and an `attrs` list all name terms one at
//! a time, and expanding one means handing the processor a synthetic document
//! built for the purpose. Doing that per term per request is what the cache
//! here removes: the answer depends only on the term and the resolved
//! `@context`, so both go into the key.

use crate::{
    context_spec::{ContextSpec, CoreContextPolicy},
    document_context::resolve_context_value,
    engine::{context_url::is_full_iri, processor::JsonLdEngine},
    error::JsonLdError,
    loader::cache_lifetime::CacheLifetime,
};
use iri_rs::IriBuf;
use moka::{Equivalent, future::Cache};
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// How often the cache reports its hit rate.
const STATS_INTERVAL: u64 = 1000;

/// Expanded terms, keyed by term and resolved context.
#[derive(Clone, Debug)]
pub(crate) struct TermCache {
    /// The cached expansions.
    expansions: Cache<TermCacheKey, IriBuf>,
    /// How many lookups were answered from the cache.
    hits: Arc<AtomicU64>,
    /// How many lookups had to expand.
    misses: Arc<AtomicU64>,
}

impl TermCache {
    /// A cache bounded by entry count and entry age.
    pub(crate) fn new(capacity: u64, ttl: CacheLifetime) -> TermCache {
        TermCache {
            expansions: Cache::builder().max_capacity(capacity).time_to_live(ttl.as_duration()).build(),
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The cached expansion of a key, recording the outcome either way.
    async fn get(&self, key: &TermCacheKeyRef<'_>) -> Option<IriBuf> {
        let cached = self.expansions.get(key).await;
        let counter = if cached.is_some() { &self.hits } else { &self.misses };
        let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_multiple_of(STATS_INTERVAL) {
            self.log_stats();
        }
        cached
    }

    /// Record an expansion.
    async fn insert(&self, key: TermCacheKey, iri: IriBuf) {
        self.expansions.insert(key, iri).await;
    }

    /// Report the hit rate.
    fn log_stats(&self) {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            return;
        }
        // Basis points computed in `u128` rather than a floating-point ratio:
        // both counters are `u64`, and an `f64` division silently loses
        // precision once either passes the 53-bit mantissa.
        let hit_rate_basis_points = u128::from(hits) * 10_000 / u128::from(total);
        tracing::debug!(
            hits,
            misses,
            total,
            hit_rate = format_args!("{}.{:02}%", hit_rate_basis_points / 100, hit_rate_basis_points % 100),
            "JSON-LD term cache statistics"
        );
    }
}

/// What kind of name is being expanded, which decides the synthetic document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TermRole {
    /// An entity type, expanded through the document's `@type`.
    Type,
    /// An attribute name, expanded as a member key.
    Attribute,
}

/// A context prepared once for all term lookups in one operation.
///
/// Resolution and canonical serialization belong at the operation boundary,
/// rather than at each lookup. The resolved JSON value is also the exact value
/// attached to synthetic documents, which keeps cache identity and processor
/// input inseparable.
#[derive(Debug, Clone)]
pub struct PreparedTermContext {
    /// The effective context that synthetic documents carry.
    resolved: Option<Arc<Value>>,
    /// Equality-safe identity for cache entries under this effective context.
    identity: Option<Arc<str>>,
    /// Kept for processor diagnostics when expansion fails.
    core_policy: CoreContextPolicy,
    /// Counts preparation work in unit tests without affecting release builds.
    #[cfg(test)]
    preparation_count: Arc<AtomicU64>,
}

impl PreparedTermContext {
    /// Resolve a request context once for the operation that will expand terms.
    fn from_spec(spec: &ContextSpec) -> PreparedTermContext {
        let resolved = resolve_context_value(None, spec).map(Arc::new);
        let identity = resolved.as_ref().map(|value| Arc::<str>::from(value.to_string()));
        PreparedTermContext {
            resolved,
            identity,
            core_policy: spec.core_policy(),
            #[cfg(test)]
            preparation_count: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Attach this operation's effective context to a synthetic document.
    fn apply_to(&self, mut document: Value) -> Value {
        if let Some(resolved) = &self.resolved
            && let Some(object) = document.as_object_mut()
        {
            // The JSON-LD processor consumes the synthetic document, so this
            // clone is required while the prepared value remains reusable.
            object.insert("@context".to_string(), Value::clone(resolved));
        }
        document
    }

    /// The processor diagnostic policy selected by the request context.
    const fn core_policy(&self) -> CoreContextPolicy {
        self.core_policy
    }

    #[cfg(test)]
    /// How many times this value resolved and serialized its context.
    fn preparation_count(&self) -> u64 {
        self.preparation_count.load(Ordering::Relaxed)
    }
}

/// One expanded-term cache entry's key.
///
/// The role keeps a type and an attribute of the same spelling apart, the term
/// is the raw name before expansion, and the context uses its complete
/// canonical serialized form. Equality therefore cannot collapse distinct
/// contexts through a hash collision.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TermCacheKey {
    /// Whether an entity type or an attribute name is being expanded.
    role: TermRole,
    /// The raw term as written, before expansion: a lookup token, not yet an IRI.
    term: Box<str>,
    /// The complete canonical resolved `@context` the term is expanded against.
    context_identity: Option<Arc<str>>,
}

/// A [`TermCacheKey`] borrowed from the lookup that probes for it.
///
/// A hit allocates nothing: the owned key is built only when a miss records
/// its expansion. The fields mirror [`TermCacheKey`] in the same order and
/// derive the same hash, because a `Box<str>` and an `Arc<str>` hash exactly
/// as the `str` they hold.
#[derive(Debug, Clone, Copy, Hash)]
struct TermCacheKeyRef<'a> {
    /// Whether an entity type or an attribute name is being expanded.
    role: TermRole,
    /// The raw term as written, before expansion.
    term: &'a str,
    /// The complete canonical resolved `@context` the term is expanded against.
    context_identity: Option<&'a str>,
}

impl<'a> TermCacheKeyRef<'a> {
    /// The key one term takes under one prepared context.
    fn new(role: TermRole, term: &'a str, context: &'a PreparedTermContext) -> TermCacheKeyRef<'a> {
        TermCacheKeyRef {
            role,
            term,
            context_identity: context.identity.as_deref(),
        }
    }

    /// The owned key a miss records its expansion under.
    fn to_owned_key(self, context: &PreparedTermContext) -> TermCacheKey {
        TermCacheKey {
            role: self.role,
            term: Box::from(self.term),
            context_identity: context.identity.clone(), // clone: cache keys retain the shared canonical identity
        }
    }
}

// Not derivable: equivalence between a borrowed and an owned key is a
// relation across two types, which no derive expresses.
impl Equivalent<TermCacheKey> for TermCacheKeyRef<'_> {
    fn equivalent(&self, key: &TermCacheKey) -> bool {
        self.role == key.role && self.term == &*key.term && self.context_identity == key.context_identity.as_deref()
    }
}

impl JsonLdEngine {
    /// Resolve a context once before expanding all terms for one operation.
    #[must_use]
    pub fn prepare_term_context(&self, context: &ContextSpec) -> PreparedTermContext {
        PreparedTermContext::from_spec(context)
    }

    /// Expand an entity type term to its IRI.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::TypeNotExpanded`] when the term does not expand,
    /// [`JsonLdError::ExpandedTermNotIri`] when it expands to something other
    /// than an absolute IRI, and whatever expansion returns when a context does
    /// not resolve.
    pub async fn expand_type(&self, type_term: &str, context: &PreparedTermContext) -> Result<IriBuf, JsonLdError> {
        if is_full_iri(type_term) {
            return iri_of(type_term);
        }
        let key = TermCacheKeyRef::new(TermRole::Type, type_term, context);
        if let Some(cached) = self.term_cache().get(&key).await {
            return Ok(cached);
        }

        let document = context.apply_to(serde_json::json!({ "id": "urn:ngsi-ld:_:temp", "type": type_term }));
        let expanded = self.expand_prepared(document, context.core_policy()).await?;
        let iri = expanded
            .get(0)
            .and_then(|node| node.get("@type"))
            .and_then(|types| types.get(0))
            .and_then(Value::as_str)
            .ok_or(JsonLdError::TypeNotExpanded)
            .and_then(iri_of)?;

        tracing::trace!("Expanded type '{}' to '{}'", type_term, iri);
        self.term_cache().insert(key.to_owned_key(context), iri.clone()).await; // clone: the cache keeps a copy, the caller gets one
        Ok(iri)
    }

    /// Expand an attribute term to its IRI.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::TermNotExpanded`] when the term does not expand,
    /// [`JsonLdError::ExpandedTermNotIri`] when it expands to something other
    /// than an absolute IRI, and whatever expansion returns when a context does
    /// not resolve.
    pub async fn expand_term(&self, term: &str, context: &PreparedTermContext) -> Result<IriBuf, JsonLdError> {
        if is_full_iri(term) {
            return iri_of(term);
        }
        let key = TermCacheKeyRef::new(TermRole::Attribute, term, context);
        if let Some(cached) = self.term_cache().get(&key).await {
            return Ok(cached);
        }

        let document = context.apply_to(serde_json::json!({ term: { "@id": "urn:dummy" } }));
        let expanded = self.expand_prepared(document, context.core_policy()).await?;
        let expanded_key = expanded
            .get(0)
            .and_then(Value::as_object)
            .and_then(|node| node.keys().find(|key| *key != "@id" && *key != "@type").cloned()) // clone: the expansion is dropped below
            .ok_or(JsonLdError::TermNotExpanded)?;
        let iri = iri_of(&expanded_key)?;

        self.term_cache().insert(key.to_owned_key(context), iri.clone()).await; // clone: the cache keeps a copy, the caller gets one
        Ok(iri)
    }
}

/// Validate an expansion result as an absolute IRI.
///
/// The processor emits expansions as document strings; a
/// well-formed one is an absolute IRI, and this is where that guarantee is
/// enforced so every caller receives an [`IriBuf`] rather than re-parsing one.
fn iri_of(candidate: &str) -> Result<IriBuf, JsonLdError> {
    IriBuf::new(candidate.to_string()).map_err(JsonLdError::ExpandedTermNotIri)
}

#[cfg(test)]
mod tests {
    use crate::{
        context_spec::ContextSpec,
        engine::{
            processor::JsonLdEngine,
            term_lookup::{TermCacheKeyRef, TermRole},
        },
    };
    use moka::Equivalent;
    use serde_json::json;
    use std::hash::{BuildHasher, RandomState};

    #[tokio::test]
    async fn repeated_lookups_reuse_one_prepared_context() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = engine.prepare_term_context(&ContextSpec::none());

        let first = engine.expand_term("speed", &context).await.expect("first lookup expands");
        let second = engine.expand_term("speed", &context).await.expect("second lookup expands");

        pretty_assertions::assert_eq!(first, second);
        assert_eq!(context.preparation_count(), 1);
    }

    #[tokio::test]
    async fn distinct_prepared_contexts_cannot_share_term_cache_entries() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let first_context = ContextSpec::from_body(json!({ "custom": "https://example.com/first/custom" }));
        let second_context = ContextSpec::from_body(json!({ "custom": "https://example.com/second/custom" }));
        let first_context = engine.prepare_term_context(&first_context);
        let second_context = engine.prepare_term_context(&second_context);

        let first = engine.expand_term("custom", &first_context).await.expect("first context expands");
        let second = engine.expand_term("custom", &second_context).await.expect("second context expands");

        assert_ne!(first, second);
        assert_eq!(first.as_str(), "https://example.com/first/custom");
        assert_eq!(second.as_str(), "https://example.com/second/custom");
    }

    /// The cache is probed with a borrowed key and filled with an owned one,
    /// so the two must hash alike and compare equal, or every probe misses.
    #[test]
    fn a_borrowed_key_hashes_and_compares_like_the_owned_key_it_stands_for() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = engine.prepare_term_context(&ContextSpec::none());
        let borrowed = TermCacheKeyRef::new(TermRole::Attribute, "speed", &context);
        let owned = borrowed.to_owned_key(&context);
        let state = RandomState::new();

        pretty_assertions::assert_eq!(state.hash_one(borrowed), state.hash_one(&owned));
        assert!(borrowed.equivalent(&owned));
    }

    /// Equality, not the hash, decides a hit: a borrowed key under another
    /// context or for the other role is never equivalent.
    #[test]
    fn a_borrowed_key_under_another_context_or_role_is_not_equivalent() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let first = engine.prepare_term_context(&ContextSpec::from_body(json!({ "custom": "https://example.com/first/custom" })));
        let second = engine.prepare_term_context(&ContextSpec::from_body(json!({ "custom": "https://example.com/second/custom" })));
        let owned = TermCacheKeyRef::new(TermRole::Attribute, "custom", &first).to_owned_key(&first);

        assert!(!TermCacheKeyRef::new(TermRole::Attribute, "custom", &second).equivalent(&owned));
        assert!(!TermCacheKeyRef::new(TermRole::Type, "custom", &first).equivalent(&owned));
    }
}
