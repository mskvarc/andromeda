//! The background worker that owns the drain loop.

use crate::{
    attribute_current::{
        apply::apply_batch_isolating_failures,
        drain::{DrainCarry, MAX_DRAIN_BATCH, next_buffered_batch},
        queue::{AttributeCurrentBarrier, AttributeCurrentQueue, AttributeCurrentReceiver, open},
        reconcile::reconcile_from_history,
    },
    error::ContextStoreError,
};
use andromeda_postgresql_query::history_batch_entry::HistoryBatchEntry;
use sqlx::PgPool;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::watch;
use tracing::{error, info, warn};

/// Background worker that drains `attribute_current` events and applies them
/// via a short-lived database connection acquired from the shared pool.
pub struct AttributeCurrentMaterializer {
    /// Consumer-side channel handle.
    rx: AttributeCurrentReceiver,
    /// Watermark sender: the highest sequence the worker has fully applied.
    applied: watch::Sender<u64>,
    /// Shared pool from which short-lived apply connections are acquired.
    pool: Arc<PgPool>,
    /// Minimum interval between success-path INFO log lines.
    apply_log_interval: Duration,
}

impl AttributeCurrentMaterializer {
    /// Construct a materializer together with its producer and barrier handles.
    ///
    /// `apply_log_interval` throttles the success-path INFO log; errors are
    /// always logged immediately.
    pub fn new(pool: Arc<PgPool>, apply_log_interval: Duration) -> (AttributeCurrentMaterializer, AttributeCurrentQueue, AttributeCurrentBarrier) {
        let (applied, applied_rx) = watch::channel(0u64);
        let (rx, queue, barrier) = open(applied_rx);
        let worker = AttributeCurrentMaterializer {
            rx,
            applied,
            pool,
            apply_log_interval,
        };
        (worker, queue, barrier)
    }

    /// One-shot reconcile run before the worker enters its drain loop.
    pub async fn reconcile_on_startup(&self) -> Result<u64, ContextStoreError> {
        reconcile_from_history(&self.pool).await
    }

    /// Drive the worker until the channel is closed.
    ///
    /// Each iteration blocks on the first event, then drains every further event
    /// currently buffered, and applies them as one batch. This lets the consumer
    /// catch up on bursts without re-acquiring a connection per producer commit.
    /// After each applied batch the watermark advances to the highest *fully
    /// applied* sequence, which is what [`AttributeCurrentBarrier`] waits on.
    pub async fn run(mut self) {
        let mut last_log = Instant::now().checked_sub(self.apply_log_interval).unwrap_or_else(Instant::now);
        let mut carry = DrainCarry::default();
        while let Some(drained) = next_buffered_batch(&mut self.rx, MAX_DRAIN_BATCH, &mut carry).await {
            if !drained.entries.is_empty() {
                self.apply_batch(&drained.entries, &mut last_log).await;
            }
            if drained.completed_seq > 0 {
                // Ignore send errors: a watermark with no receivers is a broker
                // with no in-flight q-filtered queries.
                let _ = self.applied.send(drained.completed_seq);
            }
        }
    }

    /// Apply one batch of entries, logging outcomes.
    async fn apply_batch(&self, entries: &[HistoryBatchEntry], last_log: &mut Instant) {
        let count = entries.len();
        let started = Instant::now();
        let mut conn = match self.pool.acquire().await {
            Ok(conn) => conn,
            Err(error) => {
                warn!(error = %error, count, event = "attribute_current_acquire_failed");
                return;
            }
        };
        let outcome = apply_batch_isolating_failures(&mut conn, entries).await;
        if outcome.repaired > 0 {
            info!(
                repaired = outcome.repaired,
                applied = outcome.applied,
                event = "attribute_current_rows_repaired"
            );
        }
        if outcome.dropped > 0 {
            // The projection is now missing data that no barrier can account
            // for: `wait_for_pending_writes` will report these writes as
            // applied. Until the next write for the affected attributes, or a
            // restart reconcile, `q` filters may answer from stale state.
            error!(
                dropped = outcome.dropped,
                applied = outcome.applied,
                repaired = outcome.repaired,
                error = %outcome.last_error.as_ref().map_or_else(|| "unknown".to_string(), ToString::to_string),
                event = "attribute_current_rows_dropped"
            );
        }
        let now = Instant::now();
        if now.duration_since(*last_log) >= self.apply_log_interval {
            info!(count, applied_in_ms = started.elapsed().as_millis() as u64, event = "attribute_current_drain");
            *last_log = now;
        }
    }
}
