//! Expanding a single term, and the cache that makes it cheap.
//!
//! A `q` filter, an `orderBy` clause and an `attrs` list all name terms one at
//! a time. Each is expanded by the IRI Expansion algorithm over the active
//! context the operation's `@context` processed into (as a member key for an
//! attribute, or as an `@type` value for an entity type, JSON-LD 1.1 API
//! Expansion Algorithm steps 13.2 and 13.4.4.4), never by expanding a
//! document. The cache here keeps each answer, which depends only on the term
//! and the processed context it was decided against, so both go into the key.
//! The engine processes a context again once a document it was built from
//! changes, and that processing takes a new identity, so an answer is never
//! reused for the changed context.

use crate::{
    context_spec::ContextSpec,
    engine::{context_url::is_full_iri, expansion_context::ExpansionContext, expansion_context_id::ExpansionContextId, processor::JsonLdEngine},
    error::JsonLdError,
    loader::cache_lifetime::CacheLifetime,
};
use iri_rs::IriBuf;
use jsonld::{
    Id,
    Term,
    ValidId,
    context_processing::algorithm::{Action, expand_property_key, expand_type_value},
};
use moka::{Equivalent, future::Cache};
use rdfx::vocabulary::no_vocabulary_mut;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::OnceCell;

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

/// What kind of name is being expanded, which decides how it is expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TermRole {
    /// An entity type, expanded as an `@type` value.
    Type,
    /// An attribute name, expanded as a member key.
    Attribute,
}

/// A context prepared once for all term lookups in one operation.
///
/// The first lookup that needs the context resolves it, once per operation,
/// through the engine's own cache of processed contexts, which reuses a
/// processed context only while the documents it was built from are still the
/// ones the loader resolves. Cache entries are keyed by the identity of that
/// processed context.
#[derive(Debug, Clone)]
pub struct PreparedTermContext {
    /// The context the terms are expanded under.
    spec: ContextSpec,
    /// The processed context, once a lookup needed it.
    expansion: OnceCell<Arc<ExpansionContext>>,
    /// Counts preparation work in unit tests without affecting release builds.
    #[cfg(test)]
    preparation_count: Arc<AtomicU64>,
}

impl PreparedTermContext {
    /// Resolve a request context once for the operation that will expand terms.
    fn from_spec(spec: &ContextSpec) -> PreparedTermContext {
        PreparedTermContext {
            spec: spec.clone(), // clone: the prepared context outlives the borrowed spec of the operation
            expansion: OnceCell::new(),
            #[cfg(test)]
            preparation_count: Arc::new(AtomicU64::new(1)),
        }
    }

    /// The processed context, processing it on first use.
    ///
    /// # Errors
    ///
    /// Returns the failure of a context that does not resolve or does not
    /// process.
    pub async fn expansion(&self, engine: &JsonLdEngine) -> Result<&Arc<ExpansionContext>, JsonLdError> {
        self.expansion.get_or_try_init(|| engine.expansion_context(&self.spec)).await
    }

    #[cfg(test)]
    /// How many times this value resolved its context.
    fn preparation_count(&self) -> u64 {
        self.preparation_count.load(Ordering::Relaxed)
    }
}

/// One expanded-term cache entry's key.
///
/// The role keeps a type and an attribute of the same spelling apart, the term
/// is the raw name before expansion, and the context is the processed context
/// the term was decided against.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TermCacheKey {
    /// Whether an entity type or an attribute name is being expanded.
    role: TermRole,
    /// The raw term as written, before expansion: a lookup token, not yet an IRI.
    term: Box<str>,
    /// The processed context the term is expanded against.
    context: ExpansionContextId,
}

/// A [`TermCacheKey`] borrowed from the lookup that probes for it.
///
/// A hit allocates nothing: the owned key is built only when a miss records
/// its expansion. The fields mirror [`TermCacheKey`] in the same order and
/// derive the same hash, because a `Box<str>` hashes exactly as the `str` it
/// holds.
#[derive(Debug, Clone, Copy, Hash)]
struct TermCacheKeyRef<'a> {
    /// Whether an entity type or an attribute name is being expanded.
    role: TermRole,
    /// The raw term as written, before expansion.
    term: &'a str,
    /// The processed context the term is expanded against.
    context: ExpansionContextId,
}

