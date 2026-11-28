//! What one delivery pass knows between the events it takes in: how its slots
//! are shared, whether the queue may supply more, the rounds it plans, what it
//! counted, and the first failure.
//!
//! A pass claims whenever a slot frees while the queue may still supply due
//! deliveries. A claim that comes back short leaves the pass drained, and the
//! slots that free afterwards are not claimed again until a commit's wake
//! says the queue may have gained deliveries; the wake then has the next
//! round claim every slot the pass has free, while the attempts that hold the
//! others are still in flight. A slow attempt therefore holds its own slot
//! and not the claiming of every other delivery. The pass takes a wake in
//! only while no round is in flight, so the claim the wake leads to is issued
//! after the commit, and no answer to a claim issued before it can drain the
//! pass again.

use crate::{
    delivery_claim::DeliveryClaim,
    delivery_concurrency::DeliveryConcurrency,
    delivery_drain::DeliveryDrainOutcome,
    delivery_queue_supply::QueueSupply,
    delivery_round::SettledRound,
    delivery_round_plan::{Refill, RoundPlan},
    delivery_round_trip::RoundTrip,
    delivery_settlement::DeliverySettlement,
    delivery_slot_event::{AttemptEnded, AwaitingAcknowledgement, ClaimAsked, RoundEnded, SlotEvent},
    delivery_slot_share::{SlotOccupant, SlotShare},
    error::NotificationError,
};
use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// A round to send to the queue: the settlements it records and the claim it
/// asks for.
pub(crate) struct IssuedRound {
    /// Settlements of the attempts that ended, in the order they ended.
    pub(crate) settlements: Vec<DeliverySettlement>,
    /// The claim, `None` for a round that only acknowledges.
    pub(crate) claim: Option<ClaimAsked>,
}

/// Whether a pass waits for a commit's wake beside its slots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CommitWatch {
    /// A wake would have the next round claim at once: nothing failed or
    /// cancelled the pass, and no round is in flight.
    Watching,
    /// The pass waits on its slots alone.
    SlotsOnly,
}

/// The bookkeeping of one delivery pass.
pub(crate) struct DeliveryPassState {
    /// How the pass's slots are shared between Subscriptions.
    share: SlotShare,
    /// Whether the queue may still supply due deliveries.
    supply: QueueSupply,
    /// The settlements waiting, the slots free and the round in flight.
    plan: RoundPlan<AwaitingAcknowledgement>,
    /// Deliveries claimed so far.
    claimed: usize,
    /// Claimed deliveries whose lease another worker took before their
    /// outcome was recorded.
    lease_lost: usize,
    /// The first failure of the queue, which stops the pass claiming.
    failure: Option<NotificationError>,
}

/// Release the slot of a delivery that left flight from its Subscription's
/// share, when it was read back.
fn release_share(share: &mut SlotShare, occupant: Option<SlotOccupant>) {
    if let Some(occupant) = occupant {
        share.release(occupant, Instant::now());
    }
}

impl DeliveryPassState {
    /// The state of a pass with `concurrency` slots, none claimed yet, whose
    /// Subscriptions stay active for `slot_activity` after their last answer.
    pub(crate) fn new(concurrency: DeliveryConcurrency, slot_activity: Duration) -> DeliveryPassState {
        DeliveryPassState {
            share: SlotShare::new(concurrency, slot_activity),
            supply: QueueSupply::MayHoldMore,
            plan: RoundPlan::new(concurrency.get()),
            claimed: 0,
            lease_lost: 0,
            failure: None,
        }
    }

    /// Whether the next round may claim.
    fn refill(&self, cancellation: &CancellationToken) -> Refill {
        match (&self.failure, self.supply, cancellation.is_cancelled()) {
            (None, QueueSupply::MayHoldMore, false) => Refill::Allowed,
            (Some(_), _, _) | (None, QueueSupply::Drained, _) | (None, QueueSupply::MayHoldMore, true) => Refill::Withheld,
        }
    }

    /// The round to issue now, if any. The deliveries it settles leave flight
    /// as it is issued, so the order its claim takes counts them out.
    pub(crate) fn next_round(&mut self, cancellation: &CancellationToken) -> Option<IssuedRound> {
        let round = self.plan.next_round(self.refill(cancellation))?;
        let share = &mut self.share;
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
        Some(IssuedRound { settlements, claim })
    }

    /// Whether the pass waits for a commit's wake beside its slots.
    pub(crate) fn commit_watch(&self, cancellation: &CancellationToken) -> CommitWatch {
        match (&self.failure, cancellation.is_cancelled(), self.plan.round_trip()) {
            (None, false, RoundTrip::Settled) => CommitWatch::Watching,
            (Some(_), _, _) | (None, true, _) | (None, false, RoundTrip::InFlight) => CommitWatch::SlotsOnly,
        }
    }

    /// Take in a commit's wake: the queue may hold new due deliveries, so the
    /// next round claims every free slot.
    pub(crate) const fn commit_seen(&mut self) {
        self.supply = QueueSupply::MayHoldMore;
        self.plan.reopen();
    }

    /// Take in one event of the in-flight set, returning the deliveries a
    /// round that came back claimed, to be attempted.
    pub(crate) fn take_in(&mut self, event: SlotEvent) -> Vec<DeliveryClaim> {
        match event {
            SlotEvent::Attempted(ended) => {
                match *ended {
                    AttemptEnded {
                        settlement: Ok(settlement),
                        occupant,
                    } => self.plan.wait(AwaitingAcknowledgement { settlement, occupant }),
                    AttemptEnded {
                        settlement: Err(error),
                        occupant,
                    } => {
                        release_share(&mut self.share, occupant);
                        self.plan.free();
                        self.failure.get_or_insert(error);
                    }
                }
                Vec::new()
            }
            SlotEvent::RoundEnded(ended) => self.round_ended(*ended),
        }
    }

    /// Take in a round that came back, returning what it claimed.
    fn round_ended(&mut self, RoundEnded { claim, answer }: RoundEnded) -> Vec<DeliveryClaim> {
        let claims = match answer {
            Ok(SettledRound { answers, claims }) => {
                self.lease_lost += answers.iter().filter(|answer| **answer == LeaseAcknowledgement::Lost).count();
                claims
            }
            Err(error) => Err(error),
        };
        match claims {
            Ok(claims) => {
                if let Some(asked) = claim {
                    self.supply = self.supply.after_claim(asked.width, asked.order, claims.len());
                }
                self.plan.settle(claim.map(|asked| asked.width), claims.len());
                self.claimed += claims.len();
                for claim in &claims {
                    match claim {
                        DeliveryClaim::Readable(entry) => self.share.occupy(&entry.target, &entry.subscription_id),
                        DeliveryClaim::Unreadable(_) => {}
                    }
                }
                claims
            }
            Err(error) => {
                self.plan.settle(claim.map(|asked| asked.width), 0);
                self.failure.get_or_insert(error);
                Vec::new()
            }
        }
    }

    /// What the pass did, or the first failure of the queue.
    pub(crate) fn outcome(self) -> Result<DeliveryDrainOutcome, NotificationError> {
        match self.failure {
            Some(error) => Err(error),
            None => Ok(DeliveryDrainOutcome {
                claimed: self.claimed,
                lease_lost: self.lease_lost,
            }),
        }
    }
}
