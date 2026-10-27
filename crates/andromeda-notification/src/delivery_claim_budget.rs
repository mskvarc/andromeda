//! How many more deliveries one delivery pass may claim.

use std::num::NonZeroUsize;

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
    pub(crate) const fn new(batch_size: NonZeroUsize) -> ClaimBudget {
        ClaimBudget {
            unclaimed: batch_size.get(),
            claimed: 0,
            supply: QueueSupply::MayHoldMore,
        }
    }

    /// Reserve up to `wanted` deliveries for one claim, returning how many it
    /// may ask for, or `None` once the batch is spent.
    pub(crate) fn reserve(&mut self, wanted: usize) -> Option<NonZeroUsize> {
        let width = NonZeroUsize::new(wanted.min(self.unclaimed))?;
        self.unclaimed -= width.get();
        Some(width)
    }

    /// Record that a claim asked for `asked` deliveries and got `got`.
    ///
    /// A claim that came back short found the queue out of due deliveries, so
    /// no further claim of this pass would find any either.
    pub(crate) const fn record(&mut self, asked: NonZeroUsize, got: usize) {
        self.claimed += got;
        if got < asked.get() {
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
    use std::num::NonZeroUsize;

    /// A width of `count` deliveries, `count` being positive.
    const fn width(count: usize) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add(count.saturating_sub(1))
    }

    #[test]
    fn a_reservation_never_exceeds_what_the_batch_has_left() {
        let mut budget = ClaimBudget::new(width(5));
        pretty_assertions::assert_eq!(budget.reserve(4), Some(width(4)));
        pretty_assertions::assert_eq!(budget.reserve(4), Some(width(1)));
        pretty_assertions::assert_eq!(budget.reserve(4), None);
        assert!(!budget.can_claim_more(), "the batch is spent");
    }

    /// A pass that spent its batch on filled claims stopped at its bound, and
    /// another pass may find work.
    #[test]
    fn a_batch_spent_on_filled_claims_leaves_work_that_may_remain() {
        let mut budget = ClaimBudget::new(width(2));
        pretty_assertions::assert_eq!(budget.reserve(2), Some(width(2)));
        budget.record(width(2), 2);
        pretty_assertions::assert_eq!(budget.claimed(), 2);
        assert!(!budget.can_claim_more());
        assert!(budget.work_may_remain());
    }

    /// A claim that came back short found the queue empty: the pass claims no
    /// more and reports that no work remains.
    #[test]
    fn a_short_claim_drains_the_supply() {
        let mut budget = ClaimBudget::new(width(10));
        pretty_assertions::assert_eq!(budget.reserve(4), Some(width(4)));
        budget.record(width(4), 3);
        pretty_assertions::assert_eq!(budget.claimed(), 3);
        assert!(!budget.can_claim_more(), "the queue ran out");
        assert!(!budget.work_may_remain());
    }
}
