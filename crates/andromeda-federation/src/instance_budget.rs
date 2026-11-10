//! The most attribute instances one forwarded temporal query may gather from
//! its context sources when it reads their history to the end.
//!
//! An aggregated temporal representation of an entity held in several parts is
//! an aggregate of the merged instances (clause 5.7.4.4), so the instances each
//! source holds in the window are read to the end of its clause 6.3.10 pages
//! rather than one page of them. Clause 4.12 recommends "defining a hard limit
//! to the size of generated response payload body while paginating" and names
//! `TooManyResults` as the refusal; the budget is that limit, counted in
//! instances, shared by every source the request contacts.

use std::{
    num::NonZeroU32,
    sync::atomic::{AtomicU64, Ordering},
};
use thiserror::Error;

/// The attribute instances one forwarded temporal query may still gather,
/// shared by every source it contacts concurrently.
#[derive(Debug)]
pub struct InstanceBudget {
    /// The most instances the query may gather.
    maximum: NonZeroU32,
    /// The instances gathered so far, across every source.
    gathered: AtomicU64,
}

/// A forwarded temporal query gathered more attribute instances than its
/// budget allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error(
    "the query matches more than the {maximum} attribute instances this broker gathers from its context sources for one request (federation.query.max_instances)"
)]
pub struct InstanceBudgetExceeded {
    /// The budget that was exceeded.
    pub maximum: NonZeroU32,
}

impl InstanceBudget {
    /// A budget allowing `maximum` instances.
    #[must_use]
    pub const fn new(maximum: NonZeroU32) -> InstanceBudget {
        InstanceBudget {
            maximum,
            gathered: AtomicU64::new(0),
        }
    }

    /// Charge `instances` newly gathered instances against the budget.
    ///
    /// # Errors
    ///
    /// Returns [`InstanceBudgetExceeded`] once everything gathered so far, by
    /// this source and every other one of the request, exceeds the maximum.
    pub fn charge(&self, instances: usize) -> Result<(), InstanceBudgetExceeded> {
        let instances = u64::try_from(instances).unwrap_or(u64::MAX);
        // Relaxed is enough: the counter is the only state shared, and every
        // charge sees every earlier one through the atomic addition itself.
        let gathered = self.gathered.fetch_add(instances, Ordering::Relaxed).saturating_add(instances);
        if gathered > u64::from(self.maximum.get()) {
            Err(InstanceBudgetExceeded { maximum: self.maximum })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::instance_budget::{InstanceBudget, InstanceBudgetExceeded};
    use std::num::NonZeroU32;

    /// Gathering up to the maximum is admitted; one instance past it is not.
    #[test]
    fn the_budget_admits_exactly_its_maximum() {
        let maximum = NonZeroU32::new(10).expect("ten");
        let budget = InstanceBudget::new(maximum);
        assert!(budget.charge(4).is_ok());
        assert!(budget.charge(6).is_ok());
        pretty_assertions::assert_eq!(budget.charge(1), Err(InstanceBudgetExceeded { maximum }));
    }
}
