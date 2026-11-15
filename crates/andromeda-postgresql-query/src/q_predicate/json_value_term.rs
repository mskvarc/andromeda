//! A clause 4.9 comparison decided against one JSON value.
//!
//! Two kinds of target value are only reachable as JSON: a member of a
//! compound value (`address[city]`), read out of a `value_object`, and every
//! target a temporal `q` reaches through a history instance rebuilt as an
//! expanded document. Both read one JSONB value per candidate and lift it to
//! the comparison domain the operand names, so the comparisons are written
//! once here and each reading only says where its values come from and how it
//! combines them.

use crate::{
    declared_operand::declares_other_data_type,
    jsonb::lift_comparison,
    operator::{PatternIndexes, compare, pattern as pattern_predicate},
    q_predicate::string_ordering::ordered_string,
};
use andromeda_ngsi_ld_query::{
    filter::reading::{comparator::Comparator, interval::Interval, operand::Operand, order_relation::OrderRelation, value_test::Polarity},
    pattern::Pattern,
};
use andromeda_postgresql_seaquery::{
    cast::cast_compound,
    json_type::{JsonType, jsonb_type_is, jsonb_whole_text},
    operator::{negate_total, present_and},
    pg_type::PostgresType,
};
use sea_query::{Condition, ExprTrait, SimpleExpr};

/// One JSON target value.
#[derive(Debug, Clone)]
pub(crate) struct JsonTarget {
    /// The value, as JSONB; SQL `NULL` where there is none.
    pub(crate) value: SimpleExpr,
    /// The expanded datatype IRI the value is declared as, as text; SQL `NULL`
    /// where it declares none.
    pub(crate) datatype: SimpleExpr,
    /// Whether the value is present at all.
    pub(crate) presence: SimpleExpr,
}

/// Compare a stored JSONB value against one operand.
///
/// Everything that decides the comparison domain (the JSON type the operand
/// reads, the cast a number or boolean needs, the guarded `timestamptz` comparison a `DateTime` needs, the
/// normalisation a `Time` needs) lives in [`lift_comparison`], so this reading
/// and the CSR renderer compare the same way. A String ordering compares under
/// the collation [`ordered_string`] picks.
fn operand_predicate(stored: SimpleExpr, datatype: SimpleExpr, operand: &Operand, comparator: Comparator) -> SimpleExpr {
    let lifted = lift_comparison(operand, stored, datatype);
    compare(ordered_string(lifted.stored, operand, comparator), comparator, lifted.operand)
}

/// Whether the target equals one of `operands`: the one of `==` with a value,
/// or one member of a value list, which is a set of equality comparisons
/// (clause 4.9 Table 4.9-1).
///
/// A value of another JSON type than the operand lifts to `NULL` and leaves
/// the comparison unknown, which is not a match; the reading negates this
/// condition totally for `unequal`, which clause 4.9 makes hold for it: "If the
/// data type of the target value and the data type of the Query Term value are
/// different, then they shall be considered unequal."
pub(crate) fn json_equals_any(target: &JsonTarget, operands: &[Operand]) -> Condition {
    // clone: sea-query owns every operand, and one target is compared by several arms
    let equals = |operand: &Operand| operand_predicate(target.value.clone(), target.datatype.clone(), operand, Comparator::Equal);
    match operands {
        [operand] => Condition::all().add(equals(operand)),
        operands => operands.iter().fold(Condition::any(), |cond, operand| cond.add(equals(operand))),
    }
}

/// Whether the target lies inside `interval`, both bounds included (clause
/// 4.9 `Range`), negated totally for `unequal` as [`json_equals_any`] is.
pub(crate) fn json_within(target: &JsonTarget, interval: &Interval) -> Condition {
    // clone: sea-query owns every operand, and one target is compared by both bounds
    let stored = || target.value.clone();
    // Both bounds compare against the one lifted stored side; lifting the
    // maximum only contributes its operand.
    let lower = lift_comparison(interval.min(), stored(), target.datatype.clone());
    let upper_operand = lift_comparison(interval.max(), stored(), target.datatype.clone()).operand;
    let stored_side = lower.stored;
    // Each bound orders the stored side under its own collation.
    let bounded = |relation: OrderRelation, bound: &Operand, operand: SimpleExpr| {
        let comparator = Comparator::Ordered(relation);
        compare(ordered_string(stored_side.clone(), bound, comparator), comparator, operand) // clone: the stored side is read by both halves
    };
    Condition::all()
        .add(bounded(OrderRelation::GreaterOrEqual, interval.min(), lower.operand))
        .add(bounded(OrderRelation::LessOrEqual, interval.max(), upper_operand))
}

/// Decide an ordering Query Term (`>`/`>=`/`<`/`<=`) against one target.
///
/// A value of another data type than the operand lifts to `NULL`, which does
/// not match: clause 4.9 "If there is no equality between the target value
/// data type and the Query Term value data type then it shall be considered as
/// not matching".
pub(crate) fn json_ordering(target: &JsonTarget, relation: OrderRelation, operand: &Operand) -> Condition {
    // clone: the target may be read by further terms
    Condition::all().add(operand_predicate(
        target.value.clone(),
        target.datatype.clone(),
        operand,
        Comparator::Ordered(relation),
    ))
}

/// Decide a pattern Query Term (`~=`/`!~=`) against one target.
///
/// Both polarities require the element to be present (clause 4.9), which is
/// what [`present_and`] states once for every backend, and the target to be a
/// String: "If the target value data type is different than String then it
/// shall be considered as not matching". A JSON string declared as a time
/// value, a number or a Boolean is that, not a String.
pub(crate) fn json_pattern(target: &JsonTarget, pattern: &Pattern, polarity: Polarity) -> Condition {
    let text = cast_compound(jsonb_whole_text(target.value.clone()), PostgresType::Text); // clone: the target may be read by further terms
    let value = pattern_predicate(text, polarity, pattern, PatternIndexes::None);
    let string = jsonb_type_is(target.value.clone(), JsonType::String).and(negate_total(declares_other_data_type(target.datatype.clone())));
    Condition::all().add(present_and(target.presence.clone().and(string), value))
}
