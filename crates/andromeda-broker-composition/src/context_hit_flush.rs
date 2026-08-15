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

/// Retry the final drain until no pending delta remains.
async fn flush_until_durable(counter: &HitCounter, store: &dyn ContextHitStore, interval: Duration) {
    loop {
        let pending = counter.drain();
        if pending.is_empty() {
            return;
        }
        match store.flush_hits(&pending).await {
            Ok(()) => {}
            Err(error) => {
                restore(counter, pending);
                tracing::warn!(error = %error, event = "context_hit_final_flush_failed");
                sleep(interval).await;
            }
        }
    }
}

/// Restore deltas as one operation so concurrent increments remain additive.
fn restore(counter: &HitCounter, pending: Vec<HitDelta>) {
    counter.requeue(pending);
}
