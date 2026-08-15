//! One bounded pass over the notification delivery queue.

use crate::{
    delivery_backoff::backoff_seconds,
    delivery_config::DeliveryConfig,
    delivery_entry::DeliveryEntry,
    delivery_error_bound::DeliveryErrorBound,
    delivery_lease::{DeliveryLeaseError, LeaseTerm},
    delivery_store::DeliveryStore,
    error::NotificationError,
    worker_instance_id::WorkerInstanceId,
};
use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;
use std::sync::Arc;
use tracing::{debug, warn};

/// Outcome of one bounded notification delivery pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryDrainOutcome {
    /// Number of deliveries claimed during the pass.
    pub claimed: usize,
    /// Number of claimed deliveries whose lease had expired before the worker
    /// acknowledged them, so another worker owns their outcome.
    pub lease_lost: usize,
    /// Whether another immediate pass could find work.
    pub work_may_remain: bool,
}

/// Absorb a lost lease and surface a failed statement.
///
/// A lost lease is not this pass's failure: the delivery was reclaimed by
/// another worker, which is now responsible for sending it, so the pass records
/// the loss and moves on. A failed statement is a persistence failure the task
/// supervisor must see.
fn classify_acknowledgement(result: Result<(), DeliveryLeaseError>) -> Result<LeaseAcknowledgement, NotificationError> {
    match result {
        Ok(()) => Ok(LeaseAcknowledgement::Applied),
        Err(DeliveryLeaseError::LeaseLost { delivery, worker }) => {
            warn!(delivery_id = %delivery, worker_id = %worker, event = "notification_delivery_lease_lost");
            Ok(LeaseAcknowledgement::Lost)
        }
        Err(DeliveryLeaseError::Persistence { detail }) => Err(NotificationError::persistence(detail)),
    }
}

/// Delivery classification that drives notification retry policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeliveryDisposition {
    /// The selected endpoint accepted the notification.
    Delivered,
    /// The subscription no longer exists or no longer matches, so the delivery
    /// is obsolete.
    Obsolete,
    /// Delivery may succeed on a later attempt.
    RetryableFailure {
        /// Diagnostic recorded against the delivery, trimmed before it is stored.
        detail: String,
    },
    /// Delivery cannot succeed without changing subscription configuration or
    /// payload.
    TerminalFailure {
        /// Diagnostic recorded against the delivery, trimmed before it is stored.
        detail: String,
    },
}

/// Sends one persisted notification without exposing transport implementation
/// to delivery policy.
pub trait NotificationDeliveryTransport: Send + Sync + 'static {
    /// Evaluate and send one claimed delivery.
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, Result<DeliveryDisposition, NotificationError>>;
}

/// Policy operation that advances leased notification deliveries.
pub struct NotificationDeliveryDrain {
    store: Arc<dyn DeliveryStore>,
    transport: Arc<dyn NotificationDeliveryTransport>,
    worker_id: WorkerInstanceId,
    lease_term: LeaseTerm,
    error_bound: DeliveryErrorBound,
    batch_size: i32,
    max_attempts: i32,
    base_backoff_secs: i64,
}

impl NotificationDeliveryDrain {
    /// Create a bounded delivery operation from validated runtime configuration.
    pub fn new(
        store: Arc<dyn DeliveryStore>,
        transport: Arc<dyn NotificationDeliveryTransport>,
        config: &DeliveryConfig,
    ) -> Result<NotificationDeliveryDrain, NotificationError> {
        let worker_id = WorkerInstanceId::delivery(uuid::Uuid::new_v4());
        let base_backoff_secs = i64::try_from(config.base_backoff_secs)
            .map_err(|error| NotificationError::representation(format!("notification delivery base backoff is out of range: {error}")))?;
        let lease_term = config
            .lease_term()
            .map_err(|error| NotificationError::invariant(format!("notification delivery lease term is unusable: {error}")))?;
        let error_bound = config
            .error_bound()
            .map_err(|error| NotificationError::invariant(format!("notification delivery error bound is unusable: {error}")))?;
        Ok(NotificationDeliveryDrain {
            store,
            transport,
            worker_id,
            lease_term,
            error_bound,
            batch_size: config.batch_size.max(1),
            max_attempts: config.max_attempts.max(1),
            base_backoff_secs,
        })
    }

    /// Execute one leased delivery pass using an explicit retry clock.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<DeliveryDrainOutcome, NotificationError> {
        let entries = self
            .store
            .claim_pending(&self.worker_id, self.lease_term, self.batch_size, self.max_attempts)
            .await?;
        let claimed = entries.len();
        let mut lease_lost = 0_usize;

