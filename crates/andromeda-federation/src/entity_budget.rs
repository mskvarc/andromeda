//! The most entities one forwarded query may gather from its context sources.
//!
//! Clause 4.3.6.7 asks the broker to "initially make a broad request to each
//! registered Context Source whose registration is matching the request" so it
//! can paginate the merged result consistently, and clause 5.5.9.1 makes the
//! whole of a source's answer the pages it links with `next`. Gathering all of
//! it is therefore the conformant behaviour, and it has to be bounded: clause
//! 4.12 recommends "defining a hard limit to the size of generated response
//! payload body while paginating", naming `TooManyResults` as the refusal, and
//! the error type's definition leaves the threshold "up to each
//! implementation". The budget is that threshold, shared by every source one
//! request contacts -- local answers included, which the caller checks against
//! the same maximum.

use std::{
    num::NonZeroU32,
    sync::atomic::{AtomicU64, Ordering},
};
use thiserror::Error;

/// The entities one forwarded query may still gather, shared by every source
/// it contacts concurrently.
#[derive(Debug)]
pub struct EntityBudget {
    /// The most entities the query may gather.
    maximum: NonZeroU32,
    /// The entities gathered so far, across every source.
    gathered: AtomicU64,
}

/// A forwarded query gathered more entities than its budget allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("the query matches more than the {maximum} entities this broker gathers from its context sources for one request")]
pub struct EntityBudgetExceeded {
    /// The budget that was exceeded.
    pub maximum: NonZeroU32,
}

impl EntityBudget {
    /// A budget allowing `maximum` entities.
    #[must_use]
    pub const fn new(maximum: NonZeroU32) -> EntityBudget {
        EntityBudget {
            maximum,
            gathered: AtomicU64::new(0),
        }
    }

    /// The most entities the query may gather.
    #[must_use]
    pub const fn maximum(&self) -> NonZeroU32 {
        self.maximum
    }

    /// Charge `entities` newly gathered entities against the budget.
    ///
    /// # Errors
    ///
    /// Returns [`EntityBudgetExceeded`] once everything gathered so far, by
    /// this source and every other one of the request, exceeds the maximum.
    pub fn charge(&self, entities: usize) -> Result<(), EntityBudgetExceeded> {
        let entities = u64::try_from(entities).unwrap_or(u64::MAX);
        // Relaxed is enough: the counter is the only state shared, and every
        // charge sees every earlier one through the atomic addition itself.
        let gathered = self.gathered.fetch_add(entities, Ordering::Relaxed).saturating_add(entities);
        self.admits(gathered)
    }

    /// Whether a collection of `entities` entities fits the budget.
    ///
    /// # Errors
    ///
    /// Returns [`EntityBudgetExceeded`] when it does not.
    pub fn admits(&self, entities: u64) -> Result<(), EntityBudgetExceeded> {
        if entities > u64::from(self.maximum.get()) {
            Err(EntityBudgetExceeded { maximum: self.maximum })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity_budget::{EntityBudget, EntityBudgetExceeded};
    use std::num::NonZeroU32;

    fn budget(maximum: u32) -> EntityBudget {
        EntityBudget::new(NonZeroU32::new(maximum).expect("a positive maximum"))
    }

    /// Gathering up to the maximum is admitted; one entity past it is not.
    #[test]
    fn the_budget_admits_exactly_its_maximum() {
        let budget = budget(10);
        assert!(budget.charge(4).is_ok());
        assert!(budget.charge(6).is_ok());
        pretty_assertions::assert_eq!(
            budget.charge(1),
            Err(EntityBudgetExceeded {
                maximum: NonZeroU32::new(10).expect("ten")
            })
        );
    }

    /// Every source of one request draws on the same budget.
    #[test]
    fn charges_from_several_sources_add_up() {
        let budget = budget(1_000);
        let sources = [400_usize, 400, 400];
        let outcomes: Vec<bool> = sources.iter().map(|gathered| budget.charge(*gathered).is_ok()).collect();
        pretty_assertions::assert_eq!(outcomes, vec![true, true, false]);
    }

    /// A collection is admitted against the same maximum.
    #[test]
    fn a_collection_is_admitted_up_to_the_maximum() {
        let budget = budget(3);
        assert!(budget.admits(3).is_ok());
        assert!(budget.admits(4).is_err());
    }
}
