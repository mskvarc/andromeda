//! One pass over the notification delivery queue, kept full until the queue
//! holds no more due deliveries.

use crate::{
    delivery_acknowledgement::DeliveryAcknowledgement,
    delivery_attempt_ceiling::DeliveryAttemptCeiling,
    delivery_backoff::retry_due_at,
    delivery_claim::{DeliveryClaim, UnreadableDelivery},
    delivery_claim_order::DeliveryClaimOrder,
    delivery_concurrency::DeliveryConcurrency,
    delivery_entry::DeliveryEntry,
    delivery_error_bound::DeliveryErrorBound,
    delivery_lease::LeaseTerm,
    delivery_policy::{attempt_ceiling, delivery_concurrency, error_bound, lease_term, retry_schedule},
    delivery_queue_supply::QueueSupply,
    delivery_slot_share::{SlotOccupant, SlotShare},
    delivery_store::DeliveryStore,
    delivery_transport::{DeliveryDisposition, NotificationDeliveryTransport},
    error::NotificationError,
    held_events::HeldEvents,
    notification_invariant::NotificationInvariant,
};
use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, lease_pass_clock::LeasePassClock};
use andromeda_retry::{retry_delay::RetryDelay, schedule::BackoffSchedule};
use chrono::{DateTime, Utc};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use std::{
    num::{NonZeroU32, NonZeroUsize},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error};

/// Outcome of one notification delivery pass.
///
/// A pass that completes has found the queue out of due deliveries or was
/// cancelled, so no other pass need follow it at once.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeliveryDrainOutcome {
    /// Number of deliveries claimed during the pass.
    pub claimed: usize,
    /// Number of claimed deliveries whose lease had expired before the worker
    /// acknowledged them, so another worker owns their outcome.
    pub lease_lost: usize,
}

/// Policy operation that advances leased notification deliveries.
pub struct NotificationDeliveryDrain {
    store: Arc<dyn DeliveryStore>,
    transport: Arc<dyn NotificationDeliveryTransport>,
    worker_id: LeaseHolder,
    lease_term: LeaseTerm,
    /// Where the outcome of each delivery the pass leased is recorded.
    acknowledgement: DeliveryAcknowledgement,
    error_bound: DeliveryErrorBound,
    concurrency: DeliveryConcurrency,
    /// How long a Subscription keeps a crowding one below half the slots
    /// after its last delivery was answered: `request_timeout`, the longest a
    /// slot taken meanwhile can stay taken.
    slot_activity: Duration,
    attempt_ceiling: DeliveryAttemptCeiling,
    /// The wait before a failed delivery is due again.
    retry_schedule: BackoffSchedule,
}

/// One thing that finished in a pass's in-flight set.
enum SlotEvent {
    /// A claim that asked for `asked` deliveries in `order` came back.
    Claimed {
        /// Deliveries the claim asked for.
        asked: NonZeroUsize,
        /// The order the claim took them in.
        order: DeliveryClaimOrder,
        /// What the claim leased, or why it could not.
        claims: Result<Vec<DeliveryClaim>, NotificationError>,
    },
    /// A claimed delivery was attempted and its outcome acknowledged.
    Acknowledged {
        /// Whether the outcome was recorded under the lease.
        acknowledgement: Result<LeaseAcknowledgement, NotificationError>,
        /// The Subscription whose slot the delivery freed, `None` for a
        /// delivery whose row could not be read back.
        occupant: Option<SlotOccupant>,
    },
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
    /// Create a bounded delivery operation from validated runtime configuration,
    /// waiting on `delay` between requests to record an outcome.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Invariant`] when the configured retry
    /// schedule, lease term, stored diagnostic bound, in-flight delivery bound
    /// or attempt ceiling is out of range.
    pub fn new(
        store: Arc<dyn DeliveryStore>,
        transport: Arc<dyn NotificationDeliveryTransport>,
        delay: Arc<dyn RetryDelay>,
        config: &DeliveryConfig,
    ) -> Result<NotificationDeliveryDrain, NotificationError> {
        let worker_id = LeaseHolder::mint();
        let retry_schedule = retry_schedule(config).map_err(NotificationInvariant::RetrySchedule)?;
        let lease_term = lease_term(config).map_err(NotificationInvariant::from)?;
        let error_bound = error_bound(config).map_err(NotificationInvariant::from)?;
        let concurrency = delivery_concurrency(config).map_err(NotificationInvariant::from)?;
        let attempt_ceiling = attempt_ceiling(config).map_err(NotificationInvariant::from)?;
        // clone: the acknowledgement records outcomes in the queue the drain claims from.
        let acknowledgement = DeliveryAcknowledgement::new(Arc::clone(&store), worker_id, delay, lease_term);
        Ok(NotificationDeliveryDrain {
            store,
            transport,
            worker_id,
            lease_term,
            acknowledgement,
            error_bound,
            concurrency,
            slot_activity: config.request_timeout.duration(),
            attempt_ceiling,
            retry_schedule,
        })
    }

