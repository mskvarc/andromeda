//! The closed interval a range operand spells.

use crate::filter::reading::operand::Operand;
use std::mem;

/// The two bounds of a clause 4.9 `Range`, inclusive and of one kind.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Interval {
    /// The lower bound.
    min: Operand,
    /// The upper bound.
    max: Operand,
}

impl Interval {
    /// The interval from `min` to `max`, or `None` when the bounds are of two
    /// kinds: clause 4.9 defines no interval between values of two data types.
    #[must_use]
    pub fn new(min: Operand, max: Operand) -> Option<Interval> {
        (mem::discriminant(&min) == mem::discriminant(&max)).then_some(Interval { min, max })
    }

    /// The lower bound.
    #[must_use]
    pub const fn min(&self) -> &Operand {
        &self.min
    }

    /// The upper bound.
    #[must_use]
    pub const fn max(&self) -> &Operand {
        &self.max
    }
}
