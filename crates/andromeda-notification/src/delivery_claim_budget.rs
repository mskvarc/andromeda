//! How many more deliveries one delivery pass may claim.

/// Whether the queue may still hold due deliveries for this pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QueueSupply {
    /// Every claim so far was filled, so more deliveries may be due.
    MayHoldMore,
    /// A claim came back short: the queue held no more due deliveries.
    Drained,
}

/// The claims one delivery pass may still make: the part of its batch it has
/// not claimed, and whether the queue has run out.
#[derive(Debug)]
pub(crate) struct ClaimBudget {
    /// Deliveries the pass may still claim before it reaches its batch bound.
    unclaimed: usize,
    /// Deliveries the pass has claimed so far.
    claimed: usize,
    /// Whether the queue may still hold due deliveries.
    supply: QueueSupply,
}

impl ClaimBudget {
    /// The budget of a pass that may claim up to `batch_size` deliveries.
    pub(crate) const fn new(batch_size: usize) -> ClaimBudget {
        ClaimBudget {
            unclaimed: batch_size,
            claimed: 0,
            supply: QueueSupply::MayHoldMore,
        }
    }

    /// Reserve up to `wanted` deliveries for one claim, returning how many it
    /// may ask for.
    pub(crate) fn reserve(&mut self, wanted: usize) -> usize {
        let width = wanted.min(self.unclaimed);
        self.unclaimed -= width;
        width
    }

    /// Record that a claim asked for `asked` deliveries and got `got`.
    ///
    /// A claim that came back short found the queue out of due deliveries, so
    /// no further claim of this pass would find any either.
    pub(crate) const fn record(&mut self, asked: usize, got: usize) {
        self.claimed += got;
        if got < asked {
            self.supply = QueueSupply::Drained;
        }
    }

    /// Whether the pass may claim another delivery: its batch is not spent and
    /// the queue has not run out.
    pub(crate) const fn can_claim_more(&self) -> bool {
        self.unclaimed > 0 && matches!(self.supply, QueueSupply::MayHoldMore)
    }

    /// Deliveries the pass has claimed so far.
    pub(crate) const fn claimed(&self) -> usize {
        self.claimed
    }

    /// Whether another pass could find work: this one stopped at its batch
    /// bound, not at an empty queue.
    pub(crate) const fn work_may_remain(&self) -> bool {
        matches!(self.supply, QueueSupply::MayHoldMore)
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_claim_budget::ClaimBudget;

    #[test]
    fn a_reservation_never_exceeds_what_the_batch_has_left() {
        let mut budget = ClaimBudget::new(5);
        pretty_assertions::assert_eq!(budget.reserve(4), 4);
        pretty_assertions::assert_eq!(budget.reserve(4), 1);
        pretty_assertions::assert_eq!(budget.reserve(4), 0);
        assert!(!budget.can_claim_more(), "the batch is spent");
    }

    /// A pass that spent its batch on filled claims stopped at its bound, and
    /// another pass may find work.
    #[test]
    fn a_batch_spent_on_filled_claims_leaves_work_that_may_remain() {
        let mut budget = ClaimBudget::new(2);
        let asked = budget.reserve(2);
        budget.record(asked, 2);
        pretty_assertions::assert_eq!(budget.claimed(), 2);
        assert!(!budget.can_claim_more());
        assert!(budget.work_may_remain());
    }

    /// A claim that came back short found the queue empty: the pass claims no
    /// more and reports that no work remains.
    #[test]
    fn a_short_claim_drains_the_supply() {
        let mut budget = ClaimBudget::new(10);
        let asked = budget.reserve(4);
        budget.record(asked, 3);
        pretty_assertions::assert_eq!(budget.claimed(), 3);
        assert!(!budget.can_claim_more(), "the queue ran out");
        assert!(!budget.work_may_remain());
    }
}
