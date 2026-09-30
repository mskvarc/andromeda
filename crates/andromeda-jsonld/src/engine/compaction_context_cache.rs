//! Processed compaction contexts kept across operations.
//!
//! Most requests name no context of their own (clause 5.5.7), so they compact
//! against the core context alone, and processing it again for every request
//! would repeat the same work. Only a context whose every document is embedded
//! in the broker is kept here: an embedded core context cannot change, while a
//! context fetched from a URL carries the lifetime its origin granted
//! (clause 6.3.16), which the loader enforces each time it resolves it. A
//! processed copy held here would outlive that lifetime, so such a context is
//! processed per operation instead.

use crate::{
    context_spec::ContextSpec,
    core_context::is_ngsi_ld_core_context_url,
    engine::compaction_context::{CompactionContext, CompactionContextError},
    loader::http::HttpContextLoader,
};
use moka::future::Cache;
use std::{sync::Arc, time::Duration};

/// How many distinct processed contexts are kept.
///
/// Only combinations of the released core contexts qualify, so the working
/// set is a handful of entries.
const CAPACITY: u64 = 16;

/// How long one processed context is kept.
///
/// The active context memoizes every term selection made against it, so a
/// context kept forever would accumulate one entry per attribute name ever
/// compacted. Dropping it periodically bounds that at the price of processing
/// the core context again once per period.
const TIME_TO_LIVE: Duration = Duration::from_secs(600);

/// Processed contexts built only from documents embedded in the broker, keyed
/// by the resolved `@context` they were processed from.
#[derive(Clone, Debug)]
pub(crate) struct CompactionContextCache {
    /// The processed contexts.
    contexts: Cache<Arc<str>, Arc<CompactionContext>>,
}

impl CompactionContextCache {
    /// An empty cache.
    pub(crate) fn new() -> CompactionContextCache {
        CompactionContextCache {
            contexts: Cache::builder().max_capacity(CAPACITY).time_to_live(TIME_TO_LIVE).build(),
        }
    }

    /// The processed context a spec selects, from the cache when the spec can
    /// be kept, and processed afresh otherwise.
    ///
    /// Two operations that miss together each process the context and one
    /// insert wins; both results are identical.
    ///
    /// # Errors
    ///
    /// Returns whatever [`CompactionContext::prepare`] returns.
    pub(crate) async fn get_or_prepare(&self, spec: &ContextSpec, loader: &HttpContextLoader) -> Result<Arc<CompactionContext>, CompactionContextError> {
        let Some(key) = cache_key(spec) else {
            return CompactionContext::prepare(spec, loader).await.map(Arc::new);
        };
        if let Some(cached) = self.contexts.get(&key).await {
            return Ok(cached);
        }
        let prepared = Arc::new(CompactionContext::prepare(spec, loader).await?);
        self.contexts.insert(key, Arc::clone(&prepared)).await; // clone: Arc ref-count bump
        Ok(prepared)
    }

    /// How many processed contexts are held.
    #[cfg(test)]
    pub(crate) async fn len(&self) -> u64 {
        self.contexts.run_pending_tasks().await;
        self.contexts.entry_count()
    }
}

/// The key a spec is kept under, or `None` when it must not be kept.
///
/// A spec qualifies when it carries no body `@context` and every `Link` context
/// it names is a released core context, all of which are embedded. The key is
/// the serialized `@context` the spec resolves to, which is exactly what gets
/// processed, so two specs share an entry only when they would process the same
/// value.
fn cache_key(spec: &ContextSpec) -> Option<Arc<str>> {
    let embedded_only = spec.body_context().is_none() && spec.link_context().is_none_or(|url| is_ngsi_ld_core_context_url(url.as_ref()));
    if !embedded_only {
        return None;
    }
    spec.to_context_value().map(|value| Arc::from(value.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::{context_spec::ContextSpec, core_context::NGSI_LD_CORE_CONTEXT_V16_URL, engine::compaction_context_cache::cache_key};
    use iri_rs::IriBuf;
    use serde_json::json;

    fn iri_ref(url: &str) -> IriBuf {
        IriBuf::new(url.to_owned()).expect("a valid IRI")
    }

    #[test]
    fn a_spec_naming_no_context_is_kept() {
        assert!(cache_key(&ContextSpec::none()).is_some());
        assert!(cache_key(&ContextSpec::core_only()).is_some());
    }

    #[test]
    fn a_spec_naming_only_released_core_contexts_is_kept() {
        assert!(cache_key(&ContextSpec::from_link(iri_ref(NGSI_LD_CORE_CONTEXT_V16_URL.as_str()))).is_some());
    }

    #[test]
    fn a_spec_naming_a_remote_context_is_not_kept() {
        assert!(cache_key(&ContextSpec::from_link(iri_ref("https://example.com/context.jsonld"))).is_none());
    }

    #[test]
    fn a_spec_carrying_a_body_context_is_not_kept() {
        let spec = ContextSpec::from_body(json!({ "speed": "https://example.com/speed" }));
        assert!(cache_key(&spec).is_none());
    }

    #[test]
    fn specs_resolving_to_the_same_context_share_one_key() {
        let versioned = ContextSpec::from_link(iri_ref(NGSI_LD_CORE_CONTEXT_V16_URL.as_str()));
        pretty_assertions::assert_eq!(cache_key(&versioned), cache_key(&ContextSpec::none()));
    }
}
