//! The comparison a Query Term makes, and the target kinds it compares with.

use crate::{
    filter::reading::{
        comparator::Comparator,
        interval::Interval,
        operand::{Comparison, Operand},
        order_relation::OrderRelation,
        target_kind::TargetKind,
    },
    pattern::Pattern,
};
use andromeda_ngsi_ld_number::declared_number::NonFinite;

/// What a Query Term asks of one target value, before any negation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScalarTest {
    /// Equal to one of the operands: one for `==` and `!=` with a value,
    /// several for a value list, each comparing with its own kinds.
    EqualsAny(Vec<Operand>),
    /// Ordered against the operand.
    Ordered {
        /// The relation the target stands in to the operand.
        relation: OrderRelation,
        /// The operand.
        operand: Operand,
    },
    /// Inside the closed interval.
    Within(Interval),
    /// Found anywhere in a String target: clause 4.9's "in the L(R) of the
    /// regular pattern" is read as a search, as IEEE 1003.2 matching runs.
    Matches(Pattern),
    /// A range whose bounds are of two kinds, which no target kind compares
    /// with.
    Unsatisfiable,
}

impl ScalarTest {
    /// Whether the test compares with a target of `kind`: clause 4.9 makes a
    /// target of any other kind "not matching" for every operator, and
    /// "unequal" under `!=`.
    #[must_use]
    pub fn compares_with(&self, kind: TargetKind) -> bool {
        match self {
            ScalarTest::EqualsAny(operands) => operands.iter().any(|operand| operand.compares_with(kind, Comparison::Equality)),
            ScalarTest::Ordered { operand, .. } => operand.compares_with(kind, Comparison::Order),
            ScalarTest::Within(interval) => interval.min().compares_with(kind, Comparison::Order),
            ScalarTest::Matches(_) => matches!(kind, TargetKind::String),
            ScalarTest::Unsatisfiable => false,
        }
    }

    /// Puts the operands of a value list in canonical order, each once: the
    /// list holds when one operand is equal, so neither their order nor a
    /// repetition is part of what it asks.
    pub(super) fn order_operands(&mut self) {
        if let ScalarTest::EqualsAny(operands) = self {
            operands.sort();
            operands.dedup();
        }
    }

    /// Whether the test holds on `value`, a number beyond the decimals, in the
    /// order the number primitive gives it against every finite operand.
    #[must_use]
    pub fn holds_beyond_decimals(&self, value: NonFinite) -> bool {
        match self {
            ScalarTest::EqualsAny(operands) => operands.iter().any(|operand| Comparator::Equal.holds_beyond_decimals(value, operand)),
            ScalarTest::Ordered { relation, operand } => Comparator::Ordered(*relation).holds_beyond_decimals(value, operand),
            ScalarTest::Within(interval) => {
                Comparator::Ordered(OrderRelation::GreaterOrEqual).holds_beyond_decimals(value, interval.min())
                    && Comparator::Ordered(OrderRelation::LessOrEqual).holds_beyond_decimals(value, interval.max())
            }
            ScalarTest::Matches(_) | ScalarTest::Unsatisfiable => false,
        }
    }
}
