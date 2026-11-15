//! Running one value test on one target (clause 4.9).
//!
//! What a target of each kind makes of a test, and what an array of each kind
//! makes of it, is the reading's to say: [`ValueTest::verdict`] and
//! [`ValueTest::array_rule`] derive both from the test and its polarity. What
//! is left here is to compare a value with an operand where the verdict asks
//! for it, through the order primitives and [`Pattern::is_match`].

use crate::q::target_value::{ResolvedTarget, TargetValue, ValueShape};
use andromeda_ngsi_ld_lexicon::time::value::TimeValue;
use andromeda_ngsi_ld_query::filter::reading::{
    array_rule::ArrayRule,
    family_reading::TargetOrigin,
    operand::{Comparison, Operand},
    scalar_test::ScalarTest,
    target_kind::TargetKind,
    value_test::ValueTest,
    verdict::Verdict,
};
use andromeda_ngsi_ld_value_order::string_order::string_order;
use std::cmp::Ordering;

/// Whether `target` satisfies `test`.
pub(crate) fn target_passes(test: &ValueTest, target: &ResolvedTarget<'_>) -> bool {
    match target.value.shape(target.origin) {
        ValueShape::One(kind) => verdict_holds(test.verdict(kind), &target.value, kind),
        ValueShape::Several(elements) => {
            let array = target.array_target();
            array_holds(test.array_rule(array), elements, |element| element_passes(test, element, target.origin))
        }
    }
}

/// Whether one element of an array target passes the asserted test.
///
/// Clause 4.9 compares an array target by "a single element" of it, so an
/// element that is itself an array is a value of another data type than any
/// operand and fails the asserted test, as the SQL renderings read it.
fn element_passes(test: &ValueTest, element: &TargetValue<'_>, origin: TargetOrigin) -> bool {
    match element.shape(origin) {
        ValueShape::One(kind) => verdict_holds(test.asserted_verdict(kind), element, kind),
        ValueShape::Several(_) => false,
    }
}

/// Whether `value`, of `kind`, satisfies a term whose verdict on its kind is
/// `verdict`.
fn verdict_holds(verdict: Verdict<'_>, value: &TargetValue<'_>, kind: TargetKind) -> bool {
    match verdict {
        Verdict::Never => false,
        Verdict::Always => true,
        Verdict::Holds(test) => test_holds(test, value, kind),
        Verdict::HoldsNot(test) => !test_holds(test, value, kind),
    }
}

/// Whether an array of `elements` satisfies a term whose rule for it is
/// `rule`, an element passing as `element_passes` says.
fn array_holds(rule: ArrayRule, elements: &[TargetValue<'_>], element_passes: impl Fn(&TargetValue<'_>) -> bool) -> bool {
    match rule {
        ArrayRule::Never => false,
        ArrayRule::Always => true,
        ArrayRule::SomeElement => elements.iter().any(element_passes),
        ArrayRule::NoElement => !elements.iter().any(element_passes),
    }
}

/// Whether `value`, of `kind`, a kind `test` compares with, passes it.
fn test_holds(test: &ScalarTest, value: &TargetValue<'_>, kind: TargetKind) -> bool {
    match test {
        ScalarTest::EqualsAny(operands) => operands
            .iter()
            .any(|operand| order_against(value, kind, operand, Comparison::Equality).is_some_and(Ordering::is_eq)),
        ScalarTest::Ordered { relation, operand } => order_against(value, kind, operand, Comparison::Order).is_some_and(|ordering| relation.holds(ordering)),
        ScalarTest::Within(interval) => {
            order_against(value, kind, interval.min(), Comparison::Order).is_some_and(Ordering::is_ge)
                && order_against(value, kind, interval.max(), Comparison::Order).is_some_and(Ordering::is_le)
        }
        ScalarTest::Matches(pattern) => value.as_text().is_some_and(|text| pattern.is_match(text.as_ref())),
        ScalarTest::Unsatisfiable => false,
    }
}

/// How `value`, of `kind`, orders against `operand` after operators of
/// `comparison`, or `None` when the two have no order between them: a value of
/// another data type than the operand's, or a not-a-number. A text operand is
/// read in the spelling a target of `kind` is compared with.
fn order_against(value: &TargetValue<'_>, kind: TargetKind, operand: &Operand, comparison: Comparison) -> Option<Ordering> {
    match operand {
        Operand::Number(number) => value.number_ordering(number),
        Operand::Text(_) | Operand::Uri(_) => {
            let text = operand.text_against(kind)?;
            value.as_text().map(|held| string_order(held.as_ref(), text, comparison.string_relation()))
        }
        Operand::Boolean(boolean) => value.as_boolean().map(|held| held.cmp(boolean)),
        Operand::DateTime(instant) => match value.as_time()? {
            TimeValue::DateTime(held) => Some(held.get().cmp(instant)),
            TimeValue::Date(_) | TimeValue::Time(_) => None,
        },
        Operand::Date(date) => match value.as_time()? {
            TimeValue::Date(held) => Some(held.get().cmp(date)),
            TimeValue::DateTime(_) | TimeValue::Time(_) => None,
        },
        Operand::Time(time) => match value.as_time()? {
            TimeValue::Time(held) => Some(held.get().cmp(time)),
            TimeValue::DateTime(_) | TimeValue::Date(_) => None,
        },
    }
}
