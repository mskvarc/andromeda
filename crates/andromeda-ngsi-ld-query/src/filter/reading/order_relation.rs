//! The four order relations a Query Term tests a target by.

use std::cmp::Ordering;

/// An ordering operator's relation of the target to the operand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OrderRelation {
    /// `>`.
    Greater,
    /// `>=`.
    GreaterOrEqual,
    /// `<`.
    Less,
    /// `<=`.
    LessOrEqual,
}

impl OrderRelation {
    /// Whether a target ordered `ordering` against the operand stands in the
    /// relation.
    #[must_use]
    pub const fn holds(self, ordering: Ordering) -> bool {
        match self {
            OrderRelation::Greater => ordering.is_gt(),
            OrderRelation::GreaterOrEqual => ordering.is_ge(),
            OrderRelation::Less => ordering.is_lt(),
            OrderRelation::LessOrEqual => ordering.is_le(),
        }
    }
}
