//! One pass over the notification delivery queue, kept full until the queue
//! holds no more due deliveries.

use crate::{
    delivery_acknowledgement::DeliveryAcknowledgement,
    delivery_attempt::DeliveryAttempt,
    delivery_claim::DeliveryClaim,
    delivery_concurrency::DeliveryConcurrency,
    delivery_entry::DeliveryEntry,
    delivery_lease::LeaseTerm,
    delivery_policy::{delivery_concurrency, lease_term},
    delivery_queue_supply::QueueSupply,
    delivery_round::{ClaimRequest, SettledRound},
    delivery_round_plan::{Refill, Round, RoundPlan},
    delivery_settlement::DeliverySettlement,
    delivery_slot_event::{AttemptEnded, AwaitingAcknowledgement, ClaimAsked, RoundEnded, SlotEvent},
    delivery_slot_share::{SlotOccupant, SlotShare},
    delivery_store::DeliveryStore,
    delivery_transport::NotificationDeliveryTransport,
    error::NotificationError,
    held_events::HeldEvents,
    notification_invariant::NotificationInvariant,
};
use andromeda_configuration::schema::notifications::delivery::DeliveryConfig;
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder, lease_pass_clock::LeasePassClock};
use andromeda_retry::retry_delay::RetryDelay;
use chrono::{DateTime, Utc};
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use std::{
    num::NonZeroU32,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use tracing::debug;

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
    /// The delivery queue claimed from and pruned.
    store: Arc<dyn DeliveryStore>,
    /// Sends each claimed delivery and judges its outcome.
    attempts: DeliveryAttempt,
    /// The lease holder this drain claims deliveries as.
    worker_id: LeaseHolder,
    /// How long a claimed delivery stays leased to this drain.
    lease_term: LeaseTerm,
    /// Where the outcome of each delivery the pass leased is recorded, and
    /// its slot refilled.
    acknowledgement: DeliveryAcknowledgement,
    /// The number of deliveries in flight at once, one per slot.
    concurrency: DeliveryConcurrency,
    /// How long a Subscription keeps a crowding one below half the slots
    /// after its last delivery was answered: `request_timeout`, the longest a
    /// slot taken meanwhile can stay taken.
    slot_activity: Duration,
}

/// Release the slot of a delivery that left flight from its Subscription's
/// share, when it was read back.
fn release_share(share: &mut SlotShare, occupant: Option<SlotOccupant>) {
    if let Some(occupant) = occupant {
        share.release(occupant, Instant::now());
    }
}

impl NotificationDeliveryDrain {
    /// Create a bounded delivery operation from validated runtime configuration,
    /// waiting on `delay` between requests to record an outcome.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError::Invariant`] when the configured retry
    /// schedule, stored diagnostic bound, attempt ceiling, lease term or
    /// in-flight delivery bound is out of range.
    pub fn new(
        store: Arc<dyn DeliveryStore>,
        transport: Arc<dyn NotificationDeliveryTransport>,
        delay: Arc<dyn RetryDelay>,
        config: &DeliveryConfig,
    ) -> Result<NotificationDeliveryDrain, NotificationError> {
        let worker_id = LeaseHolder::mint();
        let attempts = DeliveryAttempt::new(transport, config)?;
        let lease_term = lease_term(config).map_err(NotificationInvariant::from)?;
        let concurrency = delivery_concurrency(config).map_err(NotificationInvariant::from)?;
        let acknowledgement = DeliveryAcknowledgement::new(Arc::clone(&store), worker_id, delay, lease_term);
        Ok(NotificationDeliveryDrain {
            store,
            attempts,
            worker_id,
            lease_term,
            acknowledgement,
            concurrency,
            slot_activity: config.request_timeout.duration(),
        })
    }

