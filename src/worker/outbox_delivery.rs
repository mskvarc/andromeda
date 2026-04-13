//! Outbox delivery worker.
//!
//! Polls the `notification_outbox` table for pending entries, delivers
//! notifications via the transport layer, and marks entries as delivered
//! or failed. Periodically cleans up old delivered entries.

use crate::{core::config::OutboxConfig, domain::outbox::WorkerInstanceId, repository::outbox::OutboxRepository};
use chrono::{Duration, Utc};
use std::time::Duration as StdDuration;
use tracing::{debug, error, info};

/// Calculate exponential backoff duration for retry attempts.
///
/// Uses the formula: `base_backoff * 2^min(attempt, max_attempts)`.
/// Saturating arithmetic prevents overflow on large values.
fn calculate_backoff_secs(base_backoff_secs: i64, attempt_count: i32, max_attempts: i32) -> i64 {
    let exponent = attempt_count.min(max_attempts) as u32;
    base_backoff_secs.saturating_mul(2_i64.saturating_pow(exponent))
}

/// Background worker that polls the notification outbox and delivers pending entries.
pub struct OutboxDeliveryWorker {
    repo: OutboxRepository,
    worker_id: WorkerInstanceId,
    poll_interval: StdDuration,
    batch_size: i32,
    max_attempts: i32,
    base_backoff_secs: i64,
    cleanup_after: Duration,
}

impl OutboxDeliveryWorker {
    /// Create a new outbox delivery worker from the given configuration.
    pub fn new(repo: OutboxRepository, config: &OutboxConfig) -> OutboxDeliveryWorker {
        let worker_id = WorkerInstanceId::from(format!("outbox-worker-{}", uuid::Uuid::new_v4()));
        let cleanup_secs = i64::try_from(config.cleanup_after_secs).unwrap_or(i64::MAX);
        let base_backoff = i64::try_from(config.base_backoff_secs).unwrap_or(5);
        OutboxDeliveryWorker {
            repo,
            worker_id,
            poll_interval: StdDuration::from_secs(config.poll_interval_secs),
            batch_size: config.batch_size,
            max_attempts: config.max_attempts,
            base_backoff_secs: base_backoff,
            cleanup_after: Duration::seconds(cleanup_secs),
        }
    }

    /// Run the delivery loop until the process shuts down.
    pub async fn run(self) {
        info!(
            worker_id = %self.worker_id,
            poll_interval = ?self.poll_interval,
            batch_size = self.batch_size,
            "OutboxDeliveryWorker starting"
        );

        let mut ticker = tokio::time::interval(self.poll_interval);
        let mut cleanup_ticker = tokio::time::interval(StdDuration::from_secs(300));

        loop {
            tokio::select! {
                _ = ticker.tick() => {
                    self.poll_and_deliver().await;
                }
                _ = cleanup_ticker.tick() => {
                    self.cleanup_delivered().await;
                }
            }
        }
    }

    async fn poll_and_deliver(&self) {
        let entries = match self.repo.claim_pending(&self.worker_id, self.batch_size, self.max_attempts).await {
            Ok(entries) => entries,
            Err(e) => {
                error!(error = %e, "Failed to claim outbox entries");
                return;
            }
        };

        if entries.is_empty() {
            return;
        }

        debug!(count = entries.len(), "Claimed outbox entries for delivery");

        for entry in entries {
            debug!(
                entry_id = %entry.id,
                subscription_id = %entry.subscription_id,
                entity_id = %entry.entity_id,
                trigger = %entry.trigger,
                attempt = entry.attempt_count,
                has_snapshot = entry.entity_snapshot.is_some(),
                has_previous = entry.previous_snapshot.is_some(),
                changed_attrs = ?entry.changed_attrs,
                created_at = %entry.created_at,
                claimed_at = ?entry.claimed_at,
                claimed_by = ?entry.claimed_by,
                delivered_at = ?entry.delivered_at,
                delivery_error = ?entry.delivery_error,
                next_attempt_at = %entry.next_attempt_at,
                "Processing outbox entry"
            );

            let entry_id = entry.id;
            let attempt_count = entry.attempt_count;

            // Phase 1: mark as delivered immediately (actual HTTP/MQTT delivery
            // is still handled by the broadcast-based NotificationWorker).
            // Phase 2 will replace this with real transport dispatch.
            if let Err(e) = self.repo.mark_delivered(entry_id).await {
                error!(entry_id = %entry_id, error = %e, "Failed to mark outbox entry as delivered");
                let backoff = calculate_backoff_secs(self.base_backoff_secs, attempt_count, self.max_attempts);
                let next_attempt = Utc::now() + Duration::seconds(backoff);
                if let Err(mark_err) = self.repo.mark_failed(entry_id, &e.to_string(), next_attempt).await {
                    error!(entry_id = %entry_id, error = %mark_err, "Failed to mark outbox entry as failed");
                }
            }
        }
    }

    async fn cleanup_delivered(&self) {
        let cutoff = Utc::now() - self.cleanup_after;
        match self.repo.delete_delivered_before(cutoff).await {
            Ok(0) => {}
            Ok(count) => {
                info!(count, "Cleaned up delivered outbox entries");
            }
            Err(e) => {
                error!(error = %e, "Failed to clean up delivered outbox entries");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_first_attempt_equals_base() {
        assert_eq!(calculate_backoff_secs(5, 0, 5), 5);
    }

    #[test]
    fn backoff_doubles_each_attempt() {
        assert_eq!(calculate_backoff_secs(5, 1, 5), 10);
        assert_eq!(calculate_backoff_secs(5, 2, 5), 20);
        assert_eq!(calculate_backoff_secs(5, 3, 5), 40);
    }

    #[test]
    fn backoff_capped_at_max_attempts() {
        // Attempt 5: 5 * 2^5 = 160
        assert_eq!(calculate_backoff_secs(5, 5, 5), 160);
        // Attempt 10 should be capped at 5: still 160
        assert_eq!(calculate_backoff_secs(5, 10, 5), 160);
    }

    #[test]
    fn backoff_handles_large_values_without_overflow() {
        // Large base with high exponent should saturate, not panic
        let result = calculate_backoff_secs(i64::MAX / 2, 10, 10);
        assert!(result > 0);
    }

    #[test]
    fn backoff_zero_base_returns_zero() {
        assert_eq!(calculate_backoff_secs(0, 5, 5), 0);
    }
}