impl<'a> TermCacheKeyRef<'a> {
    /// The key one term takes under one processed context.
    const fn new(role: TermRole, term: &'a str, context: ExpansionContextId) -> TermCacheKeyRef<'a> {
        TermCacheKeyRef { role, term, context }
    }

    /// The owned key a miss records its expansion under.
    fn to_owned_key(self) -> TermCacheKey {
        TermCacheKey {
            role: self.role,
            term: Box::from(self.term),
            context: self.context,
        }
    }
}

// Not derivable: equivalence between a borrowed and an owned key is a
// relation across two types, which no derive expresses.
impl Equivalent<TermCacheKey> for TermCacheKeyRef<'_> {
    fn equivalent(&self, key: &TermCacheKey) -> bool {
        self.role == key.role && self.term == &*key.term && self.context == key.context
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
        let expansion = context.expansion(self).await?;
        let key = TermCacheKeyRef::new(TermRole::Type, type_term, expansion.id());
        if let Some(cached) = self.term_cache().get(&key).await {
            return Ok(cached);
        }

        // `Keep` never rejects, so the rejection is never reached.
        let term = expand_type_value(no_vocabulary_mut(), expansion.active(), type_term, Action::Keep).map_err(|_rejected| JsonLdError::TypeNotExpanded)?;
        let iri = match term.as_deref() {
            Some(Term::Id(Id::Valid(ValidId::Iri(iri)))) => iri.clone(), // clone: the processor's memoized term stays shared
            Some(Term::Id(id)) => iri_of(id.as_str())?,
            Some(Term::Null | Term::Keyword(_)) | None => return Err(JsonLdError::TypeNotExpanded),
        };

        tracing::trace!("Expanded type '{}' to '{}'", type_term, iri);
        self.term_cache().insert(key.to_owned_key(), iri.clone()).await; // clone: the cache keeps a copy, the caller gets one
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
        let expansion = context.expansion(self).await?;
        let key = TermCacheKeyRef::new(TermRole::Attribute, term, expansion.id());
        if let Some(cached) = self.term_cache().get(&key).await {
            return Ok(cached);
        }

        // A key that expands to anything but an IRI is dropped by expansion
        // (Expansion Algorithm step 13.3), so it names no attribute.
        let expanded = expand_property_key(no_vocabulary_mut(), expansion.active(), term, Action::Keep).map_err(|_rejected| JsonLdError::TermNotExpanded)?;
        let Some(Term::Id(Id::Valid(ValidId::Iri(iri)))) = expanded.as_deref() else {
            return Err(JsonLdError::TermNotExpanded);
        };
        let iri = iri.clone(); // clone: the processor's memoized term stays shared

        self.term_cache().insert(key.to_owned_key(), iri.clone()).await; // clone: the cache keeps a copy, the caller gets one
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
        context_cache::ContextCache,
        context_spec::ContextSpec,
        engine::{
            expansion_context_id::ExpansionContextId,
            processor::{JsonLdEngine, document_expansions},
            term_lookup::{TermCacheKeyRef, TermRole},
        },
        error::JsonLdError,
        loader::replaceable_row::ReplaceableRow,
        test_documents::literal,
    };
    use iri_rs::IriBuf;
    use moka::Equivalent;
    use serde_json::json;
    use std::{
        hash::{BuildHasher, RandomState},
        sync::Arc,
    };

    #[tokio::test]
    async fn repeated_lookups_reuse_one_prepared_context() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = engine.prepare_term_context(&ContextSpec::none());

        let first = engine.expand_term("speed", &context).await.expect("first lookup expands");
        let second = engine.expand_term("speed", &context).await.expect("second lookup expands");