    /// Execute one leased delivery pass that starts at `now`, until the queue
    /// holds no more due deliveries or `cancellation` fires.
    ///
    /// A pass keeps every one of its slots busy for as long as the queue
    /// supplies due deliveries. It goes to the queue in rounds, and has at
    /// most one round in flight: a round records the outcomes of the
    /// deliveries whose attempts ended, together or while the previous round
    /// was in flight, and claims every slot free once they are recorded, their
    /// own slots included, in one transaction. The pass opens with a round
    /// that only claims, one delivery per slot. Endpoints that answer at once
    /// cost one round per batch of attempts, not an acknowledgement and a
    /// claim per delivery, and a lone outcome with no round in flight is still
    /// recorded at once. The pass stops claiming once a claim comes back
    /// short, once a round fails, or once `cancellation` fires, then only
    /// acknowledges, and returns when the last delivery in flight is
    /// acknowledged.
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
    /// starts as its attempt does and has to outlast that one attempt (which
    /// `request_timeout` bounds), never the deliveries sent before it. A batch
    /// claimed under one lease and sent a few at a time would instead still be
    /// waiting to be sent when that lease ran out, and a second worker would
    /// claim and send it again.
    ///
    /// The claims of one pass share the events they read: a claim hands over
    /// the event a delivery still in flight already carries instead of reading
    /// it again, so the deliveries of one event, claimed one slot at a time,
    /// cost one event read between them. The attempt at an event's last
    /// delivery in flight lets go of it, so the pass holds no more events than
    /// it has slots, however long it runs.
    ///
    /// Every claimed delivery is driven to its acknowledgement even when
    /// another one fails or the pass is cancelled, so no lease this worker
    /// holds is abandoned mid-flight; the first persistence failure is then
    /// surfaced. A retry is scheduled one backoff after the instant its
    /// attempt ends, not after the start of the pass.
    ///
    /// What one delivery does never ends the pass: a delivery whose row could
    /// not be read back, or whose transport reports it can never succeed, is
    /// acknowledged as exhausted with its diagnostic, because a delivery left
    /// leased and unacknowledged would be claimed again at every lease expiry
    /// with its attempt count unchanged, forever. Only the queue failing the
    /// worker ends a pass (a claim it cannot serve, an acknowledgement it
    /// cannot record), because that is not a property of any one delivery and
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
        let mut plan = RoundPlan::new(self.concurrency.get());
        let mut claimed = 0_usize;
        let mut lease_lost = 0_usize;
        let mut failure: Option<NotificationError> = None;
        let mut slots: FuturesUnordered<BoxFuture<'_, SlotEvent>> = FuturesUnordered::new();
        loop {
            let refill = match (&failure, supply, cancellation.is_cancelled()) {
                (None, QueueSupply::MayHoldMore, false) => Refill::Allowed,
                (Some(_), _, _) | (None, QueueSupply::Drained, _) | (None, QueueSupply::MayHoldMore, true) => Refill::Withheld,
            };
            if let Some(round) = plan.next_round(refill) {
                slots.push(self.issue(round, &mut share, &clock, &held).boxed());
            }
            let Some(mut event) = slots.next().await else {
                break;
            };
            loop {
                match event {
                    SlotEvent::Attempted(ended) => match *ended {
                        AttemptEnded {
                            settlement: Ok(settlement),
                            occupant,
                        } => plan.wait(AwaitingAcknowledgement { settlement, occupant }),
                        AttemptEnded {
                            settlement: Err(error),
                            occupant,
                        } => {
                            release_share(&mut share, occupant);
                            plan.free();
                            failure.get_or_insert(error);
                        }
                    },
                    SlotEvent::RoundEnded(ended) => {
                        let RoundEnded { claim, answer } = *ended;
                        let claims = match answer {
                            Ok(SettledRound { answers, claims }) => {
                                lease_lost += answers.iter().filter(|answer| **answer == LeaseAcknowledgement::Lost).count();
                                claims
                            }
                            Err(error) => Err(error),
                        };
                        match claims {
                            Ok(claims) => {
                                if let Some(asked) = claim {
                                    supply = supply.after_claim(asked.width, asked.order, claims.len());
                                }
                                plan.settle(claim.map(|asked| asked.width), claims.len());
                                claimed += claims.len();
                                for claim in &claims {
                                    match claim {
                                        DeliveryClaim::Readable(entry) => share.occupy(&entry.target, &entry.subscription_id),
                                        DeliveryClaim::Unreadable(_) => {}
                                    }
                                }
                                slots.extend(claims.into_iter().map(|claim| self.attempt(claim, &clock, &held).boxed()));
                            }
                            Err(error) => {
                                plan.settle(claim.map(|asked| asked.width), 0);
                                failure.get_or_insert(error);
                            }
                        }
                    }
                }
                // Every event already finished is taken in before the next
                // round goes out, so attempts that end together share one
                // round.
                match slots.next().now_or_never() {
                    Some(Some(ready)) => event = ready,
                    Some(None) | None => break,
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(DeliveryDrainOutcome { claimed, lease_lost }),
        }
    }

    /// Issue one round: the deliveries it settles leave flight now, their
    /// attempts having ended, so the order its claim takes counts them out.
    fn issue<'pass>(
        &'pass self,
        round: Round<AwaitingAcknowledgement>,
        share: &mut SlotShare,
        clock: &'pass LeasePassClock,
        held: &'pass HeldEvents,
    ) -> impl Future<Output = SlotEvent> + Send + 'pass {
        let settlements = round
            .group
            .into_iter()
            .map(|AwaitingAcknowledgement { settlement, occupant }| {
                release_share(share, occupant);
                settlement
            })
            .collect();
        let claim = round.width.map(|width| ClaimAsked {
            width,
            order: share.claim_order(Instant::now()),
        });
        self.settle_and_claim(settlements, claim, clock, held)
    }

    /// Record `settlements` and claim what `claim` asks for under a lease that
    /// starts now, reusing the events the pass's in-flight deliveries hold.
    async fn settle_and_claim(&self, settlements: Vec<DeliverySettlement>, claim: Option<ClaimAsked>, clock: &LeasePassClock, held: &HeldEvents) -> SlotEvent {
        let request = claim.map(|asked| {
            NonZeroU32::try_from(asked.width).map(|width| ClaimRequest {
                lease: self.lease_term,
                width,
                order: asked.order,
                held,
            })
        });
        let answer = match request {
            Some(Ok(request)) => self.acknowledgement.settle_and_claim(&settlements, Some(request)).await,
            Some(Err(error)) => self.acknowledgement.settle_and_claim(&settlements, None).await.map(|round| SettledRound {
                claims: Err(NotificationInvariant::ClaimWidth(error).into()),
                ..round
            }),
            None => self.acknowledgement.settle_and_claim(&settlements, None).await,
        };
        if let (Some(asked), Ok(SettledRound { claims: Ok(claimed), .. })) = (claim, &answer) {
            debug!(
                count = claimed.len(),
                order = ?asked.order,
                worker_id = %self.worker_id,
                lease_expires_at = ?self.lease_term.expires_at(clock.now()),
                "Claimed notification deliveries"
            );
        }
        SlotEvent::RoundEnded(Box::new(RoundEnded { claim, answer }))
    }

    /// Attempt one claimed delivery, settle its outcome, and let go of the
    /// event it carried; its slot stays held until the settlement goes out
    /// in a round.
    async fn attempt(&self, claim: DeliveryClaim, clock: &LeasePassClock, held: &HeldEvents) -> SlotEvent {
        match claim {
            DeliveryClaim::Readable(entry) => {
                let verdict = self.attempts.verdict(&entry, clock).await;
                let DeliveryEntry {
                    target,
                    key,
                    event,
                    subscription_id,
                    ..
                } = *entry;
                held.release(&target, event);
                SlotEvent::Attempted(Box::new(AttemptEnded {
                    settlement: verdict.map(|verdict| DeliverySettlement {
                        target: target.clone(),
                        key,
                        verdict,
                    }),
                    occupant: Some(SlotOccupant::new(target, subscription_id)),
                }))
            }
            DeliveryClaim::Unreadable(unreadable) => SlotEvent::Attempted(Box::new(AttemptEnded {
                settlement: self.attempts.unreadable_verdict(&unreadable, clock).map(|verdict| DeliverySettlement {
                    target: unreadable.target,
                    key: unreadable.key,
                    verdict,
                }),
                occupant: None,
            })),
        }
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
        delivery_acknowledgement::acknowledgement_retry,
        delivery_claim::{DeliveryClaim, UnreadableDelivery},
        delivery_claim_order::{DeliveryClaimOrder, InFlightCeiling},
        delivery_drain::NotificationDeliveryDrain,
        delivery_entry::{DeliveryEntry, DeliveryKey},
        delivery_lease::LeaseTerm,
        delivery_policy::retry_schedule,
        delivery_round::{ClaimRequest, SettledRound},
        delivery_settlement::{DeliverySettlement, DeliveryVerdict},
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
    use andromeda_context_commit::{
        lease_acknowledgement::LeaseAcknowledgement,
        lease_holder::LeaseHolder,
        store_instant::StoreInstant,
        write_id::WriteId,
        write_ordinal::WriteOrdinal,
    };
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
        /// An acknowledgement of this delivery, whatever its outcome, in the
        /// group it went with.
        Acknowledge(DeliveryKey),
    }

    /// Whether the recorded store serves claims or fails them as an
    /// unreachable queue would.
    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    enum ClaimState {
        #[default]
        Served,
        Refused,
        /// The first claim is served and every later one fails, the
        /// settlements beside it still recorded.
        ServedOnce,
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
        /// Claims served so far.
        served_claims: AtomicUsize,
        /// The polls each round stays unanswered for, as its round trip to
        /// the queue would.
        round_polls: u32,
        /// Rounds issued and not yet answered.
        rounds_in_flight: AtomicUsize,
        /// The most rounds in flight at once.
        peak_rounds_in_flight: AtomicUsize,
        /// Each round as the settlements it carried and the width it claimed,
        /// in the order they were made.
        rounds: Mutex<Vec<(usize, Option<u32>)>>,
        /// Deliveries whose leases another worker took, answered lost.
        lost: BTreeSet<DeliveryKey>,
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

        /// A store whose every round stays unanswered for
        /// [`SLOW_ROUND_POLLS`] polls, and which answers the leases of `lost`
        /// lost.
        fn with_slow_rounds(entries: impl IntoIterator<Item = DeliveryEntry>, lost: impl IntoIterator<Item = DeliveryKey>) -> RecordingStore {
            RecordingStore {
                round_polls: SLOW_ROUND_POLLS,
                lost: lost.into_iter().collect(),
                ..RecordingStore::with_entries(entries)
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

        /// Whether the group is refused whole, as an unreachable queue would.
        fn refuses_group(&self) -> bool {
            match self.lease_state {
                LeaseState::Unrecordable => true,
                LeaseState::UnrecordableOnce => !self.refused_once.swap(true, Ordering::SeqCst),
                LeaseState::Held | LeaseState::Expired => false,
            }
        }

        /// Record one settlement under its kind, or answer its lease lost.
        fn settle(&self, settlement: &DeliverySettlement) -> LeaseAcknowledgement {
            if self.lease_state == LeaseState::Expired || self.lost.contains(&settlement.key) {
                return LeaseAcknowledgement::Lost;
            }
            match &settlement.verdict {
                DeliveryVerdict::Delivered => self.delivered.lock().expect("the test lock is not poisoned").push(settlement.key),
                DeliveryVerdict::Failed { detail, next_attempt_at } => {
                    self.failed
                        .lock()
                        .expect("the test lock is not poisoned")
                        .push((settlement.key, detail.clone(), *next_attempt_at));
                }
                DeliveryVerdict::Exhausted { detail, next_probe_at } => {
                    self.terminal
                        .lock()
                        .expect("the test lock is not poisoned")
                        .push((settlement.key, detail.clone()));
                    self.next_probes.lock().expect("the test lock is not poisoned").push(*next_probe_at);
                }
            }
            LeaseAcknowledgement::Applied
        }

        /// The sizes of the rounds that carried settlements.
        fn group_sizes(&self) -> Vec<usize> {
            self.recorded_rounds().into_iter().map(|(group, _)| group).filter(|group| *group > 0).collect()
        }

        fn recorded_rounds(&self) -> Vec<(usize, Option<u32>)> {
            self.rounds.lock().expect("the test lock is not poisoned").clone()
        }

        /// Record one round's settlements, or refuse them whole as an
        /// unreachable queue would.
        fn acknowledge(&self, settlements: &[DeliverySettlement]) -> Result<Vec<LeaseAcknowledgement>, NotificationError> {
            if settlements.is_empty() {
                return Ok(Vec::new());
            }
            self.calls
                .lock()
                .expect("the test lock is not poisoned")
                .extend(settlements.iter().map(|settlement| StoreCall::Acknowledge(settlement.key)));
            if self.refuses_group() {
                return Err(NotificationError::Persistence(PersistenceFailure::Unavailable));
            }
            Ok(settlements.iter().map(|settlement| self.settle(settlement)).collect())
        }

        /// Claim what `request` asks for, or fail as an unreachable queue
        /// would.
        fn claim(&self, request: ClaimRequest<'_>) -> Result<Vec<DeliveryClaim>, NotificationError> {
            let served_before = self.served_claims.fetch_add(1, Ordering::SeqCst);
            match (self.claim_state, served_before) {
                (ClaimState::Refused, _) => return Err(NotificationError::Persistence(PersistenceFailure::Unavailable)),
                (ClaimState::ServedOnce, served) if served > 0 => return Err(NotificationError::Persistence(PersistenceFailure::Unavailable)),
                (ClaimState::Served | ClaimState::ServedOnce, _) => {}
            }
            self.claimed_with.lock().expect("the test lock is not poisoned").push(request.lease);
            self.orders.lock().expect("the test lock is not poisoned").push(request.order);
            let width = usize::try_from(request.width.get()).expect("a claim width within the address width");
            let mut entries = self.entries.lock().expect("the test lock is not poisoned");
            let taken = width.min(entries.len());
            self.calls.lock().expect("the test lock is not poisoned").push(StoreCall::Claim {
                asked: request.width.get(),
                granted: u32::try_from(taken).expect("a granted count within the claim width"),
            });
            let claims = entries.drain(..taken).map(|claim| self.hand_over(claim, request.held)).collect();
            self.peak_held.fetch_max(request.held.len(), Ordering::SeqCst);
            Ok(claims)
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

    /// Wait `polls` polls, as a round trip to the queue would.
    async fn round_trip(polls: u32) {
        let mut remaining = polls;
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

    #[async_trait]
    impl DeliveryStore for RecordingStore {
        async fn settle_and_claim(
            &self,
            _worker_id: &LeaseHolder,
            settlements: &[DeliverySettlement],
            refill: Option<ClaimRequest<'_>>,
        ) -> Result<SettledRound, NotificationError> {
            let in_flight = self.rounds_in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak_rounds_in_flight.fetch_max(in_flight, Ordering::SeqCst);
            round_trip(self.round_polls).await;
            self.rounds_in_flight.fetch_sub(1, Ordering::SeqCst);
            self.rounds
                .lock()
                .expect("the test lock is not poisoned")
                .push((settlements.len(), refill.map(|request| request.width.get())));
            let answers = self.acknowledge(settlements)?;
            let claims = refill.map_or_else(|| Ok(Vec::new()), |request| self.claim(request));
            Ok(SettledRound { answers, claims })
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

    /// The polls a slow round stays unanswered for: far more than the
    /// attempts in flight beside it take to end.
    const SLOW_ROUND_POLLS: u32 = 64;

    /// A pass opens by claiming one delivery per slot and claims again only
    /// for slots that freed, never more than are free, until a claim finds
    /// the queue empty.
    #[test]
    fn a_pass_claims_only_for_free_slots_until_the_queue_runs_out() -> Result<(), NotificationError> {
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
            assert!(widths.iter().all(|width| *width <= 4), "{widths:?}");
            assert!(store.entries.lock().expect("the test lock is not poisoned").is_empty());
            pretty_assertions::assert_eq!(store.delivered.lock().expect("the delivered result lock is not poisoned").len(), 10);
            Ok(())
        })
    }

    /// The opening round claims every slot. Delivery 1 ends first and goes
    /// out alone, claiming its own slot; the five that end while that round is
    /// in flight go out together and claim their five. The queue runs out
    /// with the fourth round, so the last one only acknowledges.
    #[test]
    fn a_round_claims_the_slots_its_own_group_frees() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=12_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_slow_rounds(entries, []));
            let transport = Staggered(FixedTransport(DeliveryDisposition::Delivered));
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(transport), waits(), &sized(6))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 12);
            pretty_assertions::assert_eq!(store.recorded_rounds(), vec![(0, Some(6)), (1, Some(1)), (5, Some(5)), (1, Some(1)), (5, None)]);
            pretty_assertions::assert_eq!(store.delivered.lock().expect("the delivered result lock is not poisoned").len(), 12);
            Ok(())
        })
    }

    /// The first attempt to end is acknowledged at once. The five that end
    /// while that round is in flight wait for it and share one round, issued
    /// when it comes back.
    #[test]
    fn five_settlements_during_a_round_produce_one_further_round_of_five() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=6_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_slow_rounds(entries, []));
            let transport = Staggered(FixedTransport(DeliveryDisposition::Delivered));
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(transport), waits(), &sized(6))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 6);
            pretty_assertions::assert_eq!(store.group_sizes(), vec![1, 5]);
            pretty_assertions::assert_eq!(store.delivered.lock().expect("the delivered result lock is not poisoned").len(), 6);
            Ok(())
        })
    }

    #[test]
    fn attempts_that_end_together_share_the_first_acknowledgement() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=6_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_slow_rounds(entries, []));
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &sized(6))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 6);
            pretty_assertions::assert_eq!(store.group_sizes(), vec![6]);
            Ok(())
        })
    }

    /// The polls every delivery but the first waits before its transport
    /// answers: fewer than a round stays in flight, so the first attempt ends
    /// alone and the rest end while its round is in flight.
    const STAGGER_POLLS: u32 = 8;

    /// A transport that answers delivery 1 at once and every other one
    /// [`STAGGER_POLLS`] polls later, as `.0` would.
    struct Staggered<T>(T);

    impl<T: NotificationDeliveryTransport> NotificationDeliveryTransport for Staggered<T> {
        fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            Box::pin(async move {
                if entry.key != DeliveryKey::from(1_i64) {
                    round_trip(STAGGER_POLLS).await;
                }
                self.0.deliver(entry).await
            })
        }
    }

    #[test]
    fn a_pass_never_has_two_rounds_in_flight() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=200_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_slow_rounds(entries, []));
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(InFlightTransport::default()), waits(), &sized(20))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!(outcome.claimed, 200);
            pretty_assertions::assert_eq!(store.peak_rounds_in_flight.load(Ordering::SeqCst), 1);
            pretty_assertions::assert_eq!(store.group_sizes().iter().sum::<usize>(), 200);
            let rounds = store.recorded_rounds().len();
            assert!(rounds * 4 < 200, "{rounds} rounds for 200 deliveries");
            Ok(())
        })
    }

    /// Each answers one delivery of a group of four the transport settles
    /// differently: delivered, failed for a retry, failed on the final
    /// attempt, and delivered under a lease another worker took.
    struct MixedTransport;

    impl NotificationDeliveryTransport for MixedTransport {
        fn deliver<'a>(&'a self, entry: &'a DeliveryEntry) -> BoxFuture<'a, DeliveryDisposition> {
            Box::pin(async move {
                match entry.key.into_inner() {
                    3 => DeliveryDisposition::RetryableFailure {
                        detail: "transient".to_owned(),
                    },
                    4 => DeliveryDisposition::TerminalFailure {
                        detail: "permanent".to_owned(),
                    },
                    _ => DeliveryDisposition::Delivered,
                }
            })
        }
    }

    /// Delivery 1 is acknowledged alone; deliveries 2 to 5 end while it is in
    /// flight and go together, and each is recorded as it would be alone.
    #[test]
    fn a_group_mixing_every_outcome_records_each_as_it_would_alone() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=5_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore::with_slow_rounds(entries, [DeliveryKey::from(5_i64)]));
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(Staggered(MixedTransport)), waits(), &sized(5))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!((outcome.claimed, outcome.lease_lost), (5, 1));
            pretty_assertions::assert_eq!(store.group_sizes(), vec![1, 4]);
            pretty_assertions::assert_eq!(
                store.delivered.lock().expect("the delivered result lock is not poisoned").as_slice(),
                &[DeliveryKey::from(1_i64), DeliveryKey::from(2_i64)]
            );
            let failed = store.failed.lock().expect("the retry result lock is not poisoned");
            pretty_assertions::assert_eq!(
                failed.iter().map(|(key, detail, _)| (*key, detail.as_str())).collect::<Vec<_>>(),
                vec![(DeliveryKey::from(3_i64), "transien")]
            );
            let terminal = store.terminal.lock().expect("the terminal result lock is not poisoned");
            pretty_assertions::assert_eq!(terminal.as_slice(), &[(DeliveryKey::from(4_i64), "permanen".to_owned())]);
            pretty_assertions::assert_eq!(store.next_probes.lock().expect("the probe record lock is not poisoned").len(), 1);
            Ok(())
        })
    }

    /// A round the queue failed to record claimed nothing either, so it is
    /// asked again whole with its claim: none of its deliveries is recorded
    /// twice and every one is recorded once.
    #[test]
    fn a_round_the_queue_failed_to_record_is_asked_again_whole_with_its_claim() -> Result<(), NotificationError> {
        block_on(async {
            let entries = (1..=5_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?;
            let store = Arc::new(RecordingStore {
                lease_state: LeaseState::UnrecordableOnce,
                ..RecordingStore::with_slow_rounds(entries, [])
            });
            let transport = Staggered(FixedTransport(DeliveryDisposition::Delivered));
            let outcome = NotificationDeliveryDrain::new(store.clone(), Arc::new(transport), waits(), &sized(5))?
                .execute(Utc::now(), &CancellationToken::new())
                .await?;
            pretty_assertions::assert_eq!((outcome.claimed, outcome.lease_lost), (5, 0));
            pretty_assertions::assert_eq!(store.recorded_rounds(), vec![(0, Some(5)), (1, Some(1)), (1, Some(1)), (4, None)]);
            pretty_assertions::assert_eq!(store.claim_widths(), vec![5, 1]);
            let mut delivered = store.delivered.lock().expect("the delivered result lock is not poisoned").clone();
            delivered.sort();
            pretty_assertions::assert_eq!(delivered, (1..=5_i64).map(DeliveryKey::from).collect::<Vec<_>>());
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
    /// flight are acknowledged. It opens with a round that only claims and
    /// ends with one that only acknowledges.
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
            // Both deliveries end together, so the second round claims both their slots and comes back with the last delivery.
            pretty_assertions::assert_eq!(store.recorded_rounds(), vec![(0, Some(2)), (2, Some(2)), (1, None)]);
            Ok(())
        })
    }

    /// A claim the queue cannot serve once the settlements beside it are
    /// recorded ends the pass with them recorded, and no later round claims.
    #[test]
    fn a_claim_that_fails_after_its_group_is_recorded_ends_the_pass_with_the_group_recorded() -> Result<(), NotificationError> {
        let store = Arc::new(RecordingStore {
            claim_state: ClaimState::ServedOnce,
            ..RecordingStore::with_entries((1..=4_i64).map(entry_keyed).collect::<Result<Vec<_>, _>>()?)
        });
        let outcome = block_on(async {
            NotificationDeliveryDrain::new(store.clone(), Arc::new(FixedTransport(DeliveryDisposition::Delivered)), waits(), &sized(2))?
                .execute(Utc::now(), &CancellationToken::new())
                .await
        });
        assert!(matches!(outcome, Err(NotificationError::Persistence { .. })), "{outcome:?}");
        pretty_assertions::assert_eq!(store.recorded_rounds(), vec![(0, Some(2)), (2, Some(2))]);
        let mut delivered = store.delivered.lock().expect("the delivered result lock is not poisoned").clone();
        delivered.sort();
        pretty_assertions::assert_eq!(delivered, vec![DeliveryKey::from(1_i64), DeliveryKey::from(2_i64)]);
        Ok(())
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
            NotificationDeliveryDrain::new(store.clone(), Arc::new(Staggered(InFlightTransport::default())), waits(), &sized(4))?
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
