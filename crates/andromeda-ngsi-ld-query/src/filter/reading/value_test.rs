//! A comparison and its polarity, from which every verdict and every array
//! rule derives.
//!
//! Clause 4.9 states each operator as a condition a matching Entity meets,
//! and its negated operators as the complement over the same target, so a
//! test is held once, positively, and negation is its polarity. What a target
//! of each kind and an array of each kind make of the term follows from the
//! two, so no table of answers is stored beside them to disagree with them.

use crate::filter::reading::{
    array_rule::ArrayRule,
    array_target::ArrayTarget,
    operand::Operand,
    scalar_test::ScalarTest,
    target_kind::TargetKind,
    verdict::Verdict,
};

/// Whether a term asserts its test or its complement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Polarity {
    /// The test holds on the target.
    Asserted,
    /// The test fails on a present target of a kind the complement of the
    /// test scopes: every kind for a test `!=` negates, Strings for the
    /// pattern `!~=` negates.
    Complement,
}

/// The targets a negated operator matches when the test fails on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ValueTest {
    /// The comparison, stated positively.
    test: ScalarTest,
    /// Whether the term asserts it or its complement.
    polarity: Polarity,
}

impl ValueTest {
    /// The term asserting `test` under `polarity`. Only lowering pairs the
    /// two, so an order is never negated.
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

    /// The targets the complement the term asserts ranges over, `None` when
    /// the term asserts its test.
    #[must_use]
    pub const fn complement_scope(&self) -> Option<ComplementScope> {
        match self.polarity {
            Polarity::Asserted => None,
            Polarity::Complement => Some(scope_of(&self.test)),
        }
    }

    /// Puts the operands of a value list in canonical order, each once.
    pub(super) fn order_operands(&mut self) {
        self.test.order_operands();
    }

    /// The operands of an asserted equality, `==` with a value or a value
    /// list, `None` for any other test.
    pub(super) const fn asserted_equality_mut(&mut self) -> Option<&mut Vec<Operand>> {
        match (&mut self.test, self.polarity) {
            (ScalarTest::EqualsAny(operands), Polarity::Asserted) => Some(operands),
            (ScalarTest::EqualsAny(_), Polarity::Complement)
            | (
                ScalarTest::Ordered { .. } | ScalarTest::Within(_) | ScalarTest::Matches(_) | ScalarTest::Unsatisfiable,
                Polarity::Asserted | Polarity::Complement,
            ) => None,
        }
    }

    /// What the term makes of one target of `kind`.
    ///
    /// For a kind the test compares with, the term holds as the test does, or
    /// as it fails under a complement. A number beyond the decimals orders the
    /// same against every finite operand, so its answer is a constant. Any
    /// other kind satisfies only a complement over every kind.
    #[must_use]
    pub fn verdict(&self, kind: TargetKind) -> Verdict<'_> {
        verdict_under(&self.test, self.polarity, kind)
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
        array_rule_under(&self.test, self.polarity, target)
    }

    /// What the asserted test makes of one value of `kind`, whatever the
    /// term's polarity: an element of an array target is tested with it, the
    /// array rule deciding what the term makes of the answers, and a
    /// complement is the failure of the asserted test over the targets in its
    /// scope.
    #[must_use]
    pub fn asserted_verdict(&self, kind: TargetKind) -> Verdict<'_> {
        verdict_under(&self.test, Polarity::Asserted, kind)
    }

    /// What the asserted test makes of a target holding several values, as
    /// `target`: the rule an element of an array target that holds several
    /// values itself is tested by, at every depth.
    #[must_use]
    pub const fn asserted_array_rule(&self, target: ArrayTarget) -> ArrayRule {
        array_rule_under(&self.test, Polarity::Asserted, target)
    }
}

/// The targets the complement of `test` matches when the test fails on them.
///
/// The pattern is negated by `!~=`, whose complement is over Strings, and every
/// other test by `!=`, whose complement is over every kind. An order is never
/// negated, and is read as `!=` reads a range.
const fn scope_of(test: &ScalarTest) -> ComplementScope {
    match test {
        ScalarTest::Matches(_) => ComplementScope::StringTarget,
        ScalarTest::EqualsAny(_) | ScalarTest::Ordered { .. } | ScalarTest::Within(_) | ScalarTest::Unsatisfiable => ComplementScope::AnyKind,
    }
}

/// What `test` under `polarity` makes of one target of `kind`.
fn verdict_under(test: &ScalarTest, polarity: Polarity, kind: TargetKind) -> Verdict<'_> {
    if !test.compares_with(kind) {
        return match (polarity, scope_of(test)) {
            (Polarity::Complement, ComplementScope::AnyKind) => Verdict::Always,
            (Polarity::Asserted, ComplementScope::AnyKind | ComplementScope::StringTarget) | (Polarity::Complement, ComplementScope::StringTarget) => {
                Verdict::Never
            }
        };
    }
    match (kind.non_finite(), polarity) {
        (Some(value), Polarity::Asserted) if test.holds_beyond_decimals(value) => Verdict::Always,
        (Some(value), Polarity::Complement) if !test.holds_beyond_decimals(value) => Verdict::Always,
        (Some(_), Polarity::Asserted | Polarity::Complement) => Verdict::Never,
        (None, Polarity::Asserted) => Verdict::Holds(test),
        (None, Polarity::Complement) => Verdict::HoldsNot(test),
    }
}

/// What `test` under `polarity` makes of a target holding several values, as
/// `target`.
const fn array_rule_under(test: &ScalarTest, polarity: Polarity, target: ArrayTarget) -> ArrayRule {
    match target {
        ArrayTarget::AnyOneValue => match polarity {
            Polarity::Asserted => ArrayRule::SomeElement,
            Polarity::Complement => ArrayRule::NoElement,
        },
        ArrayTarget::ValueArray | ArrayTarget::ObjectArray => match (test, polarity) {
            (ScalarTest::EqualsAny(_), Polarity::Asserted) => ArrayRule::SomeElement,
            (ScalarTest::EqualsAny(_), Polarity::Complement) => ArrayRule::NoElement,
            (ScalarTest::Ordered { .. } | ScalarTest::Within(_) | ScalarTest::Matches(_) | ScalarTest::Unsatisfiable, Polarity::Asserted) => ArrayRule::Never,
            (ScalarTest::Ordered { .. } | ScalarTest::Within(_) | ScalarTest::Matches(_) | ScalarTest::Unsatisfiable, Polarity::Complement) => {
                match scope_of(test) {
                    ComplementScope::AnyKind => ArrayRule::Always,
                    ComplementScope::StringTarget => ArrayRule::Never,
                }
            }
        },
    }
}
