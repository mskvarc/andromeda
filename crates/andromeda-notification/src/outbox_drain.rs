//! One bounded pass over the durable notification outbox.

use crate::{
    error::NotificationError,
    outbox_config::OutboxConfig,
    outbox_entry::OutboxEntry,
    outbox_store::OutboxStore,
    worker_instance_id::WorkerInstanceId,
};
use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;
use std::sync::Arc;
use tracing::debug;

/// Calculate exponential backoff duration for retry attempts.
///
/// Uses the formula: `base_backoff * 2^min(attempt, max_attempts)`.
/// Saturating arithmetic prevents overflow on large values.
fn calculate_backoff_secs(base_backoff_secs: i64, attempt_count: i32, max_attempts: i32) -> i64 {
    let bounded_attempt = attempt_count.min(max_attempts).max(0);
    let exponent = bounded_attempt.unsigned_abs();
    base_backoff_secs.saturating_mul(2_i64.saturating_pow(exponent))
}

/// Outcome of one bounded notification outbox pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutboxDrainOutcome {
    /// Number of entries claimed during the pass.
    pub claimed: usize,
    /// Whether another immediate pass could find work.
    pub work_may_remain: bool,
}

/// Delivery classification used by durable outbox retry policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationOutboxDisposition {
    /// The selected endpoint accepted the notification.
    Delivered,
    /// The subscription no longer exists or no longer matches, so the row is obsolete.
    Obsolete,
    /// Delivery may succeed on a later attempt.
    RetryableFailure,
    /// Delivery cannot succeed without changing subscription configuration or payload.
    TerminalFailure,
}

/// Delivers one persisted notification intent without exposing transport implementation to outbox policy.
pub trait NotificationOutboxDelivery: Send + Sync + 'static {
    /// Evaluate and deliver one durable outbox entry.
    fn deliver<'a>(&'a self, entry: &'a OutboxEntry) -> BoxFuture<'a, Result<NotificationOutboxDisposition, NotificationError>>;
}

/// Policy operation that advances leased notification outbox entries.
pub struct NotificationOutboxDrain {
    repo: Arc<dyn OutboxStore>,
    delivery: Arc<dyn NotificationOutboxDelivery>,
    worker_id: WorkerInstanceId,
    batch_size: i32,
    max_attempts: i32,
    base_backoff_secs: i64,
}

impl NotificationOutboxDrain {
    /// Create a bounded outbox operation from validated runtime configuration.
    pub fn new(
        repo: Arc<dyn OutboxStore>,
        delivery: Arc<dyn NotificationOutboxDelivery>,
        config: &OutboxConfig,
    ) -> Result<NotificationOutboxDrain, NotificationError> {
        let worker_id = WorkerInstanceId::outbox(uuid::Uuid::new_v4());
        let base_backoff_secs = i64::try_from(config.base_backoff_secs)
            .map_err(|error| NotificationError::representation(format!("outbox base backoff is out of range: {error}")))?;
        Ok(NotificationOutboxDrain {
            repo,
            delivery,
            worker_id,
            batch_size: config.batch_size.max(1),
            max_attempts: config.max_attempts.max(1),
            base_backoff_secs,
        })
    }

