//! When a delivery pass goes to the queue, with which settlements, and for how
//! many slots.
//!
//! A pass has at most one round in flight, and a round both records
//! settlements and claims. A delivery whose attempt ends while a round is in
//! flight waits for it, and the next round carries every settlement that
//! waited and claims every slot free when it is issued, the slots of its own
//! settlements included: the deliveries it claims start once the round has
//! recorded those settlements, so the pass never has more deliveries in flight
//! than slots. With endpoints that answer at once, the attempts that end
//! during one round trip share the next one instead of each paying a
//! transaction and a claim of its own, and a lone settlement with no round in
//! flight is still recorded at once, with no timer.
//!
//! A slot a claim left empty, because the queue had nothing more to give it at
//! that instant, is claimed again only with the next slot that frees: claiming
//! it at once would ask the queue the same question again straight away.

use crate::delivery_round_trip::RoundTrip;
use std::{mem::take, num::NonZeroUsize};

/// Whether the next round may claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Refill {
    /// The pass still claims: the queue may hold more, and nothing failed or
    /// cancelled it.
    Allowed,
    /// The pass claims no further and only acknowledges what is in flight.
    Withheld,
}

/// One round to issue: the settlements it records and the width it claims.
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct Round<T> {
    /// Settlements waiting, in the order their attempts ended; empty for a
    /// round that only claims.
    pub(crate) group: Vec<T>,
    /// Slots to claim, `None` for a round that only acknowledges.
    pub(crate) width: Option<NonZeroUsize>,
}

/// The settlements a pass has not recorded yet, its free slots and its round
/// in flight.
#[derive(Debug)]
pub(crate) struct RoundPlan<T> {
    /// Settlements waiting for the next round, in the order their attempts
    /// ended.
    waiting: Vec<T>,
    /// Slots freed since the last claim was issued: every slot of a pass that
    /// has not claimed yet, then one per delivery settled or given up.
    freed: usize,
    /// Slots earlier claims left empty because they came back short.
    left_empty: usize,
    /// Whether a round is in flight.
    round: RoundTrip,
}

impl<T> RoundPlan<T> {
    /// The plan of a pass with `slots` slots, all of them free and none
    /// claimed yet.
    pub(crate) const fn new(slots: NonZeroUsize) -> RoundPlan<T> {
        RoundPlan {
            waiting: Vec::new(),
            freed: slots.get(),
            left_empty: 0,
            round: RoundTrip::Settled,
        }
    }

    /// Record a delivery whose attempt ended, to be settled by the next
    /// round.
    pub(crate) fn wait(&mut self, settlement: T) {
        self.waiting.push(settlement);
    }

    /// Free the slot of a delivery whose attempt ended with nothing to
    /// settle.
    pub(crate) const fn free(&mut self) {
        self.freed = self.freed.saturating_add(1);
    }

    /// Record the round in flight coming back with `got` deliveries of the
    /// `asked` it claimed, `got` being 0 for a claim or a round that failed.
    pub(crate) const fn settle(&mut self, asked: Option<NonZeroUsize>, got: usize) {
        if let Some(asked) = asked {
            self.left_empty = self.left_empty.saturating_add(asked.get().saturating_sub(got));
        }
        self.round = RoundTrip::Settled;
    }