    /// Execute one leased delivery pass that starts at `now`, until the queue
    /// holds no more due deliveries or `cancellation` fires.
    ///
    /// A pass keeps every one of its slots busy for as long as the queue
    /// supplies due deliveries. It opens by claiming one delivery per slot, and
    /// each time a delivery is acknowledged it claims one more for the slot
    /// that freed. It stops claiming once a claim comes back short, once a
    /// claim or an acknowledgement fails, or once `cancellation` fires, and
    /// returns when the last delivery in flight is acknowledged.
    ///
    /// Once one Subscription holds half the slots, each slot that frees goes
    /// to a due delivery of the Subscription with the fewest in flight, and
    /// while more than one Subscription is active none gets more than half,
    /// so an endpoint that answers late leaves the others at least half the
    /// slots. A slot such a claim leaves empty, because the Subscriptions
    /// below half had nothing due at that instant, is claimed again with the
    /// next slot that frees, so a burst written meanwhile fills every slot
    /// left to it. A lone Subscription still gets every slot, and the slots
    /// wait on a last slow attempt only when nothing else is due.
    ///
    /// A delivery is claimed only when a slot is free to send it, so its lease
    /// starts as its attempt does and has to outlast that one attempt -- which
    /// `request_timeout` bounds -- never the deliveries sent before it. A batch
    /// claimed under one lease and sent a few at a time would instead still be
    /// waiting to be sent when that lease ran out, and a second worker would
    /// claim and send it again.
    ///
    /// The claims of one pass share the events they read: a claim hands over
    /// the event a delivery still in flight already carries instead of reading
    /// it again, so the deliveries of one event, claimed one slot at a time,
    /// cost one event read between them. The acknowledgement of an event's last
    /// delivery in flight lets go of it, so the pass holds no more events than
    /// it has slots, however long it runs.
    ///
    /// Every claimed delivery is driven to its acknowledgement even when
    /// another one fails or the pass is cancelled, so no lease this worker
    /// holds is abandoned mid-flight; the first persistence failure is then
    /// surfaced. A retry is scheduled one backoff after the instant it is
    /// acknowledged, not after the start of the pass.
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
    pub async fn execute(&self, now: DateTime<Utc>, cancellation: &CancellationToken) -> Result<DeliveryDrainOutcome, NotificationError> {
        let clock = LeasePassClock::starting_at(now);
        let held = HeldEvents::default();
        let mut share = SlotShare::new(self.concurrency, self.slot_activity);
        let mut supply = QueueSupply::MayHoldMore;
        let mut idle = 0_usize;
        let mut claimed = 0_usize;
        let mut lease_lost = 0_usize;
        let mut failure: Option<NotificationError> = None;
        let mut slots: FuturesUnordered<BoxFuture<'_, SlotEvent>> = FuturesUnordered::new();
        if !cancellation.is_cancelled() {
            slots.push(self.claim(self.concurrency.get(), share.claim_order(Instant::now()), &clock, &held).boxed());
        }
        while let Some(event) = slots.next().await {
            match event {
                SlotEvent::Claimed {
                    asked,
                    order,
                    claims: Ok(claims),
                } => {
                    supply = supply.after_claim(asked, order, claims.len());
                    idle += asked.get().saturating_sub(claims.len());
                    claimed += claims.len();
                    for claim in &claims {
                        match claim {
                            DeliveryClaim::Readable(entry) => share.occupy(&entry.target, &entry.subscription_id),
                            DeliveryClaim::Unreadable(_) => {}
                        }
                    }
                    slots.extend(claims.into_iter().map(|claim| self.attempt(claim, &clock, &held).boxed()));
                }
                SlotEvent::Claimed { claims: Err(error), .. } => {
                    failure.get_or_insert(error);
                }
                SlotEvent::Acknowledged { acknowledgement, occupant } => {
                    if let Some(occupant) = occupant {
                        share.release(occupant, Instant::now());
                    }
                    match acknowledgement {
                        Ok(LeaseAcknowledgement::Applied) => {}
                        Ok(LeaseAcknowledgement::Lost) => lease_lost += 1,
                        Err(error) => {
                            failure.get_or_insert(error);
                        }
                    }
                    // The acknowledged delivery freed its slot, beside those earlier claims left empty.
                    if failure.is_none() && supply == QueueSupply::MayHoldMore && !cancellation.is_cancelled() {
                        let width = NonZeroUsize::MIN.saturating_add(idle);
                        idle = 0;
                        slots.push(self.claim(width, share.claim_order(Instant::now()), &clock, &held).boxed());
                    }
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(DeliveryDrainOutcome { claimed, lease_lost }),
        }
    }

    /// Claim up to `width` deliveries in `order` under a lease that starts
    /// now, reusing the events the pass's in-flight deliveries hold.
    async fn claim(&self, width: NonZeroUsize, order: DeliveryClaimOrder, clock: &LeasePassClock, held: &HeldEvents) -> SlotEvent {
        let claims = match NonZeroU32::try_from(width) {
            Ok(claim_width) => self.store.claim_pending(&self.worker_id, self.lease_term, claim_width, order, held).await,
            Err(error) => Err(NotificationInvariant::ClaimWidth(error).into()),
        };
        if let Ok(claimed) = &claims {
            debug!(
                count = claimed.len(),
                ?order,
                worker_id = %self.worker_id,
                lease_expires_at = ?self.lease_term.expires_at(clock.now()),
                "Claimed notification deliveries"
            );
        }
        SlotEvent::Claimed { asked: width, order, claims }
    }

    /// Attempt one claimed delivery, acknowledge its outcome, and let go of
    /// the event it carried and the slot it held.
    async fn attempt(&self, claim: DeliveryClaim, clock: &LeasePassClock, held: &HeldEvents) -> SlotEvent {
        match claim {
            DeliveryClaim::Readable(entry) => {
                trace_entry(&entry);
                let acknowledgement = self.advance(&entry, clock).await;
                let DeliveryEntry {
                    target,
                    event,
                    subscription_id,
                    ..
                } = *entry;
                held.release(&target, event);
                SlotEvent::Acknowledged {
                    acknowledgement,
                    occupant: Some(SlotOccupant::new(target, subscription_id)),
                }
            }
            DeliveryClaim::Unreadable(unreadable) => SlotEvent::Acknowledged {
                acknowledgement: self.retire_unreadable(&unreadable, clock).await,
                occupant: None,
            },
        }
    }

    /// Deliver one claimed entry and acknowledge whatever the transport reported.
    ///
    /// The attempt ceiling is applied here: a retryable failure on the attempt
    /// that reaches the ceiling is acknowledged as exhausted, exactly like a
    /// terminal failure, so the store never has to know the ceiling to keep an
    /// exhausted delivery out of the claimable set.
    ///
    /// A retryable failure holds the rest of the Subscription's queue behind
    /// the failed delivery, which the store does with the acknowledgement, and
    /// an exhausted probe hands the hold to a delivery due one first retry step
    /// later, as a delivery failing for the first time would be.
    async fn advance(&self, entry: &DeliveryEntry, clock: &LeasePassClock) -> Result<LeaseAcknowledgement, NotificationError> {
        match self.transport.deliver(entry).await {
            DeliveryDisposition::Delivered | DeliveryDisposition::Obsolete => self.acknowledgement.delivered(entry.key, &entry.target).await,
            DeliveryDisposition::RetryableFailure { detail } if !self.attempt_ceiling.is_final_attempt(entry.attempt_count) => {
                let next_attempt = retry_due_at(&self.retry_schedule, entry.attempt_count, clock.now()).map_err(NotificationInvariant::AttemptCount)?;
                let detail = self.error_bound.truncate(&detail);
                self.acknowledgement.failed(entry.key, &detail, next_attempt, &entry.target).await
            }
            DeliveryDisposition::RetryableFailure { detail } | DeliveryDisposition::TerminalFailure { detail } => {
                let detail = self.error_bound.truncate(&detail);
                self.acknowledgement
                    .exhausted(entry.key, &detail, self.next_probe_at(clock)?, &entry.target)
                    .await
            }
        }
    }

    /// The instant the next probe of a held Subscription is due when the
    /// delivery being exhausted was its probe: one first retry step from now.
    fn next_probe_at(&self, clock: &LeasePassClock) -> Result<DateTime<Utc>, NotificationError> {
        Ok(retry_due_at(&self.retry_schedule, 0, clock.now()).map_err(NotificationInvariant::AttemptCount)?)
    }

    /// Retire a claimed delivery whose stored row could not be read back.
    ///
    /// Nothing a later attempt does can repair a stored value, so the delivery
    /// is acknowledged as exhausted with the reason it could not be read as its
    /// recorded failure, exactly like a delivery whose transport reported a
    /// terminal failure.
    async fn retire_unreadable(&self, unreadable: &UnreadableDelivery, clock: &LeasePassClock) -> Result<LeaseAcknowledgement, NotificationError> {
        error!(
            delivery_key = %unreadable.key,
            error = %unreadable.diagnostic,
            event = "notification_delivery_unreadable"
        );
        let detail = self.error_bound.truncate(&unreadable.diagnostic);
        self.acknowledgement
            .exhausted(unreadable.key, &detail, self.next_probe_at(clock)?, &unreadable.target)
            .await
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
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        delivery_acknowledgement::acknowledgement_retry,
        delivery_claim::{DeliveryClaim, UnreadableDelivery},
        delivery_claim_order::{DeliveryClaimOrder, InFlightCeiling},
        delivery_drain::NotificationDeliveryDrain,
        delivery_entry::{DeliveryEntry, DeliveryKey},
        delivery_lease::{DeliveryLeaseError, LeaseTerm},
        delivery_policy::retry_schedule,
        delivery_store::DeliveryStore,
        delivery_transport::{DeliveryDisposition, NotificationDeliveryTransport},
        error::NotificationError,
        held_events::HeldEvents,
        notification_event::{NotificationEvent, NotificationEventKey},
    };
    use andromeda_configuration::{
        human_duration::HumanDuration,
        schema::notifications::delivery::{DeliveryConfig, DeliveryMode},
    };
    use andromeda_context_commit::{lease_holder::LeaseHolder, store_instant::StoreInstant, write_id::WriteId, write_ordinal::WriteOrdinal};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::entity_change::{EntityChange, EntityLifecycle};
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use andromeda_retry::{retry_delay::RetryDelay, retry_number::RetryNumber};
    use async_trait::async_trait;
    use chrono::{DateTime, TimeDelta, Utc};
    use futures::{
        executor::block_on,
        future::{BoxFuture, poll_fn},
    };
    use std::{
        collections::{BTreeSet, VecDeque},
        fmt::Display,
        num::NonZeroU32,
        sync::{
            Arc,
            Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        task::Poll,
        thread::sleep,
        time::Duration,
    };
    use tokio_util::sync::CancellationToken;

    /// Whether the recorded store still honours the leases it handed out, or
    /// fails every acknowledgement as an unreachable queue would.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    enum LeaseState {
        #[default]
        Held,
        Expired,
        Unrecordable,
        /// The first acknowledgement fails as an unreachable queue would, and
        /// every later one is recorded.
        UnrecordableOnce,
    }

    /// One call the drain made on the recorded store, in the order it made it.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum StoreCall {
        /// A claim that asked for at most `asked` deliveries and was granted
        /// `granted` of them.
        Claim {
            /// Deliveries the claim asked for.
            asked: u32,
            /// Deliveries the queue handed over.
            granted: u32,
        },
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
        /// The next probe instant each terminal acknowledgement carried.
        next_probes: Mutex<Vec<DateTime<Utc>>>,
        claimed_with: Mutex<Vec<LeaseTerm>>,
        /// The order each claim asked for, in the order the claims were made.
        orders: Mutex<Vec<DeliveryClaimOrder>>,
        calls: Mutex<Vec<StoreCall>>,
        lease_state: LeaseState,
        claim_state: ClaimState,
        /// Whether an [`LeaseState::UnrecordableOnce`] store has refused its
        /// one acknowledgement yet.
        refused_once: AtomicBool,
        /// The most events the pass held at the end of any claim.
        peak_held: AtomicUsize,
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

        fn with_one_unrecordable_acknowledgement(entry: DeliveryEntry) -> RecordingStore {
            RecordingStore {
                lease_state: LeaseState::UnrecordableOnce,
                ..RecordingStore::with_entry(entry)
            }
        }

        fn lease_check(&self, key: DeliveryKey, worker_id: &LeaseHolder) -> Result<(), DeliveryLeaseError> {
            self.calls.lock().map_err(poisoned)?.push(StoreCall::Acknowledge(key));
            match self.lease_state {
                LeaseState::Expired => Err(DeliveryLeaseError::lease_lost(key, worker_id)),
                LeaseState::Unrecordable => Err(DeliveryLeaseError::Persistence(PersistenceFailure::Unavailable)),
                LeaseState::UnrecordableOnce if !self.refused_once.swap(true, Ordering::SeqCst) => {
                    Err(DeliveryLeaseError::Persistence(PersistenceFailure::Unavailable))
                }
                LeaseState::Held | LeaseState::UnrecordableOnce => Ok(()),
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

        fn acknowledged_keys(&self) -> Vec<DeliveryKey> {
            self.recorded_calls()
                .into_iter()
                .filter_map(|call| match call {
                    StoreCall::Acknowledge(key) => Some(key),
                    StoreCall::Claim { .. } => None,
                })
                .collect()
        }

        fn claim_widths(&self) -> Vec<u32> {
            self.recorded_calls()
                .into_iter()
                .filter_map(|call| match call {
                    StoreCall::Claim { asked, .. } => Some(asked),
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
            batch_size: NonZeroU32,
            order: DeliveryClaimOrder,
            held: &HeldEvents,
        ) -> Result<Vec<DeliveryClaim>, NotificationError> {
            if self.claim_state == ClaimState::Refused {
                return Err(NotificationError::Persistence(PersistenceFailure::Unavailable));
            }
            self.claimed_with.lock().expect("the test lock is not poisoned").push(lease);
            self.orders.lock().expect("the test lock is not poisoned").push(order);
            let width = usize::try_from(batch_size.get()).expect("a claim width within the address width");
            let mut entries = self.entries.lock().expect("the test lock is not poisoned");
            let taken = width.min(entries.len());
            self.calls.lock().expect("the test lock is not poisoned").push(StoreCall::Claim {
                asked: batch_size.get(),
                granted: u32::try_from(taken).expect("a granted count within the claim width"),
            });
            let claims = entries.drain(..taken).map(|claim| self.hand_over(claim, held)).collect();
            self.peak_held.fetch_max(held.len(), Ordering::SeqCst);
            Ok(claims)
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

        async fn mark_terminal(
            &self,
            key: DeliveryKey,
            worker_id: &LeaseHolder,
            error: &str,
            next_probe_at: DateTime<Utc>,
            _target: &ContextTarget,
        ) -> Result<(), DeliveryLeaseError> {
            self.lease_check(key, worker_id)?;
            self.terminal.lock().map_err(poisoned)?.push((key, error.to_owned()));
            self.next_probes.lock().map_err(poisoned)?.push(next_probe_at);
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

    /// A wait that returns at once and records what it was asked for.
    #[derive(Default)]
    struct RecordedDelay(Mutex<Vec<Duration>>);

    impl RecordedDelay {
        fn waits(&self) -> Vec<Duration> {
            self.0.lock().expect("the wait record lock is not poisoned").clone()
        }
    }

    impl RetryDelay for RecordedDelay {
        fn wait<'life0, 'async_trait>(&'life0 self, delay: Duration) -> BoxFuture<'async_trait, ()>
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            self.0.lock().expect("the wait record lock is not poisoned").push(delay);
            Box::pin(async {})
        }
    }

    /// A wait between requests to record an outcome that returns at once.
    fn waits() -> Arc<dyn RetryDelay> {
        Arc::new(RecordedDelay::default())
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
        sized(4)
    }

    /// The durable delivery table with `concurrent` deliveries in flight at
    /// once.
    fn sized(concurrent: u32) -> DeliveryConfig {
        let mut config = DeliveryConfig {
            mode: DeliveryMode::Durable,
            request_timeout: HumanDuration::from_secs(10),
            max_concurrent: NonZeroU32::new(concurrent).expect("a positive bound"),
            max_error_length: NonZeroU32::new(8).expect("a positive bound"),
            ..DeliveryConfig::default()
        };
        config.queue.retry_interval = HumanDuration::from_secs(5);
        config
    }

    fn entry() -> Result<DeliveryEntry, NotificationError> {
        entry_of_event(1)
    }

    /// A delivery under key 1 of the event under `event_key`.
    fn entry_of_event(event_key: i64) -> Result<DeliveryEntry, NotificationError> {
        let parse = |value: &str| Ok::<Identifier, NotificationError>(value.parse::<Identifier>().expect("a valid test identifier"));
        let event = NotificationEvent::builder()
            .key(NotificationEventKey::from(event_key))
            .write_id(WriteId::mint())
            .event_ordinal(WriteOrdinal::at(0))
            .change(EntityChange::new(EntityLifecycle::Modified, Vec::new()))
            .entity_id(parse("urn:ngsi-ld:Entity:1")?)
            .created_at(Utc::now())
            .committed_at(StoreInstant::new(Utc::now()))
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
        NotificationDeliveryDrain::new(store, Arc::new(FixedTransport(disposition)), waits(), &config())
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
            let config = sized(4);
            let outcome = NotificationDeliveryDrain::new(store.clone(), transport.clone(), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
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
            let outcome = drain(store.clone(), DeliveryDisposition::Delivered)?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
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
            .execute(Utc::now(), &CancellationToken::new())
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
            .execute(Utc::now(), &CancellationToken::new())
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
            .execute(Utc::now(), &CancellationToken::new())
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
            .execute(Utc::now(), &CancellationToken::new())
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
            .execute(Utc::now(), &CancellationToken::new())
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
            drain(store.clone(), DeliveryDisposition::Delivered)?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
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
            let outcome = drain(store.clone(), DeliveryDisposition::Delivered)?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
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
            .execute(Utc::now(), &CancellationToken::new())
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
            let config = sized(4);
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
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
            let config = sized(4);
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(PoisonedTransport(DeliveryKey::from(2_i64))), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
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
                waits(),
                &config(),
            )?
            .execute(Utc::now(), &CancellationToken::new())
            .await
        });
        assert!(matches!(outcome, Err(NotificationError::Persistence { .. })), "{outcome:?}");
    }

    /// A pass opens by claiming one delivery per slot and refills a slot with a
    /// claim of one delivery each time a delivery is acknowledged, so every
    /// delivery is claimed only when a slot is free to send it, until a claim
    /// finds the queue empty.
    #[test]
    fn a_pass_claims_one_delivery_per_free_slot_until_the_queue_runs_out() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=10_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let config = sized(4);
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 10);
            let widths = store.claim_widths();
            pretty_assertions::assert_eq!(widths.first(), Some(&4));
            assert!(widths.iter().skip(1).all(|width| *width == 1), "{widths:?}");
            assert!(store.entries.lock().expect("the test lock is not poisoned").is_empty());
            pretty_assertions::assert_eq!(store.delivered.lock().expect("the delivered result lock is not poisoned").len(), 10);
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
            let config = sized(2);
            NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            let mut outstanding = 0_i64;
            for call in store.recorded_calls() {
                outstanding += match call {
                    StoreCall::Claim { granted, .. } => i64::from(granted),
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
            let config = sized(2);
            NotificationDeliveryDrain::new(store.clone(), Arc::new(SlowOnceTransport(DeliveryKey::from(1_i64))), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            let acknowledged = store.acknowledged_keys();
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
            let config = sized(2);
            NotificationDeliveryDrain::new(store.clone(), Arc::new(SlowOnceTransport(DeliveryKey::from(1_i64))), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
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
            let config = sized(1);
            NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(store.claim_widths(), vec![1, 1, 1, 1]);
            assert!(store.reused_keys().is_empty(), "{:?}", store.reused_keys());
            Ok(())
        })
    }

    /// A claim that comes back short means the queue ran out of due
    /// deliveries: the pass claims no more and ends once the deliveries in
    /// flight are acknowledged.
    #[test]
    fn a_short_claim_ends_the_pass() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=3_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let config = sized(2);
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 3);
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
        let config = sized(2);
        let outcome = block_on(async {
            NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &config)?
                .execute(Utc::now(), &CancellationToken::new())
                .await
        });
        assert!(matches!(outcome, Err(NotificationError::Persistence { .. })), "{outcome:?}");
        pretty_assertions::assert_eq!(store.claim_widths(), vec![2]);
        let requests =
            usize::try_from(acknowledgement_retry(LeaseTerm::from_seconds(30).expect("a valid lease term")).attempts.get()).expect("a representable count");
        pretty_assertions::assert_eq!(
            store.recorded_calls().len(),
            1 + 2 * requests,
            "both claimed deliveries were still acknowledged, each as often as the lease leaves room for"
        );
        Ok(())
    }

    /// An outcome the queue failed to record would leave the delivery leased,
    /// to be claimed and sent again once its lease lapsed; the worker asks
    /// again after the first backoff wait, and the notification the endpoint
    /// accepted is recorded as delivered.
    #[test]
    fn a_delivered_outcome_the_queue_failed_to_record_is_recorded_on_a_later_request() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_one_unrecordable_acknowledgement(entry()?));
            let delay = Arc::new(RecordedDelay::default());
            let outcome = NotificationDeliveryDrain::new(
                store.clone(),
                Arc::new(FixedTransport(DeliveryDisposition::Delivered)),
                Arc::clone(&delay) as Arc<dyn RetryDelay>,
                &config(),
            )?
            .execute(Utc::now(), &CancellationToken::new())
            .await?;
            pretty_assertions::assert_eq!((outcome.claimed, outcome.lease_lost), (1, 0));
            pretty_assertions::assert_eq!(
                store.delivered.lock().expect("the delivered result lock is not poisoned").as_slice(),
                &[DeliveryKey::from(1_i64)]
            );
            pretty_assertions::assert_eq!(delay.waits(), vec![Duration::from_millis(100)]);
            Ok(())
        })
    }

    /// A lost lease is the queue's answer, not a failure to record: another
    /// worker owns the delivery, so the outcome is not asked again.
    #[test]
    fn a_lost_lease_is_not_asked_again() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_expired_lease(entry()?));
            let delay = Arc::new(RecordedDelay::default());
            NotificationDeliveryDrain::new(
                store.clone(),
                Arc::new(FixedTransport(DeliveryDisposition::Delivered)),
                Arc::clone(&delay) as Arc<dyn RetryDelay>,
                &config(),
            )?
            .execute(Utc::now(), &CancellationToken::new())
            .await?;
            pretty_assertions::assert_eq!(
                store.recorded_calls(),
                vec![StoreCall::Claim { asked: 4, granted: 1 }, StoreCall::Acknowledge(DeliveryKey::from(1_i64))]
            );
            pretty_assertions::assert_eq!(delay.waits(), Vec::<Duration>::new());
            Ok(())
        })
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
            NotificationDeliveryDrain::new(store.clone(), Arc::new(SlowFailingTransport(delay)), waits(), &config())?
                .execute(now, &CancellationToken::new())
                .await?;
            let failed = store.failed.lock().expect("the retry result lock is not poisoned");
            let backoff = retry_schedule(&config()).expect("a valid retry schedule").delay(RetryNumber::FIRST);
            let earliest = now + TimeDelta::from_std(backoff + delay).expect("a representable delay");
            let scheduled = failed.first().map(|(_, _, next_attempt_at)| *next_attempt_at);
            assert!(scheduled.is_some_and(|at| at >= earliest), "{scheduled:?} is earlier than {earliest}");
            Ok(())
        })
    }

    /// An exhausted delivery may be its Subscription's probe, so the store is
    /// told when the next probe is due: one first retry step after the
    /// exhaustion, as for a delivery failing for the first time.
    #[test]
    fn an_exhausted_delivery_names_the_next_probe_one_first_retry_step_away() -> Result<(), NotificationError> {
        block_on(async {
            let mut final_attempt = entry()?;
            final_attempt.attempt_count = 4;
            let store = Arc::new(RecordingStore::with_entry(final_attempt));
            let now = Utc::now();
            drain(
                store.clone(),
                DeliveryDisposition::RetryableFailure {
                    detail: "transient".to_owned(),
                },
            )?
            .execute(now, &CancellationToken::new())
            .await?;
            let first_step = retry_schedule(&config()).expect("a valid retry schedule").delay(RetryNumber::FIRST);
            let earliest = now + TimeDelta::from_std(first_step).expect("a representable delay");
            let probes = store.next_probes.lock().expect("the probe record lock is not poisoned").clone();
            pretty_assertions::assert_eq!(probes.len(), 1);
            assert!(
                probes.iter().all(|at| *at >= earliest && *at < earliest + TimeDelta::seconds(5)),
                "{probes:?} against {earliest}"
            );
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

    /// One delivery to an endpoint that never answers holds its slot until it
    /// times out; the other slots claim and send the rest of the queue
    /// meanwhile, so it is the last one acknowledged.
    #[test]
    fn a_delivery_held_until_its_timeout_does_not_hold_back_the_rest_of_the_queue() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=1_000_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let held = DeliveryKey::from(1_i64);
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(HeldOnceTransport(held)), waits(), &sized(4))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 1_000);
            let acknowledged = store.acknowledged_keys();
            pretty_assertions::assert_eq!(acknowledged.len(), 1_000);
            pretty_assertions::assert_eq!(acknowledged.last(), Some(&held), "the held delivery was not the last acknowledged");
            pretty_assertions::assert_eq!(store.delivered.lock().expect("the delivered result lock is not poisoned").len(), 999);
            let failed = store.failed.lock().expect("the retry result lock is not poisoned");
            pretty_assertions::assert_eq!(failed.iter().map(|(key, _, _)| *key).collect::<Vec<_>>(), vec![held]);
            Ok(())
        })
    }

    /// Cancelling the drain during a pass claims nothing further, and every
    /// delivery already in flight is still acknowledged before the pass
    /// returns.
    #[test]
    fn a_cancelled_pass_claims_nothing_further_and_acknowledges_what_is_in_flight() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=10_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let cancellation = CancellationToken::new();
            // clone: the transport cancels the token the pass watches.
            let transport = Arc::new(CancellingTransport(cancellation.clone()));
            let outcome = NotificationDeliveryDrain::new(store.clone(), transport, waits(), &sized(2))?
                .execute(Utc::now(), &cancellation)
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 2);
            pretty_assertions::assert_eq!(store.claim_widths(), vec![2]);
            pretty_assertions::assert_eq!(store.acknowledged_keys().len(), 2);
            pretty_assertions::assert_eq!(store.entries.lock().expect("the test lock is not poisoned").len(), 8);
            Ok(())
        })
    }

    #[test]
    fn a_drain_cancelled_before_its_pass_claims_nothing() -> Result<(), NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_entry(entry()?));
            let cancellation = CancellationToken::new();
            cancellation.cancel();
            let outcome = drain(store.clone(), DeliveryDisposition::Delivered)?.execute(Utc::now(), &cancellation).await?;
            pretty_assertions::assert_eq!((outcome.claimed, outcome.lease_lost), (0, 0));
            pretty_assertions::assert_eq!(store.recorded_calls(), Vec::<StoreCall>::new());
            Ok(())
        })
    }

    /// The acknowledgement of an event's last delivery lets go of it, so a
    /// pass over many distinct events holds no more of them than it has slots.
    #[test]
    fn a_pass_holds_no_more_events_than_it_has_slots() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=10_000_i64)
                .map(|key| {
                    entry_of_event(key).map(|template| DeliveryEntry {
                        key: DeliveryKey::from(key),
                        ..template
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_entries(entries));
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &sized(4))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 10_000);
            pretty_assertions::assert_eq!(store.delivered.lock().expect("the delivered result lock is not poisoned").len(), 10_000);
            let peak = store.peak_held.load(Ordering::SeqCst);
            assert!(peak <= 4, "the pass held {peak} events with 4 slots");
            Ok(())
        })
    }

    /// The polls a held delivery stays unanswered for, the transport double's
    /// stand-in for `request_timeout`: far more than the rest of the queue
    /// takes to drain through the other slots.
    const HELD_POLLS: u32 = 100_000;

    /// A transport that answers every delivery at once except one, which it
    /// holds unanswered for [`HELD_POLLS`] polls and then reports timed out.
    struct HeldOnceTransport(DeliveryKey);

    impl NotificationDeliveryTransport for HeldOnceTransport {
        fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            Box::pin(async move {
                if entry.key != self.0 {
                    return DeliveryDisposition::Delivered;
                }
                let mut remaining = HELD_POLLS;
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
                DeliveryDisposition::RetryableFailure {
                    detail: "timed out".to_owned(),
                }
            })
        }
    }

    /// A transport that cancels the drain as it sends each delivery, and
    /// delivers it.
    struct CancellingTransport(CancellationToken);

    impl NotificationDeliveryTransport for CancellingTransport {
        fn deliver<'a>(&'a self, _entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            Box::pin(async move {
                self.0.cancel();
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

    /// Ten deliveries keyed from 1, each to the Subscription `subscription`
    /// names for its key.
    fn entries_to(subscription: impl Fn(i64) -> String) -> Result<Vec<DeliveryEntry>, NotificationError> {
        (1..=10_i64)
            .map(|id| {
                let subscription_id = subscription(id).parse::<Identifier>().expect("a valid test identifier");
                entry().map(|template| DeliveryEntry {
                    key: DeliveryKey::from(id),
                    subscription_id,
                    ..template
                })
            })
            .collect()
    }

    fn claim_orders(entries: Vec<DeliveryEntry>) -> Result<Vec<DeliveryClaimOrder>, NotificationError> {
        block_on(async {
            let store = Arc::new(RecordingStore::with_entries(entries));
            NotificationDeliveryDrain::new(store.clone(), Arc::new(InFlightTransport::default()), waits(), &sized(4))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            Ok(store.orders.lock().expect("the test lock is not poisoned").clone())
        })
    }

    #[test]
    fn a_subscription_holding_half_the_slots_turns_the_next_refill_to_fewest_in_flight() -> Result<(), NotificationError> {
        let orders = claim_orders(entries_to(|_| "urn:ngsi-ld:Subscription:slow".to_owned())?)?;
        pretty_assertions::assert_eq!(orders.first(), Some(&DeliveryClaimOrder::Queue));
        // The first refill comes with three of the four slots still busy with the one Subscription, and no other.
        pretty_assertions::assert_eq!(orders.get(1), Some(&DeliveryClaimOrder::FewestInFlight(InFlightCeiling::Lifted)));
        Ok(())
    }

    #[test]
    fn deliveries_spread_over_subscriptions_are_claimed_in_queue_order() -> Result<(), NotificationError> {
        let orders = claim_orders(entries_to(|id| format!("urn:ngsi-ld:Subscription:s{id}"))?)?;
        assert!(orders.iter().all(|order| *order == DeliveryClaimOrder::Queue), "{orders:?}");
        Ok(())
    }
}
