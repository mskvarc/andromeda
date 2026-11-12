//! Durable flushing of one tenant's JSON-LD context hit counter.
//!
//! A supervised scheduled task moves the accumulated hits into the store every
//! flush interval while the tenant is loaded. The tenant's teardown drains what
//! is left once nothing can count a hit any more, before the tenant's pool
//! closes.

use andromeda_context_registry::{error::ContextRegistryError, hit_store::ContextHitStore};
use andromeda_jsonld::hit_counter::HitCounter;
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;

/// How many times the final drain may fail before the tenant stops waiting.
///
/// Shutdown has to finish. An unreachable database would otherwise hold the
/// awaited teardown open forever, and clause 5.13.3.5 makes `numberOfHits`
/// optional, so losing a bounded number of access counts is a smaller failure
/// than a broker that never exits.
const FINAL_FLUSH_ATTEMPTS: u32 = 3;

/// One tenant's hit counter and the store its hits are flushed to.
pub(crate) struct ContextHitFlush {
    /// The hits counted since the last flush that landed.
    counter: Arc<HitCounter>,
    /// Where the hits are added to each stored `@context`'s count.
    store: Arc<dyn ContextHitStore>,
    /// How often the hits are flushed, and how long a failed final drain waits
    /// before it tries again.
    interval: Duration,
}

impl ContextHitFlush {
    /// Flush the hits `counter` accumulates into `store` every `interval`.
    pub(crate) fn new(counter: Arc<HitCounter>, store: Arc<dyn ContextHitStore>, interval: Duration) -> ContextHitFlush {
        ContextHitFlush { counter, store, interval }
    }

    /// How often the hits are flushed.
    pub(crate) const fn interval(&self) -> Duration {
        self.interval
    }

    /// Move the accumulated hits into the store, restoring every one of them to
    /// the counter when the store refuses, so concurrent increments stay
    /// additive.
    ///
    /// # Errors
    ///
    /// Returns the store's [`ContextRegistryError`] when it refuses the hits.
    pub(crate) async fn flush(&self) -> Result<(), ContextRegistryError> {
        let _readers_held_off = self.counter.flush_exclusively().await;
        let pending = self.counter.drain();
        if pending.is_empty() {
            return Ok(());
        }
        let refused = self.store.flush_hits(&pending).await;
        if let Err(error) = &refused {
            let count = pending.len();
            self.counter.requeue(pending);
            tracing::warn!(error = %error, count, event = "context_hit_flush_failed");
        }
        refused
    }

    /// Drain the counter on the way out, giving up after
    /// [`FINAL_FLUSH_ATTEMPTS`] consecutive failures.
    ///
    /// A successful pass does not restore the budget: a store that keeps
    /// accepting writes empties the counter, and one that does not must not be
    /// retried indefinitely just because an earlier pass landed.
    pub(crate) async fn drain(&self) {
        let mut failures = 0_u32;
        while failures < FINAL_FLUSH_ATTEMPTS {
            let readers_held_off = self.counter.flush_exclusively().await;
            let pending = self.counter.drain();
            if pending.is_empty() {
                tracing::info!(event = "context_hit_final_flush_complete");
                return;
            }
            let count = pending.len();
            if let Err(error) = self.store.flush_hits(&pending).await {
                self.counter.requeue(pending);
                // The retry waits with readers let back in.
                drop(readers_held_off);
                failures += 1;
                tracing::warn!(error = %error, count, attempt = failures, limit = FINAL_FLUSH_ATTEMPTS, event = "context_hit_final_flush_failed");
                if failures < FINAL_FLUSH_ATTEMPTS {
                    sleep(self.interval).await;
                }
            }
        }
        tracing::error!(
            limit = FINAL_FLUSH_ATTEMPTS,
            event = "context_hit_final_flush_abandoned",
            "shutdown continues without the remaining clause 5.13.3.5 access counts"
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::context_hit_flush::{ContextHitFlush, FINAL_FLUSH_ATTEMPTS};
    use andromeda_context_registry::{
        error::ContextRegistryError,
        hit_store::{ContextHitStore, ContextHitStoreFuture},
    };
    use andromeda_jsonld::hit_counter::{HitCounter, HitDelta};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use iri_rs::IriBuf;
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU32, Ordering},
        },
        time::Duration,
    };
    use tokio::time::timeout;

    /// A store that never accepts a write, counting the attempts made.
    struct UnreachableStore {
        /// How many flush attempts reached the store.
        attempts: AtomicU32,
    }

    impl ContextHitStore for UnreachableStore {
        fn flush_hits<'a>(&'a self, _deltas: &'a [HitDelta]) -> ContextHitStoreFuture<'a> {
            self.attempts.fetch_add(1, Ordering::Relaxed);
            Box::pin(async { Err(ContextRegistryError::Persistence(PersistenceFailure::Unavailable)) })
        }
    }

    /// A flush of one counted hit into a store that refuses every write, with
    /// the counter and the store kept for inspection.
    fn unreachable_flush() -> (ContextHitFlush, Arc<HitCounter>, Arc<UnreachableStore>) {
        let counter = Arc::new(HitCounter::new());
        counter.bump(&IriBuf::new("https://example.com/context.jsonld".to_owned()).expect("valid URL"));
        let store = Arc::new(UnreachableStore { attempts: AtomicU32::new(0) });
        let flush = ContextHitFlush::new(Arc::clone(&counter), Arc::clone(&store) as Arc<dyn ContextHitStore>, Duration::from_millis(1));
        (flush, counter, store)
    }

    #[tokio::test]
    async fn a_flush_the_store_refuses_reports_the_refusal_and_keeps_every_hit() {
        let (flush, counter, store) = unreachable_flush();

        let flushed = flush.flush().await;

        assert!(
            matches!(flushed, Err(ContextRegistryError::Persistence(PersistenceFailure::Unavailable))),
            "{flushed:?}"
        );
        pretty_assertions::assert_eq!(store.attempts.load(Ordering::Relaxed), 1);
        pretty_assertions::assert_eq!(counter.drain().len(), 1);
    }

    /// An unreachable database must not hold a tenant's teardown open: the
    /// final drain gives up after a bounded number of failures and shutdown
    /// continues without the optional clause 5.13.3.5 counts.
    #[tokio::test]
    async fn the_final_drain_gives_up_rather_than_wedging_shutdown() {
        let (flush, _counter, store) = unreachable_flush();

        timeout(Duration::from_secs(5), flush.drain())
            .await
            .expect("the final drain must terminate against an unreachable store");

        pretty_assertions::assert_eq!(store.attempts.load(Ordering::Relaxed), FINAL_FLUSH_ATTEMPTS);
    }

    /// The deltas the abandoned drain could not write stay in the counter
    /// rather than being dropped on the floor, so nothing is silently lost
    /// while the process is still running.
    #[tokio::test]
    async fn deltas_the_drain_abandoned_are_left_in_the_counter() {
        let (flush, counter, _store) = unreachable_flush();

        flush.drain().await;

        pretty_assertions::assert_eq!(counter.drain().len(), 1);
    }
}