    /// The round to issue now, or `None` while a round is in flight or when
    /// there is nothing to settle and nothing to claim. A round returned
    /// counts as in flight until [`RoundPlan::settle`].
    ///
    /// Its width, under [`Refill::Allowed`], is every free slot: those its
    /// own settlements free, those freed since the last claim, and those
    /// earlier claims left empty, once at least one slot freed.
    pub(crate) fn next_round(&mut self, refill: Refill) -> Option<Round<T>> {
        match self.round {
            RoundTrip::InFlight => None,
            RoundTrip::Settled => {
                let group = take(&mut self.waiting);
                self.freed = self.freed.saturating_add(group.len());
                let width = match (refill, NonZeroUsize::new(self.freed)) {
                    (Refill::Withheld, _) | (Refill::Allowed, None) => None,
                    (Refill::Allowed, Some(freed)) => {
                        let width = freed.saturating_add(self.left_empty);
                        self.freed = 0;
                        self.left_empty = 0;
                        Some(width)
                    }
                };
                let has_work = !group.is_empty() || width.is_some();
                has_work.then(|| {
                    self.round = RoundTrip::InFlight;
                    Round { group, width }
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_round_plan::{Refill, Round, RoundPlan};
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;

    /// A width of `count` slots, `count` being positive.
    const fn width(count: usize) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add(count.saturating_sub(1))
    }

    /// A plan of `slots` slots whose opening claim came back with `got`.
    fn opened(slots: usize, got: usize) -> RoundPlan<i32> {
        let mut plan = RoundPlan::new(width(slots));
        assert_eq!(plan.next_round(Refill::Allowed).and_then(|round| round.width), Some(width(slots)));
        plan.settle(Some(width(slots)), got);
        plan
    }

    #[test]
    fn a_pass_opens_with_a_round_that_claims_every_slot_and_settles_nothing() {
        let mut plan = RoundPlan::<i32>::new(width(4));
        assert_eq!(
            plan.next_round(Refill::Allowed),
            Some(Round {
                group: Vec::new(),
                width: Some(width(4)),
            })
        );
    }

    #[test]
    fn a_filled_pass_with_nothing_to_settle_issues_no_round() {
        let mut plan = opened(4, 4);
        assert_eq!(plan.next_round(Refill::Allowed), None);
    }

    #[test]
    fn a_pass_that_may_not_claim_and_has_nothing_to_settle_issues_no_round() {
        let mut plan = RoundPlan::<i32>::new(width(4));
        assert_eq!(plan.next_round(Refill::Withheld), None);
    }

    #[test]
    fn a_pass_that_may_not_claim_still_settles_what_waits() {
        let mut plan = opened(4, 4);
        plan.wait(1);
        assert_eq!(plan.next_round(Refill::Withheld), Some(Round { group: vec![1], width: None }));
    }

    #[test]
    fn a_round_claims_the_slots_its_own_settlements_free() {
        let mut plan = opened(4, 4);
        plan.wait(1);
        plan.wait(2);
        assert_eq!(
            plan.next_round(Refill::Allowed),
            Some(Round {
                group: vec![1, 2],
                width: Some(width(2)),
            })
        );
    }

    /// Two slots the opening claim left empty, one slot freed with nothing
    /// to settle and two settlements make a claim of five.
    #[test]
    fn a_round_claims_the_freed_slots_and_those_a_short_claim_left_empty_beside_its_own() {
        let mut plan = opened(6, 4);
        plan.free();
        plan.wait(1);
        plan.wait(2);
        assert_eq!(
            plan.next_round(Refill::Allowed),
            Some(Round {
                group: vec![1, 2],
                width: Some(width(5)),
            })
        );
    }

    #[test]
    fn a_slot_a_short_claim_left_empty_is_claimed_again_only_with_the_next_slot_that_frees() {
        let mut plan = opened(4, 1);
        assert_eq!(plan.next_round(Refill::Allowed), None);
        plan.wait(1);
        assert_eq!(
            plan.next_round(Refill::Allowed),
            Some(Round {
                group: vec![1],
                width: Some(width(4)),
            })
        );
    }

    #[test]
    fn five_settlements_during_a_round_produce_one_further_round_of_five() {
        let mut plan = opened(6, 6);
        plan.wait(1);
        assert_eq!(plan.next_round(Refill::Allowed).map(|round| round.group), Some(vec![1]));
        for settlement in 2..=6 {
            plan.wait(settlement);
            assert_eq!(plan.next_round(Refill::Allowed), None, "a second round was issued while one was in flight");
        }
        plan.settle(Some(width(1)), 1);
        assert_eq!(
            plan.next_round(Refill::Allowed),
            Some(Round {
                group: vec![2, 3, 4, 5, 6],
                width: Some(width(5)),
            })
        );
        assert_eq!(plan.next_round(Refill::Allowed), None);
    }

    /// A round that failed claimed nothing, so its whole width comes back as
    /// slots left empty for the next claim.
    #[test]
    fn a_failed_round_leaves_its_whole_width_for_the_next_claim() {
        let mut plan = opened(4, 4);
        plan.wait(1);
        plan.wait(2);
        assert_eq!(plan.next_round(Refill::Allowed).and_then(|round| round.width), Some(width(2)));
        plan.settle(Some(width(2)), 0);
        plan.wait(3);
        assert_eq!(plan.next_round(Refill::Allowed).and_then(|round| round.width), Some(width(3)));
    }

    #[test]
    fn a_round_that_comes_back_with_nothing_waiting_and_no_slot_freed_issues_nothing_further() {
        let mut plan = opened(4, 4);
        plan.wait(1);
        assert_eq!(plan.next_round(Refill::Withheld).map(|round| round.group), Some(vec![1]));
        plan.settle(None, 0);
        assert_eq!(plan.next_round(Refill::Withheld), None);
    }
}
