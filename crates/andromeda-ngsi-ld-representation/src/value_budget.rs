//! How many values of one entity a request body is read into before the rest
//! is skipped.
//!
//! The entity value limit (ETSI GS CIM 009 v1.9.1 clause 6.3.2) refuses an
//! entity that holds too many values, but only once the entity is decoded, and
//! a member of millions of entries costs the broker all of them first. The
//! members that grow without bound (the targets of a Relationship, the terms of
//! a `VocabProperty`, the entries of a language map, the entries of a list, the
//! scopes) are read against this budget instead. It admits one value more than
//! the limit holds, so an entity cut short is certain to be refused by the
//! limit when it is counted, and an entity within the limit is never cut: every
//! value the budget admits is a value the count makes.

use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use std::cell::Cell;

/// The values of one entity that may still be recorded.
#[derive(Debug)]
pub struct ValueBudget {
    /// The values the budget still admits.
    remaining: Cell<usize>,
}

impl ValueBudget {
    /// A budget for an entity held to `limit` values, which admits `limit`
    /// values and one more.
    #[must_use]
    pub const fn new(limit: JsonValueLimit) -> ValueBudget {
        ValueBudget {
            remaining: Cell::new(limit.get().saturating_add(1)),
        }
    }

    /// Record one value; a spent budget stays spent.
    pub fn spend(&self) {
        self.remaining.set(self.remaining.get().saturating_sub(1));
    }

    /// Give `values` back: they were recorded and then replaced by an entry that
    /// stands for them, so the count does not make them.
    pub fn refund(&self, values: usize) {
        self.remaining.set(self.remaining.get().saturating_add(values));
    }

    /// Whether the budget admits no further value.
    #[must_use]
    pub const fn is_spent(&self) -> bool {
        self.remaining.get() == 0
    }
}

#[cfg(test)]
mod tests {
    use crate::value_budget::ValueBudget;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use std::num::NonZeroUsize;

    fn budget(limit: usize) -> ValueBudget {
        ValueBudget::new(JsonValueLimit::new(NonZeroUsize::new(limit).unwrap_or(NonZeroUsize::MIN)))
    }

    #[test]
    fn a_budget_holds_the_limit_and_one_more() {
        let budget = budget(2);
        for _ in 0..2 {
            budget.spend();
            assert!(!budget.is_spent());
        }
        budget.spend();
        assert!(budget.is_spent());
    }

    #[test]
    fn a_spent_budget_stays_spent_however_much_is_spent_after() {
        let budget = budget(1);
        for _ in 0..5 {
            budget.spend();
        }
        assert!(budget.is_spent());
    }

    #[test]
    fn a_refunded_value_leaves_the_budget_unspent() {
        let budget = budget(1);
        budget.spend();
        budget.spend();
        assert!(budget.is_spent());
        budget.refund(1);
        assert!(!budget.is_spent());
    }
}
