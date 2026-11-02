//! How the slots of one delivery pass are shared between Subscriptions.
//!
//! A pass claims in queue order until a Subscription holds half its slots,
//! because that claim is one range scan as deep as the claim. Once one does,
//! an endpoint that answers slowly is keeping its slots busy while the queue
//! keeps handing it more, so the pass claims the Subscriptions with the fewest
//! deliveries in flight first, and keeps doing so until that Subscription has
//! nothing left in flight: the front of the queue is its backlog, so a single
//! claim in queue order would hand it the next slot again. The fair claim
//! visits each Subscription with queued deliveries, which is why it is kept
//! for when a Subscription crowds the others.
//!
//! While more than one Subscription is active, none gets more than half the
//! slots: a slot a slow one takes while the others have nothing due at that
//! instant would stay taken for as long as its endpoint takes to answer, so a
//! Subscription written in bursts would find most slots held at its next
//! burst. A Subscription counts as active while it has a delivery in flight
//! and for one `request_timeout` after its last one was answered, the longest
//! a slot taken in the meantime can stay taken. A Subscription alone, or
//! alone that long, uses every slot.

use crate::{
    delivery_claim_order::{DeliveryClaimOrder, InFlightCeiling},
    delivery_concurrency::DeliveryConcurrency,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use std::{
    collections::{HashMap, HashSet},
    num::{NonZeroU32, NonZeroUsize},
    time::{Duration, Instant},
};

/// The Subscription a delivery in flight occupies a slot for.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SlotOccupant {
    /// The branch the Subscription lives in: a Snapshot branch holds its own
    /// copy under the same identifier.
    target: ContextTarget,
    /// The Subscription.
    subscription_id: Identifier,
}

impl SlotOccupant {
    /// The occupant of a slot holding a delivery to `subscription_id` in
    /// `target`.
    pub(crate) const fn new(target: ContextTarget, subscription_id: Identifier) -> SlotOccupant {
        SlotOccupant { target, subscription_id }
    }
}

/// The deliveries one pass has in flight per Subscription, and which
/// Subscriptions were active lately.
#[derive(Debug)]
pub(crate) struct SlotShare {
    /// The in-flight count at which a Subscription crowds the others: half
    /// the pass's slots, rounded up.
    crowding: NonZeroU32,
    /// How long a Subscription stays active after its last delivery in
    /// flight was answered.
    active_for: Duration,
    /// Deliveries in flight per Subscription; a Subscription with none has no
    /// entry.
    in_flight: HashMap<SlotOccupant, NonZeroUsize>,
    /// When each Subscription's last delivery left flight, kept for
    /// `active_for`.
    released_at: HashMap<SlotOccupant, Instant>,
    /// The Subscriptions that reached half the slots and still have a
    /// delivery in flight.
    crowding_ones: HashSet<SlotOccupant>,
}

impl SlotShare {
    /// The share of a pass with `concurrency` slots, none of them occupied,
    /// where a Subscription stays active for `active_for` after its last
    /// delivery was answered.
    pub(crate) fn new(concurrency: DeliveryConcurrency, active_for: Duration) -> SlotShare {
        let slots = u32::try_from(concurrency.get().get()).unwrap_or(DeliveryConcurrency::MAXIMUM);
        SlotShare {
            crowding: NonZeroU32::new(slots.div_ceil(2)).unwrap_or(NonZeroU32::MIN),
            active_for,
            in_flight: HashMap::new(),
            released_at: HashMap::new(),
            crowding_ones: HashSet::new(),
        }
    }

    /// Count a delivery to `subscription_id` in `target` in flight until
    /// [`SlotShare::release`].
    pub(crate) fn occupy(&mut self, target: &ContextTarget, subscription_id: &Identifier) {
        // clone: the count outlives the claimed entry, which its attempt consumes.
        let occupant = SlotOccupant::new(target.clone(), subscription_id.clone());
        let count = self
            .in_flight
            .entry(occupant)
            .and_modify(|count| *count = count.saturating_add(1))
            .or_insert(NonZeroUsize::MIN);
        if u32::try_from(count.get()).is_ok_and(|count| count >= self.crowding.get()) {
            // clone: the set outlives this borrow of the in-flight entry.
            self.crowding_ones.insert(SlotOccupant::new(target.clone(), subscription_id.clone()));
        }
    }

    /// Take one delivery of `occupant` out of flight at `now`.
    pub(crate) fn release(&mut self, occupant: SlotOccupant, now: Instant) {
        if let Some(count) = self.in_flight.get_mut(&occupant) {
            if let Some(remaining) = NonZeroUsize::new(count.get().saturating_sub(1)) {
                *count = remaining;
            } else {
                self.in_flight.remove(&occupant);
                self.crowding_ones.remove(&occupant);
            }
        }
        self.released_at.insert(occupant, now);
    }

