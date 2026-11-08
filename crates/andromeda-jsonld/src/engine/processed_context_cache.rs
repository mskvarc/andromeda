//! Processed request contexts kept across operations.
//!
//! Processing a `@context` is the expensive half of both expansion and
//! compaction, and most requests of a deployment name one of a handful of
//! contexts, so processing it again for every request repeats the same work.
//! A processed context is a function of the documents its references resolved
//! to, and those carry the lifetime their origin granted (clause 6.3.16),
//! which the loader enforces each time it resolves them. A kept context
//! therefore remembers the documents it was built from
//! ([`ContextSources`]) and is reused only while every reference still
//! resolves to the very same document; otherwise it is processed again.

use crate::loader::{
    context_sources::{ContextSources, recording_sources},
    http::HttpContextLoader,
};
use moka::future::Cache;
use std::{future::Future, sync::Arc, time::Duration};

/// How many distinct processed contexts one engine keeps.
const CAPACITY: u64 = 64;

/// How long one processed context is kept.
///
/// An active context memoizes every term decided against it, so a context kept
/// forever would accumulate one entry per name ever expanded or compacted.
/// Dropping it periodically bounds that at the price of processing it again
/// once per period.
const TIME_TO_LIVE: Duration = Duration::from_secs(600);

/// Processed contexts of one kind, keyed by the resolved `@context` they were
/// processed from.
#[derive(derive_more::Debug)]
pub(crate) struct ProcessedContextCache<T> {
    /// The processed contexts with the documents each was built from.
    #[debug(skip)]
    entries: Cache<Arc<str>, Arc<HeldContext<T>>>,
}

/// One processed context and the documents it was built from.
struct HeldContext<T> {
    /// The processed context.
    processed: Arc<T>,
    /// The documents the loader resolved while it was processed.
    sources: ContextSources,
}

impl<T> ProcessedContextCache<T>
where
    T: Send + Sync + 'static,
{
    /// An empty cache.
    pub(crate) fn new() -> ProcessedContextCache<T> {
        ProcessedContextCache {
            entries: Cache::builder().max_capacity(CAPACITY).time_to_live(TIME_TO_LIVE).build(),
        }
    }

    /// The context kept under `key` while every document it was built from
    /// still resolves through `loader`, or the result of `prepare`, kept under
    /// `key` when it succeeds.
    ///
    /// Two operations that miss together each prepare the context and the
    /// later insert wins; both results are identical.
    ///
    /// # Errors
    ///
    /// Returns whatever the preparation returns.
    pub(crate) async fn get_or_prepare<E, P, F>(&self, key: Arc<str>, loader: &HttpContextLoader, prepare: P) -> Result<Arc<T>, E>
    where
        P: FnOnce() -> F,
        F: Future<Output = Result<T, E>>,
    {
        if let Some(held) = self.entries.get(&key).await
            && held.sources.still_resolve_through(loader).await
        {
            return Ok(Arc::clone(&held.processed));
        }
        // Processing a context is a deep future; kept on the heap, it does not
        // inflate the future of every caller that finds its context kept.
        let (prepared, sources) = Box::pin(recording_sources(prepare())).await;
        let processed = Arc::new(prepared?);
        let held = HeldContext {
            processed: Arc::clone(&processed),
            sources,
        };
        self.entries.insert(key, Arc::new(held)).await;
        Ok(processed)
    }

    /// How many processed contexts are held.
    #[cfg(test)]
    pub(crate) async fn len(&self) -> u64 {
        self.entries.run_pending_tasks().await;
        self.entries.entry_count()
    }
}
