//! One bounded pass over the notification delivery queue.

use crate::{
    delivery_attempt_ceiling::DeliveryAttemptCeiling,
    delivery_backoff::backoff_seconds,
    delivery_claim::{DeliveryClaim, UnreadableDelivery},
    delivery_claim_budget::ClaimBudget,
    delivery_concurrency::DeliveryConcurrency,
    delivery_config::DeliveryConfig,
    delivery_entry::DeliveryEntry,
    delivery_error_bound::DeliveryErrorBound,
    delivery_lease::{DeliveryLeaseError, LeaseTerm},
    delivery_store::DeliveryStore,
    error::NotificationError,
    held_events::HeldEvents,
    notification_invariant::NotificationInvariant,
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, lease_pass_clock::LeasePassClock};
use andromeda_telemetry::queue::{QueueName, QueueOutcome, record_queue_outcome};
use chrono::{DateTime, Duration, Utc};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use std::sync::Arc;
use tracing::{debug, error, warn};

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
fn classify_acknowledgement(result: &Result<(), DeliveryLeaseError>) -> Result<LeaseAcknowledgement, NotificationError> {
    match result {
        Ok(()) => Ok(LeaseAcknowledgement::Applied),
        Err(DeliveryLeaseError::LeaseLost { delivery, worker }) => {
            warn!(delivery_id = %delivery, worker_id = %worker, event = "notification_delivery_lease_lost");
            Ok(LeaseAcknowledgement::Lost)
        }
        Err(DeliveryLeaseError::Persistence(failure)) => Err(NotificationError::Persistence(*failure)),
    }
}

/// Record the queue outcome of one acknowledged delivery, or its lost lease.
fn record_outcome(acknowledgement: LeaseAcknowledgement, outcome: QueueOutcome) -> LeaseAcknowledgement {
    let outcome = match acknowledgement {
        LeaseAcknowledgement::Applied => outcome,
        LeaseAcknowledgement::Lost => QueueOutcome::LeaseLost,
    };
    record_queue_outcome(QueueName::NotificationDeliveries, outcome, 1);
    acknowledgement
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
///
/// Every outcome of one delivery is a [`DeliveryDisposition`], never an error:
/// the transport is the one that knows whether a failure can pass, so it
/// classifies it. A failure a later attempt may overcome, such as an endpoint
/// that did not answer, is a
/// [`DeliveryDisposition::RetryableFailure`]; one the stored delivery can never
/// overcome, such as a payload that cannot be evaluated, is a
/// [`DeliveryDisposition::TerminalFailure`]. Without an error channel no single
/// delivery can end the pass its peers were claimed in, nor leave its own lease
/// to run out unacknowledged.
pub trait NotificationDeliveryTransport: Send + Sync + 'static {
    /// Evaluate and send one claimed delivery.
    fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition>;
}

/// Policy operation that advances leased notification deliveries.
pub struct NotificationDeliveryDrain {
    store: Arc<dyn DeliveryStore>,
    transport: Arc<dyn NotificationDeliveryTransport>,
    worker_id: LeaseHolder,
    lease_term: LeaseTerm,
    error_bound: DeliveryErrorBound,
    concurrency: DeliveryConcurrency,
    /// Deliveries one pass sends at most.
    batch_size: usize,
    attempt_ceiling: DeliveryAttemptCeiling,
    base_backoff_secs: i64,
}

/// One thing that finished in a pass's in-flight set.
enum SlotEvent {
    /// A claim that asked for `asked` deliveries came back.
    Claimed {
        /// Deliveries the claim asked for.
        asked: usize,
        /// What the claim leased, or why it could not.
        claims: Result<Vec<DeliveryClaim>, NotificationError>,
    },
    /// A claimed delivery was attempted and its outcome acknowledged.
    Acknowledged(Result<LeaseAcknowledgement, NotificationError>),
}

/// Record what a claimed delivery carries before it is attempted.
fn trace_entry(entry: &DeliveryEntry) {
    debug!(
        delivery_key = %entry.key,
        event_key = %entry.event.key,
        subscription_id = %entry.subscription_id,
        entity_id = %entry.event.entity_id,
        lifecycle = ?entry.event.change.lifecycle,
        attempt = entry.attempt_count,
        has_snapshot = entry.event.entity_snapshot.is_some(),
        has_previous = entry.event.previous_snapshot.is_some(),
        changed_attributes = ?entry.event.change.attributes(),
        created_at = %entry.event.created_at,
        next_attempt_at = %entry.next_attempt_at,
        "Processing notification delivery"
    );
}

