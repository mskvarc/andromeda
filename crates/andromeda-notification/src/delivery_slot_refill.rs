//! When a delivery pass claims again, and for how many of its slots.
//!
//! A pass has at most one claim in flight. A slot that frees while a claim is
//! in flight waits for it to come back, and the next claim asks for every slot
//! free when it is issued. With endpoints that answer at once, the
//! acknowledgements that land during one claim round trip share the next
//! claim instead of each paying a claim of their own, and a lone
//! acknowledgement with no claim in flight still refills its slot at once,
//! with no timer.
//!
//! A slot a claim left empty, because the queue had nothing more to give it at
//! that instant, is claimed again only with the next slot that frees: claiming
//! it at once would ask the queue the same question again straight away.

use std::num::NonZeroUsize;

/// Whether a claim of the pass is in flight.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ClaimFlight {
    /// No claim is in flight, so the next free slot is claimed at once.
    #[default]
    Settled,
    /// A claim was issued and has not come back yet.
    InFlight,
}

/// The free slots of one delivery pass and its claim in flight.
#[derive(Debug)]
pub(crate) struct SlotRefill {
    /// Slots freed since the last claim was issued: every slot of a pass that
    /// has not claimed yet, then one per acknowledged delivery.
    freed: usize,
    /// Slots earlier claims left empty because they came back short.
    left_empty: usize,
    /// Whether a claim is in flight.
    claim: ClaimFlight,
}

impl SlotRefill {
    /// The refill of a pass with `slots` slots, all of them free and none
    /// claimed yet.
    pub(crate) const fn new(slots: NonZeroUsize) -> SlotRefill {
        SlotRefill {
            freed: slots.get(),
            left_empty: 0,
            claim: ClaimFlight::Settled,
        }
    }

    /// Record a delivery acknowledged, freeing its slot.
    pub(crate) const fn free(&mut self) {
        self.freed = self.freed.saturating_add(1);
    }

    /// Record the claim in flight coming back with `got` of the `asked`
    /// deliveries it asked for, `got` being 0 for a claim that failed.
    pub(crate) const fn settle(&mut self, asked: NonZeroUsize, got: usize) {
        self.left_empty = self.left_empty.saturating_add(asked.get().saturating_sub(got));
        self.claim = ClaimFlight::Settled;
    }

    /// The width of the claim to issue now, every free slot, or `None` while
    /// a claim is in flight or no slot freed since the last one. A width
    /// returned counts as a claim in flight until [`SlotRefill::settle`].
    pub(crate) const fn next_claim(&mut self) -> Option<NonZeroUsize> {
        match (self.claim, NonZeroUsize::new(self.freed)) {
            (ClaimFlight::InFlight, _) | (ClaimFlight::Settled, None) => None,
            (ClaimFlight::Settled, Some(freed)) => {
                let width = freed.saturating_add(self.left_empty);
                self.freed = 0;
                self.left_empty = 0;
                self.claim = ClaimFlight::InFlight;
                Some(width)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_slot_refill::SlotRefill;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;

    /// A width of `count` slots, `count` being positive.
    const fn width(count: usize) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add(count.saturating_sub(1))
    }

    #[test]
    fn a_pass_opens_with_one_claim_for_every_slot() {
        let mut refill = SlotRefill::new(width(20));
        assert_eq!(refill.next_claim(), Some(width(20)));
        assert_eq!(refill.next_claim(), None);
    }

    #[test]
    fn five_acknowledgements_during_a_claim_produce_one_further_claim_of_width_five() {
        let mut refill = SlotRefill::new(width(20));
        assert_eq!(refill.next_claim(), Some(width(20)));
        for _ in 0..5 {
            refill.free();
            assert_eq!(refill.next_claim(), None, "a second claim was issued while one was in flight");
        }
        refill.settle(width(20), 20);
        assert_eq!(refill.next_claim(), Some(width(5)));
        assert_eq!(refill.next_claim(), None);
    }

    #[test]
    fn a_lone_acknowledgement_with_no_claim_in_flight_refills_its_slot_at_once() {
        let mut refill = SlotRefill::new(width(4));
        assert_eq!(refill.next_claim(), Some(width(4)));
        refill.settle(width(4), 4);
        refill.free();
        assert_eq!(refill.next_claim(), Some(width(1)));
    }

    #[test]
    fn a_claim_that_comes_back_full_with_no_slot_freed_claims_nothing_further() {
        let mut refill = SlotRefill::new(width(4));
        assert_eq!(refill.next_claim(), Some(width(4)));
        refill.settle(width(4), 4);
        assert_eq!(refill.next_claim(), None);
    }

    #[test]
    fn a_slot_a_short_claim_left_empty_is_claimed_with_the_next_slot_that_frees() {
        let mut refill = SlotRefill::new(width(4));
        assert_eq!(refill.next_claim(), Some(width(4)));
        refill.settle(width(4), 1);
        assert_eq!(refill.next_claim(), None, "the slots left empty were claimed again at once");
        refill.free();
        assert_eq!(refill.next_claim(), Some(width(4)));
    }

    #[test]
    fn a_failed_claim_leaves_every_slot_it_asked_for_empty() {
        let mut refill = SlotRefill::new(width(4));
        assert_eq!(refill.next_claim(), Some(width(4)));
        refill.settle(width(4), 4);
        refill.free();
        assert_eq!(refill.next_claim(), Some(width(1)));
        refill.settle(width(1), 0);
        refill.free();
        assert_eq!(refill.next_claim(), Some(width(2)));
    }
}