        debug!(
            count = claimed,
            worker_id = %self.worker_id,
            lease_expires_at = ?self.lease_term.expires_at(now),
            "Claimed notification deliveries"
        );

        for entry in entries {
            self.trace_entry(&entry);
            match self.advance(&entry, now).await? {
                LeaseAcknowledgement::Applied => (),
                LeaseAcknowledgement::Lost => lease_lost += 1,
            }
        }
        let batch_capacity = match usize::try_from(self.batch_size) {
            Ok(value) => value,
            Err(_) => usize::MAX,
        };
        Ok(DeliveryDrainOutcome {
            claimed,
            lease_lost,
            work_may_remain: claimed == batch_capacity,
        })
    }

    /// Deliver one claimed entry and acknowledge whatever the transport reported.
    async fn advance(&self, entry: &DeliveryEntry, now: DateTime<Utc>) -> Result<LeaseAcknowledgement, NotificationError> {
        match self.transport.deliver(entry).await? {
            DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete => {
                classify_acknowledgement(self.store.mark_delivered(entry.id, &self.worker_id).await)
            }
            DeliveryDisposition::RetryableFailure { detail } => {
                let backoff = backoff_seconds(self.base_backoff_secs, entry.attempt_count, self.max_attempts);
                let next_attempt = now + Duration::seconds(backoff);
                let detail = self.error_bound.truncate(&detail);
                classify_acknowledgement(self.store.mark_failed(entry.id, &self.worker_id, &detail, next_attempt).await)
            }
            DeliveryDisposition::TerminalFailure { detail } => {
                let detail = self.error_bound.truncate(&detail);
                classify_acknowledgement(self.store.mark_terminal(entry.id, &self.worker_id, &detail).await)
            }
        }
    }

    /// Record what a claimed delivery carries before it is attempted.
    fn trace_entry(&self, entry: &DeliveryEntry) {
        debug!(
            delivery_id = %entry.id,
            event_id = %entry.event.id,
            subscription_id = %entry.subscription_id,
            entity_id = %entry.event.entity_id,
            trigger = %entry.event.trigger,
            attempt = entry.attempt_count,
            has_snapshot = entry.event.entity_snapshot.is_some(),
            has_previous = entry.event.previous_snapshot.is_some(),
            changed_attributes = ?entry.event.changed_attributes,
            created_at = %entry.event.created_at,
            next_attempt_at = %entry.next_attempt_at,
            "Processing notification delivery"
        );
    }

    /// Purge acknowledged deliveries before an explicit retention cutoff.
    pub async fn purge_delivered(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        self.store.delete_delivered_before(cutoff).await
    }

    /// Purge dead-lettered deliveries whose event predates an explicit
    /// retention cutoff.
    pub async fn purge_exhausted(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        self.store.delete_exhausted_before(cutoff, self.max_attempts).await
    }

    /// Purge events with no referencing delivery, created before an explicit
    /// retention cutoff.
    pub async fn purge_orphan_events(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        self.store.delete_orphan_events_before(cutoff).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_config::DeliveryConfig,
        delivery_drain::{DeliveryDisposition, NotificationDeliveryDrain, NotificationDeliveryTransport},
        delivery_entry::{DeliveryEntry, DeliveryId},
        delivery_lease::{DeliveryLeaseError, LeaseTerm},
        delivery_store::DeliveryStore,
        error::NotificationError,
        notification_event::{NotificationEvent, NotificationEventId},
        worker_instance_id::WorkerInstanceId,
    };
    use andromeda_context_commit::{write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::notification::NotificationTrigger;
    use async_trait::async_trait;
    use chrono::{DateTime, Utc};
    use futures::{executor::block_on, future::BoxFuture};
    use std::{
        collections::VecDeque,
        fmt::Display,
        sync::{Arc, Mutex},
    };

    /// Whether the recorded store still honours the leases it handed out.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    enum LeaseState {
        #[default]
        Held,
        Expired,
    }

    #[derive(Default)]
    struct RecordingStore {
        entries: Mutex<VecDeque<DeliveryEntry>>,
        delivered: Mutex<Vec<DeliveryId>>,
        failed: Mutex<Vec<(DeliveryId, String, DateTime<Utc>)>>,
        terminal: Mutex<Vec<(DeliveryId, String)>>,
        claimed_with: Mutex<Vec<LeaseTerm>>,
        lease_state: LeaseState,
    }

    impl RecordingStore {
        fn with_entry(entry: DeliveryEntry) -> RecordingStore {
            RecordingStore {
                entries: Mutex::new(VecDeque::from([entry])),
                ..RecordingStore::default()
            }
        }

        fn with_expired_lease(entry: DeliveryEntry) -> RecordingStore {
            RecordingStore {
                lease_state: LeaseState::Expired,
                ..RecordingStore::with_entry(entry)
            }
        }

        fn lease_check(&self, id: DeliveryId, worker_id: &WorkerInstanceId) -> Result<(), DeliveryLeaseError> {
            match self.lease_state {
                LeaseState::Held => Ok(()),
                LeaseState::Expired => Err(DeliveryLeaseError::lease_lost(id, worker_id)),
            }
        }
    }

    /// Map a poisoned recording lock into the port's persistence category.
    fn poisoned(error: impl Display) -> DeliveryLeaseError {
        DeliveryLeaseError::persistence(error.to_string())
    }

    #[async_trait]
    impl DeliveryStore for RecordingStore {
        async fn claim_pending(
            &self,
            _worker_id: &WorkerInstanceId,
            lease: LeaseTerm,
            _batch_size: i32,
            _max_attempts: i32,
        ) -> Result<Vec<DeliveryEntry>, NotificationError> {
            self.claimed_with
                .lock()
                .map_err(|error| NotificationError::persistence(error.to_string()))?
                .push(lease);
            let mut entries = self.entries.lock().map_err(|error| NotificationError::persistence(error.to_string()))?;
            Ok(entries.pop_front().into_iter().collect())
        }

        async fn mark_delivered(&self, id: DeliveryId, worker_id: &WorkerInstanceId) -> Result<(), DeliveryLeaseError> {
            self.lease_check(id, worker_id)?;
            self.delivered.lock().map_err(poisoned)?.push(id);
            Ok(())
        }

        async fn mark_failed(
            &self,
            id: DeliveryId,
            worker_id: &WorkerInstanceId,
            error: &str,
            next_attempt_at: DateTime<Utc>,
        ) -> Result<(), DeliveryLeaseError> {
            self.lease_check(id, worker_id)?;
            self.failed.lock().map_err(poisoned)?.push((id, error.to_owned(), next_attempt_at));
            Ok(())
        }

        async fn mark_terminal(&self, id: DeliveryId, worker_id: &WorkerInstanceId, error: &str) -> Result<(), DeliveryLeaseError> {
            self.lease_check(id, worker_id)?;
            self.terminal.lock().map_err(poisoned)?.push((id, error.to_owned()));
            Ok(())
        }

        async fn delete_delivered_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
            Ok(0)
        }

        async fn delete_exhausted_before(&self, _cutoff: DateTime<Utc>, _max_attempts: i32) -> Result<u64, NotificationError> {
            Ok(0)
        }

        async fn delete_orphan_events_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
            Ok(0)
        }
    }

    struct FixedTransport(DeliveryDisposition);

    impl NotificationDeliveryTransport for FixedTransport {
        fn deliver<'a>(&'a self, _entry: &'a DeliveryEntry) -> BoxFuture<'a, Result<DeliveryDisposition, NotificationError>> {
            // clone: every claimed delivery is answered with the same fixed disposition.
            Box::pin(async move { Ok(self.0.clone()) })
        }
    }

    fn config() -> DeliveryConfig {
        DeliveryConfig {
            enabled: true,
            max_attempts: 5,
            base_backoff_secs: 5,
            poll_interval_secs: 1,
            batch_size: 1,
            lease_secs: 30,
            error_max_chars: 8,
            cleanup_after_secs: 86_400,
            cleanup_interval_secs: 300,
            dead_letter_after_secs: 604_800,
            orphan_event_after_secs: 86_400,
        }
    }

    fn entry() -> Result<DeliveryEntry, NotificationError> {
        let parse = |value: &str| value.parse::<Uri>().map_err(|error| NotificationError::representation(error.to_string()));
        let event = NotificationEvent::builder()
            .id(NotificationEventId::from(1))
            .write_id(WriteId::mint())
            .event_ordinal(WriteOrdinal::at(0))
            .trigger(NotificationTrigger::EntityUpdated)
            .entity_id(parse("urn:ngsi-ld:Entity:1")?)
            .created_at(Utc::now())
            .build();
        Ok(DeliveryEntry::builder()
            .id(DeliveryId::from(1))
            .subscription_id(parse("urn:ngsi-ld:Subscription:1")?)
            .attempt_count(0)
            .next_attempt_at(Utc::now())
            .event(event)
            .build())
    }

    fn drain(store: Arc<RecordingStore>, disposition: DeliveryDisposition) -> Result<NotificationDeliveryDrain, NotificationError> {
        NotificationDeliveryDrain::new(store, Arc::new(FixedTransport(disposition)), &config())
    }

    #[test]
    fn delivered_entries_are_acknowledged_only_after_delivery() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_entry(entry()?));
            let outcome = drain(store.clone(), DeliveryDisposition::Delivered)?.execute(Utc::now()).await?;
            pretty_assertions::assert_eq!(outcome.claimed, 1);
            let delivered = store
                .delivered
                .lock()
                .map_err(|error| NotificationError::invariant(format!("delivered result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(delivered.as_slice(), &[DeliveryId::from(1)]);
            Ok(())
        })
    }

    #[test]
    fn retryable_and_terminal_delivery_results_take_distinct_paths() -> Result<(), NotificationError> {
        block_on(async {
            let retry_store = Arc::new(RecordingStore::with_entry(entry()?));
            drain(
                retry_store.clone(),
                DeliveryDisposition::RetryableFailure {
                    detail: "transient".to_owned(),
                },
            )?
            .execute(Utc::now())
            .await?;
            let failed_count = retry_store
                .failed
                .lock()
                .map_err(|error| NotificationError::invariant(format!("retry result lock was poisoned: {error}")))?
                .len();
            pretty_assertions::assert_eq!(failed_count, 1);

            let terminal_store = Arc::new(RecordingStore::with_entry(entry()?));
            drain(
                terminal_store.clone(),
                DeliveryDisposition::TerminalFailure {
                    detail: "permanent".to_owned(),
                },
            )?
            .execute(Utc::now())
            .await?;
            let terminal = terminal_store
                .terminal
                .lock()
                .map_err(|error| NotificationError::invariant(format!("terminal result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(terminal.len(), 1);
            pretty_assertions::assert_eq!(terminal.first().map(|(id, _)| *id), Some(DeliveryId::from(1)));
            Ok(())
        })
    }

    #[test]
    fn a_recorded_failure_diagnostic_is_truncated_to_the_configured_bound() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_entry(entry()?));
            drain(
                store.clone(),
                DeliveryDisposition::RetryableFailure {
                    detail: "endpoint refused the notification".to_owned(),
                },
            )?
            .execute(Utc::now())
            .await?;
            let failed = store
                .failed
                .lock()
                .map_err(|error| NotificationError::invariant(format!("retry result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(failed.first().map(|(_, detail, _)| detail.as_str()), Some("endpoint"));
            Ok(())
        })
    }

    #[test]
    fn claims_carry_the_configured_lease_term() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_entry(entry()?));
            drain(store.clone(), DeliveryDisposition::Delivered)?.execute(Utc::now()).await?;
            let claimed_with = store
                .claimed_with
                .lock()
                .map_err(|error| NotificationError::invariant(format!("claim record lock was poisoned: {error}")))?;
            let expected = LeaseTerm::from_seconds(30).map_err(|error| NotificationError::invariant(error.to_string()))?;
            pretty_assertions::assert_eq!(claimed_with.as_slice(), &[expected]);
            Ok(())
        })
    }

    #[test]
    fn an_expired_lease_is_reported_instead_of_acknowledged_silently() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_expired_lease(entry()?));
            let outcome = drain(store.clone(), DeliveryDisposition::Delivered)?.execute(Utc::now()).await?;
            pretty_assertions::assert_eq!(outcome.claimed, 1);
            pretty_assertions::assert_eq!(outcome.lease_lost, 1);
            let delivered = store
                .delivered
                .lock()
                .map_err(|error| NotificationError::invariant(format!("delivered result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(delivered.is_empty(), true);
            Ok(())
        })
    }

    #[test]
    fn a_lost_lease_on_a_retryable_failure_does_not_reschedule_the_delivery() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_expired_lease(entry()?));
            let outcome = drain(
                store.clone(),
                DeliveryDisposition::RetryableFailure {
                    detail: "transient".to_owned(),
                },
            )?
            .execute(Utc::now())
            .await?;
            pretty_assertions::assert_eq!(outcome.lease_lost, 1);
            let failed = store
                .failed
                .lock()
                .map_err(|error| NotificationError::invariant(format!("retry result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(failed.is_empty(), true);
            Ok(())
        })
    }
}