impl NotificationDeliveryDrain {
    /// Create a bounded delivery operation from validated runtime configuration.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Invariant`] when the configured base
    /// backoff does not fit a signed second count, and
    /// [`NotificationError::Invariant`] when the configured lease term, stored
    /// diagnostic bound, in-flight delivery bound, attempt ceiling or batch
    /// size is out of range.
    pub fn new(
        store: Arc<dyn DeliveryStore>,
        transport: Arc<dyn NotificationDeliveryTransport>,
        config: &DeliveryConfig,
    ) -> Result<NotificationDeliveryDrain, NotificationError> {
        let worker_id = LeaseHolder::mint();
        let base_backoff_secs = i64::try_from(config.base_backoff.duration().as_secs()).map_err(NotificationInvariant::BaseBackoff)?;
        let lease_term = config.lease_term().map_err(NotificationInvariant::from)?;
        let error_bound = config.error_bound().map_err(NotificationInvariant::from)?;
        let concurrency = config.delivery_concurrency().map_err(NotificationInvariant::from)?;
        let attempt_ceiling = config.attempt_ceiling().map_err(NotificationInvariant::from)?;
        let batch_size = usize::try_from(config.batch_size)
            .ok()
            .filter(|size| *size > 0)
            .ok_or(NotificationInvariant::BatchSizeNotPositive)?;
        Ok(NotificationDeliveryDrain {
            store,
            transport,
            worker_id,
            lease_term,
            error_bound,
            concurrency,
            batch_size,
            attempt_ceiling,
            base_backoff_secs,
        })
    }