    /// Execute one leased delivery pass using an explicit retry clock.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<OutboxDrainOutcome, NotificationError> {
        let entries = self.repo.claim_pending(&self.worker_id, self.batch_size, self.max_attempts).await?;
        let claimed = entries.len();

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
                next_attempt_at = %entry.next_attempt_at,
                "Processing outbox entry"
            );

            let entry_id = entry.id;
            let attempt_count = entry.attempt_count;

            match self.delivery.deliver(&entry).await? {
                NotificationOutboxDisposition::Delivered | NotificationOutboxDisposition::Obsolete => {
                    self.repo.mark_delivered(entry_id).await?;
                }
                NotificationOutboxDisposition::RetryableFailure => {
                    let backoff = calculate_backoff_secs(self.base_backoff_secs, attempt_count, self.max_attempts);
                    let next_attempt = now + Duration::seconds(backoff);
                    self.repo
                        .mark_failed(entry_id, "notification delivery failed transiently", next_attempt)
                        .await?;
                    continue;
                }
                NotificationOutboxDisposition::TerminalFailure => {
                    self.repo.mark_terminal(entry_id, "notification delivery failed permanently").await?;
                    continue;
                }
            }
        }
        let batch_capacity = match usize::try_from(self.batch_size) {
            Ok(value) => value,
            Err(_) => usize::MAX,
        };
        Ok(OutboxDrainOutcome {
            claimed,
            work_may_remain: claimed == batch_capacity,
        })
    }

    /// Purge acknowledged entries before an explicit retention cutoff.
    pub async fn purge_delivered(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        self.repo.delete_delivered_before(cutoff).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outbox_entry::{OutboxEntry, OutboxEntryId};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use async_trait::async_trait;
    use futures::future::BoxFuture;
    use std::{collections::VecDeque, sync::Mutex};

    #[derive(Default)]
    struct RecordingStore {
        entries: Mutex<VecDeque<OutboxEntry>>,
        delivered: Mutex<Vec<OutboxEntryId>>,
        failed: Mutex<Vec<(OutboxEntryId, DateTime<Utc>)>>,
        terminal: Mutex<Vec<OutboxEntryId>>,
    }

    impl RecordingStore {
        fn with_entry(entry: OutboxEntry) -> RecordingStore {
            RecordingStore {
                entries: Mutex::new(VecDeque::from([entry])),
                ..RecordingStore::default()
            }
        }
    }

    #[async_trait]
    impl OutboxStore for RecordingStore {
        async fn claim_pending(&self, _worker_id: &WorkerInstanceId, _batch_size: i32, _max_attempts: i32) -> Result<Vec<OutboxEntry>, NotificationError> {
            let mut entries = self.entries.lock().map_err(|error| NotificationError::persistence(error.to_string()))?;
            Ok(entries.pop_front().into_iter().collect())
        }

        async fn mark_delivered(&self, id: OutboxEntryId) -> Result<u64, NotificationError> {
            self.delivered
                .lock()
                .map_err(|error| NotificationError::persistence(error.to_string()))?
                .push(id);
            Ok(1)
        }

        async fn mark_failed(&self, id: OutboxEntryId, _error: &str, next_attempt_at: DateTime<Utc>) -> Result<u64, NotificationError> {
            self.failed
                .lock()
                .map_err(|error| NotificationError::persistence(error.to_string()))?
                .push((id, next_attempt_at));
            Ok(1)
        }

        async fn mark_terminal(&self, id: OutboxEntryId, _error: &str) -> Result<u64, NotificationError> {
            self.terminal
                .lock()
                .map_err(|error| NotificationError::persistence(error.to_string()))?
                .push(id);
            Ok(1)
        }

        async fn delete_delivered_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
            Ok(0)
        }
    }

    struct FixedDelivery(NotificationOutboxDisposition);

    impl NotificationOutboxDelivery for FixedDelivery {
        fn deliver<'a>(&'a self, _entry: &'a OutboxEntry) -> BoxFuture<'a, Result<NotificationOutboxDisposition, NotificationError>> {
            Box::pin(async move { Ok(self.0) })
        }
    }

    fn config() -> OutboxConfig {
        OutboxConfig {
            enabled: true,
            max_attempts: 5,
            base_backoff_secs: 5,
            poll_interval_secs: 1,
            batch_size: 1,
            cleanup_after_secs: 86_400,
            cleanup_interval_secs: 300,
        }
    }

    fn entry() -> Result<OutboxEntry, NotificationError> {
        Ok(OutboxEntry::builder()
            .id(OutboxEntryId::from(1))
            .subscription_id(
                "urn:ngsi-ld:Subscription:1"
                    .parse::<Uri>()
                    .map_err(|error| NotificationError::representation(error.to_string()))?,
            )
            .entity_id(
                "urn:ngsi-ld:Entity:1"
                    .parse::<Uri>()
                    .map_err(|error| NotificationError::representation(error.to_string()))?,
            )
            .trigger(NotificationTrigger::EntityUpdated)
            .created_at(Utc::now())
            .attempt_count(0)
            .next_attempt_at(Utc::now())
            .build())
    }

    #[test]
    fn delivered_entries_are_acknowledged_only_after_delivery() -> Result<(), NotificationError> {
        futures::executor::block_on(async {
            let store = Arc::new(RecordingStore::with_entry(entry()?));
            let drain = NotificationOutboxDrain::new(store.clone(), Arc::new(FixedDelivery(NotificationOutboxDisposition::Delivered)), &config())?;
            let outcome = drain.execute(Utc::now()).await?;
            pretty_assertions::assert_eq!(outcome.claimed, 1);
            let delivered = store
                .delivered
                .lock()
                .map_err(|error| NotificationError::invariant(format!("delivered result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(delivered.as_slice(), &[OutboxEntryId::from(1)]);
            Ok(())
        })
    }

    #[test]
    fn retryable_and_terminal_delivery_results_take_distinct_paths() -> Result<(), NotificationError> {
        futures::executor::block_on(async {
            let retry_store = Arc::new(RecordingStore::with_entry(entry()?));
            let retry = NotificationOutboxDrain::new(
                retry_store.clone(),
                Arc::new(FixedDelivery(NotificationOutboxDisposition::RetryableFailure)),
                &config(),
            )?;
            retry.execute(Utc::now()).await?;
            let failed_count = retry_store
                .failed
                .lock()
                .map_err(|error| NotificationError::invariant(format!("retry result lock was poisoned: {error}")))?
                .len();
            pretty_assertions::assert_eq!(failed_count, 1);

            let terminal_store = Arc::new(RecordingStore::with_entry(entry()?));
            let terminal = NotificationOutboxDrain::new(
                terminal_store.clone(),
                Arc::new(FixedDelivery(NotificationOutboxDisposition::TerminalFailure)),
                &config(),
            )?;
            terminal.execute(Utc::now()).await?;
            let terminal = terminal_store
                .terminal
                .lock()
                .map_err(|error| NotificationError::invariant(format!("terminal result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(terminal.as_slice(), &[OutboxEntryId::from(1)]);
            Ok(())
        })
    }

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
