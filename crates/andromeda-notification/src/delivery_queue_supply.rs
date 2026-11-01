//! Whether the queue may still supply due deliveries to a delivery pass.
//!
//! A pass claims for each slot that frees, and keeps claiming for as long as
//! every claim comes back filled. A claim that comes back short found no more
//! due deliveries, so the pass claims no further: a delivery that falls due
//! later, a retry whose backoff ends or one a commit queues, is left to the
//! next pass, which the commit's wake or the drain's fallback timer starts.

use std::num::NonZeroUsize;

/// Whether the queue may still hold due deliveries for the pass.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum QueueSupply {
    /// Every claim so far was filled, so more deliveries may be due.
    #[default]
    MayHoldMore,
    /// A claim came back short: the queue held no more due deliveries.
    Drained,
}

impl QueueSupply {
    /// The supply once a claim that asked for `asked` deliveries got `got`.
    pub(crate) const fn after_claim(self, asked: NonZeroUsize, got: usize) -> QueueSupply {
        match self {
            QueueSupply::MayHoldMore if got >= asked.get() => QueueSupply::MayHoldMore,
            QueueSupply::MayHoldMore | QueueSupply::Drained => QueueSupply::Drained,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::delivery_queue_supply::QueueSupply;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroUsize;

    /// A width of `count` deliveries, `count` being positive.
    const fn width(count: usize) -> NonZeroUsize {
        NonZeroUsize::MIN.saturating_add(count.saturating_sub(1))
    }

    #[test]
    fn a_filled_claim_leaves_the_queue_able_to_supply_more() {
        assert_eq!(QueueSupply::MayHoldMore.after_claim(width(4), 4), QueueSupply::MayHoldMore);
    }

    #[test]
    fn a_short_claim_drains_the_supply() {
        assert_eq!(QueueSupply::MayHoldMore.after_claim(width(4), 3), QueueSupply::Drained);
        assert_eq!(QueueSupply::MayHoldMore.after_claim(width(1), 0), QueueSupply::Drained);
    }

    /// A refill already in flight when the queue ran out can still come back
    /// filled; the pass stays drained all the same.
    #[test]
    fn a_drained_supply_stays_drained_after_a_filled_claim() {
        assert_eq!(QueueSupply::Drained.after_claim(width(1), 1), QueueSupply::Drained);
    }
}