    /// Execute one leased delivery pass that starts at `now`.
    ///
    /// A pass keeps up to the configured concurrency of slots busy until it has
    /// claimed its batch or the queue runs out. It opens by claiming one
    /// delivery per slot, and each time a delivery is acknowledged it claims
    /// one more for the slot that freed. A delivery is claimed only when a slot
    /// is free to send it, so its lease starts as its attempt does and has to
    /// outlast that one attempt -- which `request_timeout` bounds -- never the
    /// deliveries sent before it; and no slot waits on a slow delivery in
    /// another. A batch claimed under one lease and sent a few at a time would
    /// instead still be waiting to be sent when that lease ran out, and a
    /// second worker would claim and send it again.
    ///
    /// The claims of one pass share the events they read: a claim hands over
    /// the event a delivery still in flight already carries instead of reading
    /// it again, so the deliveries of one event, claimed one slot at a time,
    /// cost one event read between them.
    ///
    /// Every claimed delivery is driven to its acknowledgement even when
    /// another one fails, so no lease this worker holds is abandoned
    /// mid-flight; the first persistence failure is then surfaced, and no
    /// further delivery is claimed once it occurred. A retry is scheduled one
    /// backoff after the instant it is acknowledged, not after the start of the
    /// pass, so it is never already due before the pass that scheduled it ends.
    ///
    /// What one delivery does never ends the pass: a delivery whose row could
    /// not be read back, or whose transport reports it can never succeed, is
    /// acknowledged as exhausted with its diagnostic, because a delivery left
    /// leased and unacknowledged would be claimed again at every lease expiry
    /// with its attempt count unchanged, forever. Only the queue failing the
    /// worker ends a pass -- a claim it cannot serve, an acknowledgement it
    /// cannot record -- because that is not a property of any one delivery and
    /// the next pass may find it passed.
    ///
    /// # Errors
    ///
    /// Returns whatever the delivery store reports when the queue cannot be
    /// claimed from, and [`NotificationError::Persistence`] when acknowledging a
    /// delivery outcome fails. A lease another worker reclaimed is counted in
    /// [`DeliveryDrainOutcome::lease_lost`] rather than surfaced as an error.
    pub async fn execute(&self, now: DateTime<Utc>) -> Result<DeliveryDrainOutcome, NotificationError> {
        let clock = LeasePassClock::starting_at(now);
        let held = HeldEvents::default();
        let mut budget = ClaimBudget::new(self.batch_size);
        let mut lease_lost = 0_usize;
        let mut failure: Option<NotificationError> = None;
        let mut slots: FuturesUnordered<BoxFuture<'_, SlotEvent>> = FuturesUnordered::new();
        let opening = budget.reserve(self.concurrency.get());
        slots.push(self.claim(opening, &clock, &held).boxed());
        while let Some(event) = slots.next().await {
            match event {
                SlotEvent::Claimed { asked, claims: Ok(claims) } => {
                    budget.record(asked, claims.len());
                    slots.extend(claims.into_iter().map(|claim| self.attempt(claim, &clock).boxed()));
                }
                SlotEvent::Claimed { claims: Err(error), .. } => {
                    failure.get_or_insert(error);
                }
                SlotEvent::Acknowledged(acknowledgement) => {
                    match acknowledgement {
                        Ok(LeaseAcknowledgement::Applied) => {}
                        Ok(LeaseAcknowledgement::Lost) => lease_lost += 1,
                        Err(error) => {
                            failure.get_or_insert(error);
                        }
                    }
                    // The acknowledged delivery freed its slot.
                    if failure.is_none() && budget.can_claim_more() {
                        let refill = budget.reserve(1);
                        slots.push(self.claim(refill, &clock, &held).boxed());
                    }
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(DeliveryDrainOutcome {
                claimed: budget.claimed(),
                lease_lost,
                work_may_remain: budget.work_may_remain(),
            }),
        }
    }

    /// Claim up to `width` deliveries under a lease that starts now, reusing
    /// the events the pass's in-flight deliveries hold.
    async fn claim(&self, width: usize, clock: &LeasePassClock, held: &HeldEvents) -> SlotEvent {
        let claims = match i32::try_from(width) {
            Ok(claim_width) => self.store.claim_pending(&self.worker_id, self.lease_term, claim_width, held).await,
            Err(error) => Err(NotificationInvariant::ClaimWidth(error).into()),
        };
        if let Ok(claimed) = &claims {
            debug!(
                count = claimed.len(),
                worker_id = %self.worker_id,
                lease_expires_at = ?self.lease_term.expires_at(clock.now()),
                "Claimed notification deliveries"
            );
        }
        SlotEvent::Claimed { asked: width, claims }
    }

    /// Attempt one claimed delivery and acknowledge its outcome.
    async fn attempt(&self, claim: DeliveryClaim, clock: &LeasePassClock) -> SlotEvent {
        SlotEvent::Acknowledged(match claim {
            DeliveryClaim::Readable(entry) => {
                trace_entry(&entry);
                self.advance(&entry, clock).await
            }
            DeliveryClaim::Unreadable(unreadable) => self.retire_unreadable(&unreadable).await,
        })
    }

    /// Deliver one claimed entry and acknowledge whatever the transport reported.
    ///
    /// The attempt ceiling is applied here: a retryable failure on the attempt
    /// that reaches the ceiling is acknowledged as exhausted, exactly like a
    /// terminal failure, so the store never has to know the ceiling to keep an
    /// exhausted delivery out of the claimable set.
    async fn advance(&self, entry: &DeliveryEntry, clock: &LeasePassClock) -> Result<LeaseAcknowledgement, NotificationError> {
        let (acknowledgement, outcome) = match self.transport.deliver(entry).await {
            DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete => (
                classify_acknowledgement(&self.store.mark_delivered(entry.key, &self.worker_id, &entry.target).await)?,
                QueueOutcome::Completed,
            ),
            DeliveryDisposition::RetryableFailure { detail } if !self.attempt_ceiling.is_final_attempt(entry.attempt_count) => {
                let backoff = backoff_seconds(self.base_backoff_secs, entry.attempt_count, self.attempt_ceiling.attempts());
                let next_attempt = clock.now() + Duration::seconds(backoff);
                let detail = self.error_bound.truncate(&detail);
                (
                    classify_acknowledgement(&self.store.mark_failed(entry.key, &self.worker_id, &detail, next_attempt, &entry.target).await)?,
                    QueueOutcome::Retried,
                )
            }
            DeliveryDisposition::RetryableFailure { detail } | DeliveryDisposition::TerminalFailure { detail } => {
                let detail = self.error_bound.truncate(&detail);
                (
                    classify_acknowledgement(&self.store.mark_terminal(entry.key, &self.worker_id, &detail, &entry.target).await)?,
                    QueueOutcome::Exhausted,
                )
            }
        };
        Ok(record_outcome(acknowledgement, outcome))
    }

    /// Retire a claimed delivery whose stored row could not be read back.
    ///
    /// Nothing a later attempt does can repair a stored value, so the delivery
    /// is acknowledged as exhausted with the reason it could not be read as its
    /// recorded failure, exactly like a delivery whose transport reported a
    /// terminal failure.
    async fn retire_unreadable(&self, unreadable: &UnreadableDelivery) -> Result<LeaseAcknowledgement, NotificationError> {
        error!(
            delivery_key = %unreadable.key,
            error = %unreadable.diagnostic,
            event = "notification_delivery_unreadable"
        );
        let detail = self.error_bound.truncate(&unreadable.diagnostic);
        let acknowledgement = classify_acknowledgement(&self.store.mark_terminal(unreadable.key, &self.worker_id, &detail, &unreadable.target).await)?;
        Ok(record_outcome(acknowledgement, QueueOutcome::Exhausted))
    }

    /// Purge acknowledged deliveries before an explicit retention cutoff.
    ///
    /// # Errors
    ///
    /// Returns whatever the delivery store reports when the deletion statement
    /// fails, typically [`NotificationError::Persistence`].
    pub async fn purge_delivered(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        self.store.delete_delivered_before(cutoff).await
    }

    /// Purge deliveries dead-lettered before an explicit retention cutoff.
    ///
    /// # Errors
    ///
    /// Returns whatever the delivery store reports when the deletion statement
    /// fails, typically [`NotificationError::Persistence`].
    pub async fn purge_exhausted(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        self.store.delete_exhausted_before(cutoff).await
    }

    /// Purge events with no referencing delivery, created before an explicit
    /// retention cutoff.
    ///
    /// # Errors
    ///
    /// Returns whatever the delivery store reports when the deletion statement
    /// fails, typically [`NotificationError::Persistence`].
    pub async fn purge_orphan_events(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        self.store.delete_orphan_events_before(cutoff).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        delivery_backoff::backoff_seconds,
        delivery_claim::{DeliveryClaim, UnreadableDelivery},
        delivery_config::{DeliveryConfig, DeliveryMode},
        delivery_drain::{DeliveryDisposition, NotificationDeliveryDrain, NotificationDeliveryTransport},
        delivery_entry::{DeliveryEntry, DeliveryKey},
        delivery_lease::{DeliveryLeaseError, LeaseTerm},
        delivery_store::DeliveryStore,
        error::NotificationError,
        held_events::HeldEvents,
        notification_event::{NotificationEvent, NotificationEventKey},
    };
    use andromeda_configuration_scalars::human_duration::HumanDuration;
    use andromeda_context_commit::{lease_holder::LeaseHolder, write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use async_trait::async_trait;
    use chrono::{DateTime, TimeDelta, Utc};
    use futures::{
        executor::block_on,
        future::{BoxFuture, poll_fn},
    };
    use std::{
        collections::{BTreeSet, VecDeque},
        fmt::Display,
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        task::Poll,
        thread::sleep,
        time::Duration,
    };

    /// Whether the recorded store still honours the leases it handed out, or
    /// fails every acknowledgement as an unreachable queue would.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    enum LeaseState {
        #[default]
        Held,
        Expired,
        Unrecordable,
    }

    /// One call the drain made on the recorded store, in the order it made it.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum StoreCall {
        /// A claim of at most this many deliveries.
        Claim(i32),
        /// An acknowledgement of this delivery, whatever its outcome.
        Acknowledge(DeliveryKey),
    }

    /// Whether the recorded store serves claims or fails them as an
    /// unreachable queue would.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    enum ClaimState {
        #[default]
        Served,
        Refused,
    }

