//! Durable flushing for one tenant's JSON-LD context hit counter.

use andromeda_context_registry::hit_store::ContextHitStore;
use andromeda_jsonld::hit_counter::{HitCounter, HitDelta};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, interval as tokio_interval, sleep},
};

/// A tenant-owned flush worker with an orderly final-drain protocol.
pub struct ContextHitFlushWorker {
    shutdown: watch::Sender<()>,
    handle: JoinHandle<()>,
}

impl ContextHitFlushWorker {
    /// Start periodically transferring accumulated hits into the durable store.
    pub fn spawn(counter: Arc<HitCounter>, store: Arc<dyn ContextHitStore>, interval: Duration) -> ContextHitFlushWorker {
        let (shutdown, mut shutdown_rx) = watch::channel(());
        let handle = tokio::spawn(async move {
            let mut ticker = tokio_interval(interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = ticker.tick() => flush_once(&counter, store.as_ref()).await,
                    changed = shutdown_rx.changed() => {
                        if changed.is_ok() {
                            flush_until_durable(&counter, store.as_ref(), interval).await;
                        }
                        break;
                    }
                }
            }
        });
        ContextHitFlushWorker { shutdown, handle }
    }

    /// Stop periodic work and wait until the final durable flush succeeds.
    pub async fn shutdown(self) {
        let _receivers = self.shutdown.send(());
        if let Err(error) = self.handle.await {
            tracing::error!(error = %error, event = "context_hit_flush_worker_join_failed");
        } else {
            tracing::info!(event = "context_hit_final_flush_complete");
        }
    }
}

/// Drain once, atomically restoring the entire delta set on failure.
async fn flush_once(counter: &HitCounter, store: &dyn ContextHitStore) {
    let _readers_held_off = counter.flush_exclusively().await;
    let pending = counter.drain();
    if pending.is_empty() {
        return;
    }
    if let Err(error) = store.flush_hits(&pending).await {
        let count = pending.len();
        counter.requeue(pending);
        tracing::warn!(error = %error, count, event = "context_hit_flush_failed");
    }
}

/// How many times the final drain may fail before the tenant stops waiting.
///
/// Shutdown has to finish. An unreachable database would otherwise hold the
/// awaited teardown open forever, and clause 5.13.3.5 makes `numberOfHits`
/// optional — losing a bounded number of access counts is a smaller failure
/// than a broker that never exits.
const FINAL_FLUSH_ATTEMPTS: u32 = 3;

/// Drain the counter on the way out, giving up after `FINAL_FLUSH_ATTEMPTS`
/// consecutive failures.
///
/// A successful pass does not restore the budget: a store that keeps accepting
/// writes empties the counter, and one that does not must not be retried
/// indefinitely just because an earlier pass landed.
async fn flush_until_durable(counter: &HitCounter, store: &dyn ContextHitStore, interval: Duration) {
    let mut failures = 0_u32;
    while failures < FINAL_FLUSH_ATTEMPTS {
        let readers_held_off = counter.flush_exclusively().await;
        let pending = counter.drain();
        if pending.is_empty() {
            return;
        }
        let count = pending.len();
        if let Err(error) = store.flush_hits(&pending).await {
            restore(counter, pending);
            // The retry waits with readers let back in.
            drop(readers_held_off);
            failures += 1;
            tracing::warn!(error = %error, count, attempt = failures, limit = FINAL_FLUSH_ATTEMPTS, event = "context_hit_final_flush_failed");
            if failures < FINAL_FLUSH_ATTEMPTS {
                sleep(interval).await;
            }
        }
    }
    tracing::error!(
        limit = FINAL_FLUSH_ATTEMPTS,
        event = "context_hit_final_flush_abandoned",
        "shutdown continues without the remaining clause 5.13.3.5 access counts"
    );
}

/// Restore deltas as one operation so concurrent increments remain additive.
fn restore(counter: &HitCounter, pending: Vec<HitDelta>) {
    counter.requeue(pending);
}

#[cfg(test)]
mod tests {
    use crate::context_hit_flush::{FINAL_FLUSH_ATTEMPTS, flush_until_durable};
    use andromeda_context_registry::{
        error::ContextRegistryError,
        hit_store::{ContextHitStore, ContextHitStoreFuture},
    };
    use andromeda_jsonld::hit_counter::{HitCounter, HitDelta};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use iri_rs::IriBuf;
    use std::{
        sync::atomic::{AtomicU32, Ordering},
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

    /// An unreachable database must not hold a tenant's teardown open: the
    /// final drain gives up after a bounded number of failures and shutdown
    /// continues without the optional clause 5.13.3.5 counts.
    #[tokio::test]
    async fn the_final_drain_gives_up_rather_than_wedging_shutdown() {
        let counter = HitCounter::new();
        counter.bump(&IriBuf::new("https://example.com/context.jsonld".to_owned()).expect("valid URL"));
        let store = UnreachableStore { attempts: AtomicU32::new(0) };

        timeout(Duration::from_secs(5), flush_until_durable(&counter, &store, Duration::from_millis(1)))
            .await
            .expect("the final drain must terminate against an unreachable store");

        pretty_assertions::assert_eq!(store.attempts.load(Ordering::Relaxed), FINAL_FLUSH_ATTEMPTS);
    }

    /// The deltas the abandoned drain could not write stay in the counter
    /// rather than being dropped on the floor, so nothing is silently lost
    /// while the process is still running.
    #[tokio::test]
    async fn deltas_the_drain_abandoned_are_left_in_the_counter() {
        let counter = HitCounter::new();
        counter.bump(&IriBuf::new("https://example.com/context.jsonld".to_owned()).expect("valid URL"));
        let store = UnreachableStore { attempts: AtomicU32::new(0) };

        flush_until_durable(&counter, &store, Duration::from_millis(1)).await;

        pretty_assertions::assert_eq!(counter.drain().len(), 1);
    }
}
