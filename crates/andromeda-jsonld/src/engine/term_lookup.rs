//! Expanding a single term, and the cache that makes it cheap.
//!
//! A `q` filter, an `orderBy` clause and an `attrs` list all name terms one at
//! a time, and expanding one means handing the processor a synthetic document
//! built for the purpose. Doing that per term per request is what the cache
//! here removes: the answer depends only on the term and the resolved
//! `@context`, so both go into the key.

use crate::{
    context_spec::ContextSpec,
    document_context::resolve_context_value,
    engine::{context_url::is_full_iri, processor::JsonLdEngine},
    error::JsonLdError,
};
use moka::future::Cache;
use serde_json::Value;
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

/// How often the cache reports its hit rate.
const STATS_INTERVAL: u64 = 1000;

/// Expanded terms, keyed by term and resolved context.
#[derive(Clone, Debug)]
pub(crate) struct TermCache {
    /// The cached expansions.
    expansions: Cache<String, String>,
    /// How many lookups were answered from the cache.
    hits: Arc<AtomicU64>,
    /// How many lookups had to expand.
    misses: Arc<AtomicU64>,
}

impl TermCache {
    /// A cache bounded by entry count and entry age.
    pub(crate) fn new(capacity: u64, ttl_seconds: u64) -> TermCache {
        TermCache {
            expansions: Cache::builder().max_capacity(capacity).time_to_live(Duration::from_secs(ttl_seconds)).build(),
            hits: Arc::new(AtomicU64::new(0)),
            misses: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The cached expansion of a key, recording the outcome either way.
    async fn get(&self, key: &str) -> Option<String> {
        let cached = self.expansions.get(key).await;
        let counter = if cached.is_some() { &self.hits } else { &self.misses };
        let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_multiple_of(STATS_INTERVAL) {
            self.log_stats();
        }
        cached
    }

    /// Record an expansion.
    async fn insert(&self, key: String, iri: String) {
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
        let hit_rate = (hits as f64 / total as f64) * 100.0;
        tracing::debug!(hits, misses, total, hit_rate = format_args!("{hit_rate:.2}%"), "JSON-LD term cache statistics");
    }
}

/// What kind of name is being expanded, which decides the synthetic document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TermRole {
    /// An entity type, expanded through the document's `@type`.
    Type,
    /// An attribute name, expanded as a member key.
    Attribute,
}

impl TermRole {
    /// The prefix that keeps the two roles' cache entries apart.
    const fn key_prefix(self) -> &'static str {
        match self {
            TermRole::Type => "type",
            TermRole::Attribute => "term",
        }
    }
}

impl JsonLdEngine {
    /// Expand an entity type term to its IRI.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::InvalidIri`] when the term does not expand, and
    /// whatever expansion returns when a context does not resolve.
    pub async fn expand_type(&self, type_term: &str, context: &ContextSpec) -> Result<String, JsonLdError> {
        if is_full_iri(type_term) {
            return Ok(type_term.to_string());
        }
        let key = cache_key(TermRole::Type, type_term, context);
        if let Some(cached) = self.term_cache().get(&key).await {
            return Ok(cached);
        }

        let document = with_resolved_context(serde_json::json!({ "id": "urn:ngsi-ld:_:temp", "type": type_term }), context);
        let expanded = self.expand(document, context).await?;
        let iri = expanded
            .get(0)
            .and_then(|node| node.get("@type"))
            .and_then(|types| types.get(0))
            .and_then(Value::as_str)
            .map(String::from)
            .ok_or_else(|| JsonLdError::invalid_iri(format!("Failed to expand type: {type_term}")))?;

        tracing::trace!("Expanded type '{}' to '{}'", type_term, iri);
        self.term_cache().insert(key, iri.clone()).await; // clone: the cache keeps a copy, the caller gets one
        Ok(iri)
    }

    /// Expand an attribute term to its IRI.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLdError::InvalidIri`] when the term does not expand, and
    /// whatever expansion returns when a context does not resolve.
    pub async fn expand_term(&self, term: &str, context: &ContextSpec) -> Result<String, JsonLdError> {
        if is_full_iri(term) {
            return Ok(term.to_string());
        }
        let key = cache_key(TermRole::Attribute, term, context);
        if let Some(cached) = self.term_cache().get(&key).await {
            return Ok(cached);
        }

        let document = with_resolved_context(serde_json::json!({ term: { "@id": "urn:dummy" } }), context);
        let expanded = self.expand(document, context).await?;
        let iri = expanded
            .get(0)
            .and_then(Value::as_object)
            .and_then(|node| node.keys().find(|key| *key != "@id" && *key != "@type").cloned()) // clone: the expansion is dropped below
            .ok_or_else(|| JsonLdError::invalid_iri(format!("Failed to expand term: {term}")))?;

        self.term_cache().insert(key, iri.clone()).await; // clone: the cache keeps a copy, the caller gets one
        Ok(iri)
    }
}

/// Attach the spec's resolved `@context` to a synthetic document.
fn with_resolved_context(mut document: Value, context: &ContextSpec) -> Value {
    if let Some(resolved) = resolve_context_value(None, context)
        && let Some(object) = document.as_object_mut()
    {
        object.insert("@context".to_string(), resolved);
    }
    document
}

/// The cache key for one term under one context.
///
/// The context is hashed rather than spelled out: a resolved `@context` can be
/// an inline object of arbitrary size, and the key is only ever compared for
/// equality.
fn cache_key(role: TermRole, term: &str, context: &ContextSpec) -> String {
    let context_hash = resolve_context_value(None, context).map_or(0u64, |resolved| {
        let mut hasher = DefaultHasher::new();
        resolved.to_string().hash(&mut hasher);
        hasher.finish()
    });
    format!("{}:{term}:{context_hash:x}", role.key_prefix())
}