    #[derive(Default)]
    struct RecordingStore {
        entries: Mutex<VecDeque<DeliveryClaim>>,
        /// Deliveries handed over with the event the pass already held, as a
        /// queue skips reading it.
        reused: Mutex<Vec<DeliveryKey>>,
        delivered: Mutex<Vec<DeliveryKey>>,
        failed: Mutex<Vec<(DeliveryKey, String, DateTime<Utc>)>>,
        terminal: Mutex<Vec<(DeliveryKey, String)>>,
        claimed_with: Mutex<Vec<LeaseTerm>>,
        calls: Mutex<Vec<StoreCall>>,
        lease_state: LeaseState,
        claim_state: ClaimState,
    }

    impl RecordingStore {
        fn with_entry(entry: DeliveryEntry) -> RecordingStore {
            RecordingStore::with_entries([entry])
        }

        fn with_entries(entries: impl IntoIterator<Item = DeliveryEntry>) -> RecordingStore {
            RecordingStore::with_claims(entries.into_iter().map(|entry| DeliveryClaim::Readable(Box::new(entry))))
        }

        fn with_claims(claims: impl IntoIterator<Item = DeliveryClaim>) -> RecordingStore {
            RecordingStore {
                entries: Mutex::new(claims.into_iter().collect()),
                ..RecordingStore::default()
            }
        }

        fn refusing_claims() -> RecordingStore {
            RecordingStore {
                claim_state: ClaimState::Refused,
                ..RecordingStore::default()
            }
        }

        fn with_expired_lease(entry: DeliveryEntry) -> RecordingStore {
            RecordingStore {
                lease_state: LeaseState::Expired,
                ..RecordingStore::with_entry(entry)
            }
        }

        fn with_unrecordable_acknowledgements(entries: impl IntoIterator<Item = DeliveryEntry>) -> RecordingStore {
            RecordingStore {
                lease_state: LeaseState::Unrecordable,
                ..RecordingStore::with_entries(entries)
            }
        }

        fn lease_check(&self, key: DeliveryKey, worker_id: &LeaseHolder) -> Result<(), DeliveryLeaseError> {
            self.calls.lock().map_err(poisoned)?.push(StoreCall::Acknowledge(key));
            match self.lease_state {
                LeaseState::Held => Ok(()),
                LeaseState::Expired => Err(DeliveryLeaseError::lease_lost(key, worker_id)),
                LeaseState::Unrecordable => Err(DeliveryLeaseError::Persistence(PersistenceFailure::Unavailable)),
            }
        }