    /// The order the pass's next claim, made at `now`, takes due deliveries in.
    pub(crate) fn claim_order(&mut self, now: Instant) -> DeliveryClaimOrder {
        let active_for = self.active_for;
        self.released_at.retain(|_, released| now.saturating_duration_since(*released) < active_for);
        if self.crowding_ones.is_empty() {
            return DeliveryClaimOrder::Queue;
        }
        let mut active = self.in_flight.keys().chain(self.released_at.keys());
        let first = active.next();
        let another = active.any(|occupant| Some(occupant) != first);
        DeliveryClaimOrder::FewestInFlight(if another {
            InFlightCeiling::Below(self.crowding)
        } else {
            InFlightCeiling::Lifted
        })
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        delivery_claim_order::{DeliveryClaimOrder, InFlightCeiling},
        delivery_concurrency::DeliveryConcurrency,
        delivery_slot_share::{SlotOccupant, SlotShare},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;
    use std::{
        num::NonZeroU32,
        time::{Duration, Instant},
    };

    /// How long a Subscription stays active after its last answer, as the default `request_timeout`.
    const ACTIVE_FOR: Duration = Duration::from_secs(10);

    fn share(concurrency: u32) -> SlotShare {
        SlotShare::new(DeliveryConcurrency::from_count(concurrency).expect("a valid concurrency"), ACTIVE_FOR)
    }

    fn subscription(name: &str) -> Identifier {
        format!("urn:ngsi-ld:Subscription:{name}").parse().expect("a valid identifier")
    }

    fn occupy(share: &mut SlotShare, name: &str, deliveries: usize) {
        for _ in 0..deliveries {
            share.occupy(&ContextTarget::Live, &subscription(name));
        }
    }

    fn occupant(name: &str) -> SlotOccupant {
        SlotOccupant::new(ContextTarget::Live, subscription(name))
    }

    fn below(ceiling: u32) -> DeliveryClaimOrder {
        DeliveryClaimOrder::FewestInFlight(InFlightCeiling::Below(NonZeroU32::new(ceiling).expect("a positive ceiling")))
    }

    #[test]
    fn a_pass_claims_in_queue_order_while_no_subscription_holds_half_its_slots() {
        let mut share = share(20);
        occupy(&mut share, "slow", 9);
        occupy(&mut share, "fast", 1);
        assert_eq!(share.claim_order(Instant::now()), DeliveryClaimOrder::Queue);
    }

    #[test]
    fn a_subscription_alone_holding_half_the_slots_is_claimed_for_without_a_ceiling() {
        let mut share = share(20);
        occupy(&mut share, "slow", 10);
        assert_eq!(share.claim_order(Instant::now()), DeliveryClaimOrder::FewestInFlight(InFlightCeiling::Lifted));
    }

    #[test]
    fn a_crowding_subscription_beside_one_in_flight_is_held_below_half_the_slots() {
        let mut share = share(20);
        occupy(&mut share, "slow", 10);
        occupy(&mut share, "fast", 1);
        assert_eq!(share.claim_order(Instant::now()), below(10));
    }

    #[test]
    fn a_subscription_answered_within_the_activity_window_still_holds_the_ceiling() {
        let mut share = share(20);
        let start = Instant::now();
        occupy(&mut share, "slow", 10);
        occupy(&mut share, "fast", 1);
        share.release(occupant("fast"), start);
        assert_eq!(share.claim_order(start + Duration::from_millis(9_999)), below(10));
    }

    #[test]
    fn the_ceiling_lifts_once_the_other_subscription_is_quiet_for_the_activity_window() {
        let mut share = share(20);
        let start = Instant::now();
        occupy(&mut share, "slow", 10);
        occupy(&mut share, "fast", 1);
        share.release(occupant("fast"), start);
        assert_eq!(
            share.claim_order(start + ACTIVE_FOR),
            DeliveryClaimOrder::FewestInFlight(InFlightCeiling::Lifted)
        );
        assert!(share.released_at.is_empty());
    }

    #[test]
    fn two_crowding_subscriptions_hold_each_other_below_half_the_slots() {
        let mut share = share(20);
        occupy(&mut share, "first", 10);
        occupy(&mut share, "second", 10);
        let now = Instant::now();
        share.release(occupant("second"), now);
        assert_eq!(share.claim_order(now), below(10));
    }

    #[test]
    fn half_of_an_odd_slot_count_is_rounded_up() {
        let mut share = share(5);
        occupy(&mut share, "slow", 2);
        assert_eq!(share.claim_order(Instant::now()), DeliveryClaimOrder::Queue);
        occupy(&mut share, "slow", 1);
        assert_eq!(share.claim_order(Instant::now()), DeliveryClaimOrder::FewestInFlight(InFlightCeiling::Lifted));
    }

    #[test]
    fn a_crowding_subscription_keeps_the_fair_order_until_it_has_nothing_in_flight() {
        let mut share = share(4);
        occupy(&mut share, "slow", 2);
        let now = Instant::now();
        share.release(occupant("slow"), now);
        assert_eq!(share.claim_order(now), DeliveryClaimOrder::FewestInFlight(InFlightCeiling::Lifted));
        share.release(occupant("slow"), now);
        assert!(share.in_flight.is_empty());
        assert_eq!(share.claim_order(now), DeliveryClaimOrder::Queue);
    }

    #[test]
    fn a_crowding_subscription_below_half_beside_another_is_still_held_below_half() {
        let mut share = share(20);
        occupy(&mut share, "slow", 10);
        occupy(&mut share, "fast", 1);
        let now = Instant::now();
        share.release(occupant("slow"), now);
        assert_eq!(share.claim_order(now), below(10));
    }

    #[test]
    fn releasing_a_subscription_with_nothing_in_flight_counts_nothing_out() {
        let mut share = share(4);
        share.release(occupant("absent"), Instant::now());
        assert!(share.in_flight.is_empty());
    }

    #[test]
    fn a_single_slot_is_crowded_by_one_delivery() {
        let mut share = share(1);
        occupy(&mut share, "only", 1);
        assert_eq!(share.claim_order(Instant::now()), DeliveryClaimOrder::FewestInFlight(InFlightCeiling::Lifted));
    }
}
