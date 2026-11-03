//! How many values of one entity a request body is read into before the rest
//! is skipped.
//!
//! The entity value limit (ETSI GS CIM 009 v1.9.1 clause 6.3.2) refuses an
//! entity that holds too many values, but only once the entity is decoded, and
//! a member of millions of entries costs the broker all of them first. The
//! members that grow without bound (the targets of a Relationship, the terms of
//! a `VocabProperty`, the entries of a language map, the entries of a list, the
//! scopes, and the attributes, instances and sub-attributes themselves) are
//! read against this budget instead. It admits one value more than
//! the limit holds, so an entity cut short is certain to be refused by the
//! limit when it is counted, and an entity within the limit is never cut: every
//! value the budget admits is a value the count makes.

use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use std::cell::Cell;

/// The values of one entity that may still be recorded, and the attributes,
/// instances and sub-attributes that may still be.
///
/// The two are counted apart. The entity count makes every value of the first
/// beside the structure that holds it, so a structural element spent from the
/// same budget as the values would cut an entity the count admits. Each is
/// admitted one past the limit, and an element of structure costs the count at
/// least one value, so either being spent makes the entity certain to be
/// refused.
#[derive(Debug)]
pub struct ValueBudget {
    /// The values the budget still admits.
    remaining: Cell<usize>,
    /// The attributes, instances and sub-attributes the budget still admits.
    remaining_structure: Cell<usize>,
}

impl ValueBudget {
    /// A budget for an entity held to `limit` values, which admits `limit`
    /// values and one more, and as many elements of structure.
    #[must_use]
    pub const fn new(limit: JsonValueLimit) -> ValueBudget {
        ValueBudget {
            remaining: Cell::new(limit.get().saturating_add(1)),
            remaining_structure: Cell::new(limit.get().saturating_add(1)),
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

    /// Record one attribute, instance or sub-attribute; a spent budget stays
    /// spent.
    pub fn spend_structure(&self) {
        self.remaining_structure.set(self.remaining_structure.get().saturating_sub(1));
    }

    /// Whether the budget admits no further attribute, instance or
    /// sub-attribute.
    #[must_use]
    pub const fn is_structure_spent(&self) -> bool {
        self.remaining_structure.get() == 0
    }

    /// Note what the budget admits now, to measure what is spent after it.
    #[must_use]
    pub const fn mark(&self) -> BudgetMark {
        BudgetMark {
            values: self.remaining.get(),
            structure: self.remaining_structure.get(),
        }
    }

    /// What was spent since `mark` was taken, net of refunds.
    #[must_use]
    pub const fn spent_since(&self, mark: BudgetMark) -> Spent {
        Spent {
            values: mark.values.saturating_sub(self.remaining.get()),
            structure: mark.structure.saturating_sub(self.remaining_structure.get()),
        }
    }

    /// Give back everything `spent` records: an element recorded and then
    /// replaced by a later one stands for none of it.
    pub fn refund_spent(&self, spent: Spent) {
        self.remaining.set(self.remaining.get().saturating_add(spent.values));
        self.remaining_structure.set(self.remaining_structure.get().saturating_add(spent.structure));
    }
}

/// What a [`ValueBudget`] admitted at one moment.
#[derive(Clone, Copy, Debug)]
pub struct BudgetMark {
    /// The values it admitted.
    values: usize,
    /// The elements of structure it admitted.
    structure: usize,
}

/// What was spent from a [`ValueBudget`] over a span.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Spent {
    /// The values spent.
    values: usize,
    /// The elements of structure spent.
    structure: usize,
}

#[cfg(test)]
mod tests {
    use crate::value_budget::{Spent, ValueBudget};
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
    fn what_is_spent_since_a_mark_is_net_of_refunds() {
        let budget = budget(10);
        let mark = budget.mark();
        budget.spend();
        budget.spend();
        budget.spend_structure();
        budget.refund(1);
        let spent = budget.spent_since(mark);
        assert!(!budget.is_structure_spent());
        budget.refund_spent(spent);
        assert!(!budget.is_spent());
        pretty_assertions::assert_eq!(budget.spent_since(mark), Spent::default());
    }

    #[test]
    fn structure_is_spent_apart_from_values() {
        let budget = budget(1);
        budget.spend_structure();
        budget.spend_structure();
        assert!(budget.is_structure_spent());
        assert!(!budget.is_spent());
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