        /// Hand a claimed delivery over with the event the pass already holds,
        /// or hold its own event for the claims after it.
        fn hand_over(&self, claim: DeliveryClaim, held: &HeldEvents) -> DeliveryClaim {
            match claim {
                DeliveryClaim::Readable(mut entry) => {
                    match held.get(&entry.target, entry.event.key) {
                        Some(event) => {
                            self.reused.lock().expect("the test lock is not poisoned").push(entry.key);
                            entry.event = event;
                        }
                        None => held.hold(&entry.target, &entry.event),
                    }
                    DeliveryClaim::Readable(entry)
                }
                unreadable @ DeliveryClaim::Unreadable(_) => unreadable,
            }
        }

        fn reused_keys(&self) -> Vec<DeliveryKey> {
            self.reused.lock().expect("the test lock is not poisoned").clone()
        }

        fn recorded_calls(&self) -> Vec<StoreCall> {
            self.calls.lock().expect("the call record lock is not poisoned").iter().copied().collect()
        }

        fn claim_widths(&self) -> Vec<i32> {
            self.recorded_calls()
                .into_iter()
                .filter_map(|call| match call {
                    StoreCall::Claim(width) => Some(width),
                    StoreCall::Acknowledge(_) => None,
                })
                .collect()
        }
    }

    /// A poisoned recording lock is a test bug, not a store outcome.
    fn poisoned(error: impl Display) -> DeliveryLeaseError {
        panic!("a recording lock was poisoned: {error}")
    }

    #[async_trait]
    impl DeliveryStore for RecordingStore {
        async fn claim_pending(
            &self,
            _worker_id: &LeaseHolder,
            lease: LeaseTerm,
            batch_size: i32,
            held: &HeldEvents,
        ) -> Result<Vec<DeliveryClaim>, NotificationError> {
            if self.claim_state == ClaimState::Refused {
                return Err(NotificationError::Persistence(PersistenceFailure::Unavailable));
            }
            self.claimed_with.lock().expect("the test lock is not poisoned").push(lease);
            self.calls.lock().expect("the test lock is not poisoned").push(StoreCall::Claim(batch_size));
            let width = usize::try_from(batch_size).expect("a non-negative claim width");
            let mut entries = self.entries.lock().expect("the test lock is not poisoned");
            let taken = width.min(entries.len());
            Ok(entries.drain(..taken).map(|claim| self.hand_over(claim, held)).collect())
        }

        async fn mark_delivered(&self, key: DeliveryKey, worker_id: &LeaseHolder, _target: &ContextTarget) -> Result<(), DeliveryLeaseError> {
            self.lease_check(key, worker_id)?;
            self.delivered.lock().map_err(poisoned)?.push(key);
            Ok(())
        }

        async fn mark_failed(
            &self,
            key: DeliveryKey,
            worker_id: &LeaseHolder,
            error: &str,
            next_attempt_at: DateTime<Utc>,
            _target: &ContextTarget,
        ) -> Result<(), DeliveryLeaseError> {
            self.lease_check(key, worker_id)?;
            self.failed.lock().map_err(poisoned)?.push((key, error.to_owned(), next_attempt_at));
            Ok(())
        }

        async fn mark_terminal(&self, key: DeliveryKey, worker_id: &LeaseHolder, error: &str, _target: &ContextTarget) -> Result<(), DeliveryLeaseError> {
            self.lease_check(key, worker_id)?;
            self.terminal.lock().map_err(poisoned)?.push((key, error.to_owned()));
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
        fn deliver<'a>(&'a self, _entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            // clone: every claimed delivery is answered with the same fixed disposition.
            Box::pin(async move { self.0.clone() })
        }
    }

    /// A transport that can never deliver one particular delivery and
    /// delivers every other.
    struct PoisonedTransport(DeliveryKey);

