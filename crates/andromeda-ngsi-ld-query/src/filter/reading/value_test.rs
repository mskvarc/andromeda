//! A comparison and its polarity, from which every verdict and every array
//! rule derives.
//!
//! Clause 4.9 states each operator as a condition a matching Entity meets,
//! and its negated operators as the complement over the same target, so a
//! test is held once, positively, and negation is its polarity. What a target
//! of each kind and an array of each kind make of the term follows from the
//! two, so no table of answers is stored beside them to disagree with them.

use crate::filter::reading::{array_rule::ArrayRule, array_target::ArrayTarget, scalar_test::ScalarTest, target_kind::TargetKind, verdict::Verdict};

/// Whether a term asserts its test or its complement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Polarity {
    /// The test holds on the target.
    Asserted,
    /// The test fails on a present target within the scope.
    Complement(ComplementScope),
}

/// The targets a negated operator matches when the test fails on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComplementScope {
    /// Every kind. A target the operand does not compare with "shall be
    /// considered unequal" (clause 4.9, Unequal): `!=` with a value, a list or
    /// a range.
    AnyKind,
    /// String targets only. "If the target value data type is different than
    /// String then it shall be considered as not matching" (clause 4.9, Do not
    /// match pattern): `!~=`.
    StringTarget,
}

/// A comparison and what a target makes of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueTest {
    /// The comparison, stated positively.
    test: ScalarTest,
    /// Whether the term asserts it or its complement.
    polarity: Polarity,
}

impl ValueTest {
    /// The term asserting `test` under `polarity`. Only lowering pairs the two,
    /// so a complement is scoped to every kind for `!=` and to Strings for
    /// `!~=`, and an order is never negated.
    pub(super) const fn new(test: ScalarTest, polarity: Polarity) -> ValueTest {
        ValueTest { test, polarity }
    }

    /// The comparison, stated positively.
    #[must_use]
    pub const fn test(&self) -> &ScalarTest {
        &self.test
    }

    /// Whether the term asserts the comparison or its complement.
    #[must_use]
    pub const fn polarity(&self) -> Polarity {
        self.polarity
    }

    /// What the term makes of one target of `kind`.
    ///
    /// For a kind the test compares with, the term holds as the test does, or
    /// as it fails under a complement. A number beyond the decimals orders the
    /// same against every finite operand, so its answer is a constant. Any
    /// other kind satisfies only a complement over every kind.
    #[must_use]
    pub fn verdict(&self, kind: TargetKind) -> Verdict<'_> {
        if !self.test.compares_with(kind) {
            return match self.polarity {
                Polarity::Complement(ComplementScope::AnyKind) => Verdict::Always,
                Polarity::Asserted | Polarity::Complement(ComplementScope::StringTarget) => Verdict::Never,
            };
        }
        match (kind.non_finite(), self.polarity) {
            (Some(value), Polarity::Asserted) if self.test.holds_beyond_decimals(value) => Verdict::Always,
            (Some(value), Polarity::Complement(_)) if !self.test.holds_beyond_decimals(value) => Verdict::Always,
            (Some(_), Polarity::Asserted | Polarity::Complement(_)) => Verdict::Never,
            (None, Polarity::Asserted) => Verdict::Holds(&self.test),
            (None, Polarity::Complement(_)) => Verdict::HoldsNot(&self.test),
        }
    }

    /// What the term makes of a target holding several values, as `target`.
    ///
    /// Any one string of a languageMap is the target value, so the term holds
    /// when one string passes the asserted test, or under a complement when
    /// none does. A value or object array is included in by an equality
    /// ("Is included in the target value, if the latter is an array", and its
    /// Unequal counterpart); to every other test it is another data type,
    /// which satisfies only a complement over every kind.
    #[must_use]
    pub const fn array_rule(&self, target: ArrayTarget) -> ArrayRule {
        match target {
            ArrayTarget::AnyOneValue => match self.polarity {
                Polarity::Asserted => ArrayRule::SomeElement,
                Polarity::Complement(_) => ArrayRule::NoElement,
            },
            ArrayTarget::ValueArray | ArrayTarget::ObjectArray => match (&self.test, self.polarity) {
                (ScalarTest::EqualsAny(_), Polarity::Asserted) => ArrayRule::SomeElement,
                (ScalarTest::EqualsAny(_), Polarity::Complement(_)) => ArrayRule::NoElement,
                (
                    ScalarTest::Ordered { .. } | ScalarTest::Within(_) | ScalarTest::Matches(_) | ScalarTest::Unsatisfiable,
                    Polarity::Asserted | Polarity::Complement(ComplementScope::StringTarget),
                ) => ArrayRule::Never,
                (
                    ScalarTest::Ordered { .. } | ScalarTest::Within(_) | ScalarTest::Matches(_) | ScalarTest::Unsatisfiable,
                    Polarity::Complement(ComplementScope::AnyKind),
                ) => ArrayRule::Always,
            },
        }
    }
}
