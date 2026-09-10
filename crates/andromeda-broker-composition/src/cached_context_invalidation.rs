//! Periodic invalidation of one tenant's cached JSON-LD `@contexts`.
//!
//! Clause 5.13.1 and clause 6.3.16 both require it in the same words:
//! "Implementations shall periodically invalidate the "Cached" @contexts."
//! Periodically means on a clock. Reclaiming lapsed rows as a side effect of
//! the next cache write would leave a broker that has stopped downloading new
//! `@contexts` never invalidating the ones it already holds, so the pass gets a
//! worker of its own beside the hit-counter flush.
//!
//! Serving is safe whatever this worker does: the resolver's read already
//! passes over a row whose expiration time has come. What the pass owns is
//! reclamation, and clause 5.13.3's listing telling a client what the broker
//! actually still holds.

use andromeda_context_registry::cached_context_invalidation::CachedContextInvalidation;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::watch,
    task::JoinHandle,
    time::{MissedTickBehavior, interval as tokio_interval},
};

/// How often a tenant's lapsed `"Cached"` Contexts are reclaimed.
///
/// Clause 5.13.1 requires the pass but fixes no cadence, and correctness does
/// not depend on one: an entry stops being served the instant it lapses,
/// whether or not the row has been reclaimed yet. Half a minute keeps the
/// listing close to the truth while costing one narrow `DELETE` over rows of a
/// single kind.
pub(crate) const CACHED_CONTEXT_INVALIDATION_INTERVAL: Duration = Duration::from_secs(30);

/// A tenant-owned worker running the clause 5.13.1 invalidation pass.
pub struct CachedContextInvalidationWorker {
    /// Signals the pass to stop; dropping it stops the worker too.
    shutdown: watch::Sender<()>,
    /// The spawned pass, awaited by [`CachedContextInvalidationWorker::shutdown`].
    handle: JoinHandle<()>,
}

impl CachedContextInvalidationWorker {
    /// Start invalidating lapsed cached Contexts every `interval`.
    pub fn spawn(store: Arc<dyn CachedContextInvalidation>, interval: Duration) -> CachedContextInvalidationWorker {
        let (shutdown, mut shutdown_rx) = watch::channel(());
        let handle = tokio::spawn(async move {
            let mut ticker = tokio_interval(interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
            ticker.tick().await;
            loop {
                tokio::select! {
                    _ = ticker.tick() => invalidate_once(store.as_ref()).await,
                    // No final pass on the way out, unlike the hit-counter
                    // flush: nothing is accumulated in memory here, and the
                    // rows this reclaims are already unservable.
                    _ = shutdown_rx.changed() => break,
                }
            }
        });
        CachedContextInvalidationWorker { shutdown, handle }
    }

    /// Stop the periodic pass and wait for the task to end.
    pub async fn shutdown(self) {
        let _receivers = self.shutdown.send(());
        if let Err(error) = self.handle.await {
            tracing::error!(error = %error, event = "cached_context_invalidation_worker_join_failed");
        } else {
            tracing::info!(event = "cached_context_invalidation_stopped");
        }
    }
}

/// Run one invalidation pass, reporting what it reclaimed.
///
/// A failed pass is logged and the next tick tries again: the rows it would
/// have reclaimed are already refused by every read, so a database that is
/// briefly unavailable costs storage rather than correctness.
async fn invalidate_once(store: &dyn CachedContextInvalidation) {
    match store.invalidate_expired().await {
        Ok(invalidated) if invalidated.is_empty() => {}
        Ok(invalidated) => tracing::debug!(reclaimed = invalidated.count(), event = "cached_contexts_invalidated"),
        Err(error) => tracing::warn!(error = %error, event = "cached_context_invalidation_failed"),
    }
}

#[cfg(test)]
mod tests {
    use crate::cached_context_invalidation::{CACHED_CONTEXT_INVALIDATION_INTERVAL, CachedContextInvalidationWorker};
    use andromeda_context_registry::{
        cached_context_invalidation::{CachedContextInvalidation, CachedContextInvalidationFuture, InvalidatedContexts},
        error::ContextRegistryError,
    };
    use std::{
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };
    use tokio::time::{sleep, timeout};

    /// A store that counts the passes made against it and can be told to fail.
    struct CountingInvalidation {
        /// How many passes have reached the store.
        passes: AtomicU64,
        /// What each pass reports.
        outcome: PassOutcome,
    }

    /// What the fixture's invalidation pass reports back.
    #[derive(Clone, Copy)]
    enum PassOutcome {
        /// The pass reclaimed one Context.
        Reclaimed,
        /// The pass could not reach the store.
        Unavailable,
    }

    impl CachedContextInvalidation for CountingInvalidation {
        fn invalidate_expired(&self) -> CachedContextInvalidationFuture<'_> {
            self.passes.fetch_add(1, Ordering::Relaxed);
            let outcome = self.outcome;
            Box::pin(async move {
                match outcome {
                    PassOutcome::Reclaimed => Ok(InvalidatedContexts::reclaimed(1)),
                    PassOutcome::Unavailable => Err(ContextRegistryError::Persistence {
                        detail: "connection closed".to_owned(),
                    }),
                }
            })
        }
    }

    /// Clause 5.13.1: "Implementations shall periodically invalidate the
    /// "Cached" @contexts." The pass runs on its own clock, so a broker that
    /// never writes another cache row still invalidates the rows it holds.
    #[tokio::test]
    async fn the_pass_runs_on_its_own_clock_without_any_cache_write() {
        let store = Arc::new(CountingInvalidation {
            passes: AtomicU64::new(0),
            outcome: PassOutcome::Reclaimed,
        });
        // clone: the worker owns one handle while the assertion reads the other.
        let worker = CachedContextInvalidationWorker::spawn(Arc::clone(&store) as Arc<dyn CachedContextInvalidation>, Duration::from_millis(20));

        sleep(Duration::from_millis(150)).await;
        worker.shutdown().await;

        assert!(
            store.passes.load(Ordering::Relaxed) >= 2,
            "the pass must recur on its interval, not once: {} passes",
            store.passes.load(Ordering::Relaxed)
        );
    }

    /// A pass that cannot reach the store must not stop the worker: the rows it
    /// would have reclaimed are already refused by every read, so the next tick
    /// simply tries again.
    #[tokio::test]
    async fn a_failing_pass_does_not_stop_the_worker() {
        let store = Arc::new(CountingInvalidation {
            passes: AtomicU64::new(0),
            outcome: PassOutcome::Unavailable,
        });
        // clone: the worker owns one handle while the assertion reads the other.
        let worker = CachedContextInvalidationWorker::spawn(Arc::clone(&store) as Arc<dyn CachedContextInvalidation>, Duration::from_millis(20));

        sleep(Duration::from_millis(150)).await;
        worker.shutdown().await;

        assert!(store.passes.load(Ordering::Relaxed) >= 2, "a failed pass must be retried on the next tick");
    }

    /// Shutdown ends the worker rather than waiting for another tick, so a long
    /// invalidation cadence never delays a tenant's teardown.
    #[tokio::test]
    async fn shutdown_ends_the_worker_without_waiting_for_the_next_tick() {
        let store = Arc::new(CountingInvalidation {
            passes: AtomicU64::new(0),
            outcome: PassOutcome::Reclaimed,
        });
        let worker = CachedContextInvalidationWorker::spawn(store as Arc<dyn CachedContextInvalidation>, CACHED_CONTEXT_INVALIDATION_INTERVAL);

        timeout(Duration::from_secs(2), worker.shutdown())
            .await
            .expect("shutdown must not wait out the invalidation interval");
    }
}