        pretty_assertions::assert_eq!(first, second);
        assert_eq!(context.preparation_count(), 1);
    }

    /// A term is expanded by the IRI Expansion algorithm over the processed
    /// context, as a member key or an `@type` value: no document is handed to
    /// the processor for it.
    #[tokio::test]
    async fn a_term_lookup_does_not_expand_a_document() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = engine.prepare_term_context(&ContextSpec::from_body(literal(&json!({
            "Vehicle": "https://example.com/Vehicle",
            "speed": "https://example.com/speed"
        }))));
        let before = document_expansions();

        let vehicle = engine.expand_type("Vehicle", &context).await.expect("the type expands");
        let speed = engine.expand_term("speed", &context).await.expect("the term expands");

        pretty_assertions::assert_eq!(vehicle.as_str(), "https://example.com/Vehicle");
        pretty_assertions::assert_eq!(speed.as_str(), "https://example.com/speed");
        pretty_assertions::assert_eq!(document_expansions(), before);
    }

    /// A key that expands to a keyword names no attribute, and a type the
    /// context cannot expand to an absolute IRI is no entity type IRI.
    #[tokio::test]
    async fn a_keyword_is_no_attribute_and_a_blank_type_is_no_iri() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let context = engine.prepare_term_context(&ContextSpec::none());

        let keyword = engine.expand_term("id", &context).await;
        let blank = engine.expand_type("_:b0", &context).await;

        assert!(matches!(keyword, Err(JsonLdError::TermNotExpanded)), "{keyword:?}");
        assert!(matches!(blank, Err(JsonLdError::ExpandedTermNotIri(_))), "{blank:?}");
    }

    #[tokio::test]
    async fn distinct_prepared_contexts_cannot_share_term_cache_entries() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let first_context = ContextSpec::from_body(literal(&json!({ "custom": "https://example.com/first/custom" })));
        let second_context = ContextSpec::from_body(literal(&json!({ "custom": "https://example.com/second/custom" })));
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
        let borrowed = TermCacheKeyRef::new(TermRole::Attribute, "speed", ExpansionContextId::fresh());
        let owned = borrowed.to_owned_key();
        let state = RandomState::new();

        pretty_assertions::assert_eq!(state.hash_one(borrowed), state.hash_one(&owned));
        assert!(borrowed.equivalent(&owned));
    }

    /// Equality, not the hash, decides a hit: a borrowed key under another
    /// processed context or for the other role is never equivalent.
    #[test]
    fn a_borrowed_key_under_another_context_or_role_is_not_equivalent() {
        let first = ExpansionContextId::fresh();
        let owned = TermCacheKeyRef::new(TermRole::Attribute, "custom", first).to_owned_key();

        assert!(!TermCacheKeyRef::new(TermRole::Attribute, "custom", ExpansionContextId::fresh()).equivalent(&owned));
        assert!(!TermCacheKeyRef::new(TermRole::Type, "custom", first).equivalent(&owned));
    }

    /// Where the reloaded context is read from.
    fn reloaded_url() -> IriBuf {
        IriBuf::new("https://example.org/reloaded.jsonld".to_owned()).expect("a valid IRI")
    }

    /// An engine resolving [`reloaded_url`] from `row`.
    fn engine_over(row: &Arc<ReplaceableRow>) -> JsonLdEngine {
        JsonLdEngine::builder()
            .allow_remote(false)
            .with_l2_cache(Arc::clone(row) as Arc<dyn ContextCache>)
            .build()
            .expect("engine builds")
    }

    /// Expand `term` in a new operation naming [`reloaded_url`].
    async fn expanded_in_a_new_operation(engine: &JsonLdEngine, term: &str) -> IriBuf {
        let context = engine.prepare_term_context(&ContextSpec::from_link(reloaded_url()));
        engine.expand_term(term, &context).await.expect("the term expands")
    }

    /// Clause 5.13.5.4 replaces a reloaded `@context` with the document
    /// downloaded then, so a term looked up again is decided by that document.
    #[tokio::test]
    async fn a_term_looked_up_after_its_context_is_reloaded_expands_by_the_new_document() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "speed": "https://example.org/speed" } })));
        let engine = engine_over(&row);
        pretty_assertions::assert_eq!(expanded_in_a_new_operation(&engine, "speed").await.as_str(), "https://example.org/speed");

        row.replace(&json!({ "@context": { "speed": "https://example.org/velocity" } }));
        engine.invalidate_context(&reloaded_url()).await;

        pretty_assertions::assert_eq!(expanded_in_a_new_operation(&engine, "speed").await.as_str(), "https://example.org/velocity");
    }

    #[tokio::test]
    async fn a_context_keeps_its_identity_while_its_documents_are_unchanged_and_takes_a_new_one_once_reloaded() {
        let row = Arc::new(ReplaceableRow::new(&json!({ "@context": { "speed": "https://example.org/speed" } })));
        let engine = engine_over(&row);
        let identity = async || {
            let context = engine.prepare_term_context(&ContextSpec::from_link(reloaded_url()));
            context.expansion(&engine).await.expect("the context processes").id()
        };
        let first = identity().await;

        pretty_assertions::assert_eq!(identity().await, first);

        row.replace(&json!({ "@context": { "speed": "https://example.org/velocity" } }));
        engine.invalidate_context(&reloaded_url()).await;

        assert_ne!(identity().await, first);
    }
}