    impl NotificationDeliveryTransport for PoisonedTransport {
        fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            Box::pin(async move {
                if entry.key == self.0 {
                    DeliveryDisposition::TerminalFailure {
                        detail: "stored payload cannot be evaluated".to_owned(),
                    }
                } else {
                    DeliveryDisposition::Delivered
                }
            })
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
        fn deliver<'a>(&'a self, _entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
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
                DeliveryDisposition::Delivered
            })
        }
    }

    fn config() -> DeliveryConfig {
        DeliveryConfig {
            mode: DeliveryMode::Durable,
            request_timeout: HumanDuration::from_secs(10),
            max_attempts: 5,
            base_backoff: HumanDuration::from_secs(5),
            poll_interval: HumanDuration::from_secs(1),
            poll_interval_max: HumanDuration::from_secs(30),
            batch_size: 1,
            concurrency: 4,
            lease: HumanDuration::from_secs(30),
            error_max_chars: 8,
            cleanup_after: HumanDuration::from_secs(86_400),
            cleanup_interval: HumanDuration::from_secs(300),
            dead_letter_after: HumanDuration::from_secs(604_800),
            orphan_event_after: HumanDuration::from_secs(86_400),
        }
    }

    fn entry() -> Result<DeliveryEntry, NotificationError> {
        let parse = |value: &str| Ok::<Identifier, NotificationError>(value.parse::<Identifier>().expect("a valid test identifier"));
        let event = NotificationEvent::builder()
            .key(NotificationEventKey::from(1))
            .write_id(WriteId::mint())
            .event_ordinal(WriteOrdinal::at(0))
            .change(EntityChange::new(EntityLifecycle::Modified, Vec::new()))
            .entity_id(parse("urn:ngsi-ld:Entity:1")?)
            .created_at(Utc::now())
            .build();
        Ok(DeliveryEntry::builder()
            .target(live_target())
            .key(DeliveryKey::from(1))
            .subscription_id(parse("urn:ngsi-ld:Subscription:1")?)
            .attempt_count(0)
            .next_attempt_at(Utc::now())
            .event(Arc::new(event))
            .build())
    }

    fn live_target() -> ContextTarget {
        ContextTarget::Live
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
                        key: DeliveryKey::from(id),
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
            let delivered = store.delivered.lock().expect("the delivered result lock is not poisoned");
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
            let delivered = store.delivered.lock().expect("the delivered result lock is not poisoned");
            pretty_assertions::assert_eq!(delivered.as_slice(), &[DeliveryKey::from(1)]);
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
            let failed_count = retry_store.failed.lock().expect("the retry result lock is not poisoned").len();
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
            let terminal = terminal_store.terminal.lock().expect("the terminal result lock is not poisoned");
            pretty_assertions::assert_eq!(terminal.len(), 1);
            pretty_assertions::assert_eq!(terminal.first().map(|(id, _)| *id), Some(DeliveryKey::from(1)));
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
            let failed_count = store.failed.lock().expect("the retry result lock is not poisoned").len();
            pretty_assertions::assert_eq!(failed_count, 0, "the final attempt must not be rescheduled");
            let terminal = store.terminal.lock().expect("the terminal result lock is not poisoned");
            pretty_assertions::assert_eq!(
                terminal.first().map(|(id, detail)| (*id, detail.as_str())),
                Some((DeliveryKey::from(1), "transien"))
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
            let terminal_count = store.terminal.lock().expect("the terminal result lock is not poisoned").len();
            pretty_assertions::assert_eq!(terminal_count, 0);
            let failed_count = store.failed.lock().expect("the retry result lock is not poisoned").len();
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
            let failed = store.failed.lock().expect("the retry result lock is not poisoned");
            pretty_assertions::assert_eq!(failed.first().map(|(_, detail, _)| detail.as_str()), Some("endpoint"));
            Ok(())
        })
    }

    #[test]
    fn claims_carry_the_configured_lease_term() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_entry(entry()?));
            drain(store.clone(), DeliveryDisposition::Delivered)?.execute(Utc::now()).await?;
            let claimed_with = store.claimed_with.lock().expect("the claim record lock is not poisoned");
            let expected = LeaseTerm::from_seconds(30).expect("a valid lease term");
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
            let delivered = store.delivered.lock().expect("the delivered result lock is not poisoned");
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
            let failed = store.failed.lock().expect("the retry result lock is not poisoned");
            pretty_assertions::assert_eq!(failed.is_empty(), true);
            Ok(())
        })
    }

    /// The entry template under another delivery key.
    fn entry_keyed(key: i64) -> Result<DeliveryEntry, NotificationError> {
        entry().map(|template| DeliveryEntry {
            key: DeliveryKey::from(key),
            ..template
        })
    }

    /// A row that cannot be read back is still leased to the worker, so it is
    /// retired as exhausted with its reason rather than left to be claimed
    /// again at every lease expiry, and the readable delivery claimed with it
    /// is still delivered.
    #[test]
    fn an_unreadable_claimed_delivery_is_retired_without_holding_back_its_batch() -> Result<(), NotificationError> {
        block_on(async {
            let unreadable = DeliveryClaim::Unreadable(UnreadableDelivery {
                target: live_target(),
                key: DeliveryKey::from(2_i64),
                diagnostic: "notification delivery 2 references invalid entity URI `not a uri`".to_owned(),
            });
            let store = Arc::new(RecordingStore::with_claims([unreadable, DeliveryClaim::Readable(Box::new(entry()?))]));
            let config = DeliveryConfig { batch_size: 2, ..config() };
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), &config)?
                .execute(Utc::now())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 2);
            pretty_assertions::assert_eq!(outcome.lease_lost, 0);
            let terminal = store.terminal.lock().expect("the terminal result lock is not poisoned");
            pretty_assertions::assert_eq!(
                terminal.iter().map(|(key, detail)| (*key, detail.as_str())).collect::<Vec<_>>(),
                vec![(DeliveryKey::from(2_i64), "notifica")],
                "the unreadable row is retired with its reason, bounded like any other diagnostic"
            );
            let delivered = store.delivered.lock().expect("the delivered result lock is not poisoned");
            pretty_assertions::assert_eq!(delivered.as_slice(), &[DeliveryKey::from(1_i64)]);
            Ok(())
        })
    }

    /// A delivery its transport can never evaluate is acknowledged as
    /// exhausted like any terminal failure, and its peers in the same pass are
    /// still delivered and acknowledged.
    #[test]
    fn a_delivery_that_can_never_succeed_does_not_hold_back_its_peers() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_entries([entry_keyed(1)?, entry_keyed(2)?, entry_keyed(3)?]));
            let config = DeliveryConfig { batch_size: 3, ..config() };
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(PoisonedTransport(DeliveryKey::from(2_i64))), &config)?
                .execute(Utc::now())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 3);
            let terminal = store.terminal.lock().expect("the terminal result lock is not poisoned");
            pretty_assertions::assert_eq!(terminal.iter().map(|(key, _)| *key).collect::<Vec<_>>(), vec![DeliveryKey::from(2_i64)]);
            let delivered = store.delivered.lock().expect("the delivered result lock is not poisoned");
            pretty_assertions::assert_eq!(
                delivered.iter().copied().collect::<BTreeSet<_>>(),
                BTreeSet::from([DeliveryKey::from(1_i64), DeliveryKey::from(3_i64)])
            );
            Ok(())
        })
    }

    /// A queue that cannot be claimed from is not a property of any one
    /// delivery, so the pass fails and the supervisor retries it.
    #[test]
    fn a_claim_the_queue_cannot_serve_ends_the_pass() {
        let outcome = block_on(async {
            NotificationDeliveryDrain::new(
                Arc::new(RecordingStore::refusing_claims()),
                Arc::new(FixedTransport(DeliveryDisposition::Delivered)),
                &config(),
            )?
            .execute(Utc::now())
            .await
        });
        assert!(matches!(outcome, Err(NotificationError::Persistence { .. })), "{outcome:?}");
    }

    /// A pass opens by claiming one delivery per slot and refills a slot with a
    /// claim of one delivery each time a delivery is acknowledged, so every
    /// delivery is claimed only when a slot is free to send it.
    #[test]
    fn a_pass_claims_one_delivery_per_free_slot() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=10_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let config = DeliveryConfig { batch_size: 10, ..config() };
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), &config)?
                .execute(Utc::now())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 10);
            assert!(outcome.work_may_remain, "the pass stopped at its batch bound");
            pretty_assertions::assert_eq!(store.claim_widths(), vec![4, 1, 1, 1, 1, 1, 1]);
            Ok(())
        })
    }

    /// No more deliveries are claimed and not yet acknowledged than the pass
    /// has slots, so no claimed delivery waits for a slot with its lease
    /// running.
    #[test]
    fn a_pass_never_holds_more_unacknowledged_claims_than_it_has_slots() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=6_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let config = DeliveryConfig {
                batch_size: 6,
                concurrency: 2,
                ..config()
            };
            NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), &config)?
                .execute(Utc::now())
                .await?;
            let mut outstanding = 0_i32;
            for call in store.recorded_calls() {
                outstanding += match call {
                    StoreCall::Claim(width) => width,
                    StoreCall::Acknowledge(_) => -1,
                };
                assert!(outstanding <= 2, "{outstanding} deliveries were claimed but unacknowledged at once");
            }
            pretty_assertions::assert_eq!(outstanding, 0, "every claimed delivery was acknowledged");
            Ok(())
        })
    }

    /// A slow delivery holds only its own slot: the other slot keeps claiming
    /// and sending while it is in flight, rather than waiting for it.
    #[test]
    fn a_slow_delivery_does_not_hold_back_the_other_slots() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=5_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let config = DeliveryConfig {
                batch_size: 5,
                concurrency: 2,
                ..config()
            };
            NotificationDeliveryDrain::new(store.clone(), Arc::new(SlowOnceTransport(DeliveryKey::from(1_i64))), &config)?
                .execute(Utc::now())
                .await?;
            let acknowledged: Vec<DeliveryKey> = store
                .recorded_calls()
                .into_iter()
                .filter_map(|call| match call {
                    StoreCall::Acknowledge(key) => Some(key),
                    StoreCall::Claim(_) => None,
                })
                .collect();
            pretty_assertions::assert_eq!(acknowledged.len(), 5);
            pretty_assertions::assert_eq!(
                acknowledged.last(),
                Some(&DeliveryKey::from(1_i64)),
                "the other deliveries were sent while the slow one was in flight: {acknowledged:?}"
            );
            Ok(())
        })
    }

    /// Every delivery claimed while delivery 1 is still in flight answers the
    /// same event, so each later claim of the pass finds it held.
    #[test]
    fn the_claims_of_a_pass_find_the_event_a_delivery_in_flight_carries() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=5_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let config = DeliveryConfig {
                batch_size: 5,
                concurrency: 2,
                ..config()
            };
            NotificationDeliveryDrain::new(store.clone(), Arc::new(SlowOnceTransport(DeliveryKey::from(1_i64))), &config)?
                .execute(Utc::now())
                .await?;
            pretty_assertions::assert_eq!(store.reused_keys(), [2, 3, 4, 5].map(DeliveryKey::from));
            Ok(())
        })
    }

    /// With one slot, each delivery is acknowledged before the next is
    /// claimed, so no delivery is left carrying the event and the pass holds
    /// nothing for the next claim.
    #[test]
    fn a_pass_does_not_keep_an_event_once_no_delivery_carries_it() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=3_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let config = DeliveryConfig {
                batch_size: 3,
                concurrency: 1,
                ..config()
            };
            NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), &config)?
                .execute(Utc::now())
                .await?;
            pretty_assertions::assert_eq!(store.claim_widths(), vec![1, 1, 1]);
            assert!(store.reused_keys().is_empty(), "{:?}", store.reused_keys());
            Ok(())
        })
    }

    /// A claim that comes back short means the queue ran out of due
    /// deliveries: the pass claims no more and reports no remaining work.
    #[test]
    fn a_short_claim_ends_the_pass_with_no_work_remaining() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=3_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let config = DeliveryConfig {
                batch_size: 10,
                concurrency: 2,
                ..config()
            };
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), &config)?
                .execute(Utc::now())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 3);
            assert!(!outcome.work_may_remain, "a refill found the queue empty");
            pretty_assertions::assert_eq!(store.claim_widths(), vec![2, 1, 1]);
            Ok(())
        })
    }

    /// An acknowledgement the queue cannot record ends the pass once every
    /// claimed delivery is settled: no further delivery is claimed under a
    /// queue that is failing.
    #[test]
    fn an_acknowledgement_the_queue_cannot_record_claims_no_further_delivery() -> Result<(), NotificationError> {
        let store = Arc::new(RecordingStore::with_unrecordable_acknowledgements(
            (1..=4_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?,
        ));
        let config = DeliveryConfig {
            batch_size: 4,
            concurrency: 2,
            ..config()
        };
        let outcome = block_on(async {
            NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), &config)?
                .execute(Utc::now())
                .await
        });
        assert!(matches!(outcome, Err(NotificationError::Persistence { .. })), "{outcome:?}");
        pretty_assertions::assert_eq!(store.claim_widths(), vec![2]);
        pretty_assertions::assert_eq!(store.recorded_calls().len(), 3, "both claimed deliveries were still acknowledged");
        Ok(())
    }

    /// A retry is due one backoff after the instant it is acknowledged, not
    /// after the start of the pass: a slow delivery would otherwise schedule a
    /// retry that is already due before its pass ends.
    #[test]
    fn a_retry_is_scheduled_from_the_instant_of_its_acknowledgement() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_entry(entry()?));
            let delay = Duration::from_millis(50);
            let now = Utc::now();
            NotificationDeliveryDrain::new(store.clone(), Arc::new(SlowFailingTransport(delay)), &config())?
                .execute(now)
                .await?;
            let failed = store.failed.lock().expect("the retry result lock is not poisoned");
            let earliest = now + TimeDelta::seconds(backoff_seconds(5, 0, 5)) + TimeDelta::from_std(delay).expect("a representable delay");
            let scheduled = failed.first().map(|(_, _, next_attempt_at)| *next_attempt_at);
            assert!(scheduled.is_some_and(|at| at >= earliest), "{scheduled:?} is earlier than {earliest}");
            Ok(())
        })
    }

    /// A transport that delivers every notification, one of them only after
    /// yielding many times, as a slow endpoint would.
    struct SlowOnceTransport(DeliveryKey);

    impl NotificationDeliveryTransport for SlowOnceTransport {
        fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            Box::pin(async move {
                if entry.key == self.0 {
                    let mut remaining = 64_u32;
                    poll_fn(|context| {
                        if remaining == 0 {
                            Poll::Ready(())
                        } else {
                            remaining -= 1;
                            context.waker().wake_by_ref();
                            Poll::Pending
                        }
                    })
                    .await;
                }
                DeliveryDisposition::Delivered
            })
        }
    }

    /// A transport that fails every notification transiently after `.0`.
    struct SlowFailingTransport(Duration);

    impl NotificationDeliveryTransport for SlowFailingTransport {
        fn deliver<'a>(&'a self, _entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            Box::pin(async move {
                sleep(self.0);
                DeliveryDisposition::RetryableFailure {
                    detail: "transient".to_owned(),
                }
            })
        }
    }
}
