//! One bounded pass over the notification delivery queue.

use crate::{
    delivery_attempt_ceiling::DeliveryAttemptCeiling,
    delivery_backoff::backoff_seconds,
    delivery_concurrency::DeliveryConcurrency,
    delivery_config::DeliveryConfig,
    delivery_entry::DeliveryEntry,
    delivery_error_bound::DeliveryErrorBound,
    delivery_lease::{DeliveryLeaseError, LeaseTerm},
    delivery_store::DeliveryStore,
    error::NotificationError,
    worker_instance_id::WorkerInstanceId,
};
use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
use chrono::{DateTime, Duration, Utc};
use futures::{StreamExt, future::BoxFuture, stream};
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
    concurrency: DeliveryConcurrency,
    batch_size: i32,
    attempt_ceiling: DeliveryAttemptCeiling,
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
        let concurrency = config
            .delivery_concurrency()
            .map_err(|error| NotificationError::invariant(format!("notification delivery concurrency is unusable: {error}")))?;
        let attempt_ceiling = config
            .attempt_ceiling()
            .map_err(|error| NotificationError::invariant(format!("notification delivery attempt ceiling is unusable: {error}")))?;
        Ok(NotificationDeliveryDrain {
            store,
            transport,
            worker_id,
            lease_term,
            error_bound,
            concurrency,
            batch_size: config.batch_size.max(1),
            attempt_ceiling,
            base_backoff_secs,
        })
    }

    /// Execute one leased delivery pass using an explicit retry clock.
    ///
    /// Claimed deliveries are sent with up to the configured concurrency in
    /// flight at once. Every delivery is driven to its acknowledgement even
    /// when another one fails, so no lease this worker holds is abandoned
    /// mid-flight; the first persistence failure is then surfaced.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<DeliveryDrainOutcome, NotificationError> {
        let entries = self.store.claim_pending(&self.worker_id, self.lease_term, self.batch_size).await?;
        let claimed = entries.len();
        let mut lease_lost = 0_usize;

        debug!(
            count = claimed,
            worker_id = %self.worker_id,
            lease_expires_at = ?self.lease_term.expires_at(now),
            "Claimed notification deliveries"
        );

        let acknowledgements: Vec<Result<LeaseAcknowledgement, NotificationError>> = stream::iter(entries)
            .map(|entry| async move {
                self.trace_entry(&entry);
                self.advance(&entry, now).await
            })
            .buffer_unordered(self.concurrency.get())
            .collect()
            .await;
        for acknowledgement in acknowledgements {
            match acknowledgement? {
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
    ///
    /// The attempt ceiling is applied here: a retryable failure on the attempt
    /// that reaches the ceiling is acknowledged as exhausted, exactly like a
    /// terminal failure, so the store never has to know the ceiling to keep an
    /// exhausted delivery out of the claimable set.
    async fn advance(&self, entry: &DeliveryEntry, now: DateTime<Utc>) -> Result<LeaseAcknowledgement, NotificationError> {
        let (acknowledgement, outcome) = match self.transport.deliver(entry).await? {
            DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete => (
                classify_acknowledgement(self.store.mark_delivered(entry.id, &self.worker_id).await)?,
                QueueOutcome::Completed,
            ),
            DeliveryDisposition::RetryableFailure { detail } if !self.attempt_ceiling.is_final_attempt(entry.attempt_count) => {
                let backoff = backoff_seconds(self.base_backoff_secs, entry.attempt_count, self.attempt_ceiling.attempts());
                let next_attempt = now + Duration::seconds(backoff);
                let detail = self.error_bound.truncate(&detail);
                (
                    classify_acknowledgement(self.store.mark_failed(entry.id, &self.worker_id, &detail, next_attempt).await)?,
                    QueueOutcome::Retried,
                )
            }
            DeliveryDisposition::RetryableFailure { detail } | DeliveryDisposition::TerminalFailure { detail } => {
                let detail = self.error_bound.truncate(&detail);
                (
                    classify_acknowledgement(self.store.mark_terminal(entry.id, &self.worker_id, &detail).await)?,
                    QueueOutcome::Exhausted,
                )
            }
        };
        let outcome = match acknowledgement {
            LeaseAcknowledgement::Applied => outcome,
            LeaseAcknowledgement::Lost => QueueOutcome::LeaseLost,
        };
        record_queue_outcome(QueueName::NotificationDeliveries, outcome, 1);
        Ok(acknowledgement)
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

    /// Purge deliveries dead-lettered before an explicit retention cutoff.
    pub async fn purge_exhausted(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        self.store.delete_exhausted_before(cutoff).await
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
    use futures::{
        executor::block_on,
        future::{BoxFuture, poll_fn},
    };
    use std::{
        collections::VecDeque,
        fmt::Display,
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        task::Poll,
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
            RecordingStore::with_entries([entry])
        }

        fn with_entries(entries: impl IntoIterator<Item = DeliveryEntry>) -> RecordingStore {
            RecordingStore {
                entries: Mutex::new(entries.into_iter().collect()),
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
        async fn claim_pending(&self, _worker_id: &WorkerInstanceId, lease: LeaseTerm, _batch_size: i32) -> Result<Vec<DeliveryEntry>, NotificationError> {
            self.claimed_with
                .lock()
                .map_err(|error| NotificationError::persistence(error.to_string()))?
                .push(lease);
            let mut entries = self.entries.lock().map_err(|error| NotificationError::persistence(error.to_string()))?;
            Ok(entries.drain(..).collect())
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

        async fn delete_exhausted_before(&self, _cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
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

    /// A transport that yields once mid-delivery and records how many
    /// deliveries were in flight at the same time.
    #[derive(Default)]
    struct InFlightTransport {
        in_flight: AtomicUsize,
        peak: AtomicUsize,
    }

    impl NotificationDeliveryTransport for InFlightTransport {
        fn deliver<'a>(&'a self, _entry: &'a DeliveryEntry) -> BoxFuture<'a, Result<DeliveryDisposition, NotificationError>> {
            Box::pin(async move {
                let now_in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(now_in_flight, Ordering::SeqCst);
                let mut yielded = false;
                poll_fn(|context| {
                    if yielded {
                        Poll::Ready(())
                    } else {
                        yielded = true;
                        context.waker().wake_by_ref();
                        Poll::Pending
                    }
                })
                .await;
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
                Ok(DeliveryDisposition::Delivered)
            })
        }
    }

    fn config() -> DeliveryConfig {
        DeliveryConfig {
            enabled: true,
            max_attempts: 5,
            base_backoff_secs: 5,
            poll_interval_secs: 1,
            batch_size: 1,
            concurrency: 4,
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
    fn a_pass_sends_claimed_deliveries_concurrently_up_to_the_configured_bound() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=10_i64)
                .map(|id| {
                    entry().map(|template| DeliveryEntry {
                        id: DeliveryId::from(id),
                        ..template
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let transport = Arc::new(InFlightTransport::default());
            let config = DeliveryConfig { batch_size: 10, ..config() };
            let outcome = NotificationDeliveryDrain::new(store.clone(), transport.clone(), &config)?
                .execute(Utc::now())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 10);
            let peak = transport.peak.load(Ordering::SeqCst);
            assert!(peak > 1, "deliveries were sent one at a time, peak {peak}");
            assert!(peak <= 4, "more deliveries in flight than configured, peak {peak}");
            let delivered = store
                .delivered
                .lock()
                .map_err(|error| NotificationError::invariant(format!("delivered result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(delivered.len(), 10);
            Ok(())
        })
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
    fn a_retryable_failure_on_the_final_permitted_attempt_is_acknowledged_as_exhausted() -> Result<(), NotificationError> {
        block_on(async {
            let mut final_attempt = entry()?;
            final_attempt.attempt_count = 4;
            let store = Arc::new(RecordingStore::with_entry(final_attempt));
            drain(
                store.clone(),
                DeliveryDisposition::RetryableFailure {
                    detail: "transient".to_owned(),
                },
            )?
            .execute(Utc::now())
            .await?;
            let failed_count = store
                .failed
                .lock()
                .map_err(|error| NotificationError::invariant(format!("retry result lock was poisoned: {error}")))?
                .len();
            pretty_assertions::assert_eq!(failed_count, 0, "the final attempt must not be rescheduled");
            let terminal = store
                .terminal
                .lock()
                .map_err(|error| NotificationError::invariant(format!("terminal result lock was poisoned: {error}")))?;
            pretty_assertions::assert_eq!(
                terminal.first().map(|(id, detail)| (*id, detail.as_str())),
                Some((DeliveryId::from(1), "transien"))
            );
            Ok(())
        })
    }

    #[test]
    fn a_retryable_failure_below_the_ceiling_is_rescheduled_not_exhausted() -> Result<(), NotificationError> {
        block_on(async {
            let mut penultimate = entry()?;
            penultimate.attempt_count = 3;
            let store = Arc::new(RecordingStore::with_entry(penultimate));
            drain(
                store.clone(),
                DeliveryDisposition::RetryableFailure {
                    detail: "transient".to_owned(),
                },
            )?
            .execute(Utc::now())
            .await?;
            let terminal_count = store
                .terminal
                .lock()
                .map_err(|error| NotificationError::invariant(format!("terminal result lock was poisoned: {error}")))?
                .len();
            pretty_assertions::assert_eq!(terminal_count, 0);
            let failed_count = store
                .failed
                .lock()
                .map_err(|error| NotificationError::invariant(format!("retry result lock was poisoned: {error}")))?
                .len();
            pretty_assertions::assert_eq!(failed_count, 1);
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
