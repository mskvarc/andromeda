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
    jsonb::lift_comparison,
    operator::{PatternIndexes, compare, pattern as pattern_predicate},
    q_predicate::string_ordering::ordered_string,
    temporal_operand::declares_time_value,
};
use andromeda_ngsi_ld_query::{
    filter::{
        predicate::{CmpOp, OrderingOp, PatternPolarity},
        scalar::{CompareValue, Scalar},
    },
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

/// Compare a stored JSONB value against one scalar operand.
///
/// Everything that decides the comparison domain (the JSON type the operand
/// reads, the cast a number or boolean needs, the guarded `timestamptz` comparison a `DateTime` needs, the
/// normalisation a `Time` needs) lives in [`lift_comparison`], so this reading
/// and the CSR renderer compare the same way. A String ordering compares under
/// the collation [`ordered_string`] picks.
fn scalar_predicate(stored: SimpleExpr, datatype: SimpleExpr, scalar: &Scalar, op: CmpOp) -> SimpleExpr {
    let lifted = lift_comparison(scalar, stored, datatype);
    compare(ordered_string(lifted.stored, scalar, op), op, lifted.operand)
}

/// Whether the target equals a scalar, lies inside a range, or equals one
/// member of a value list (clause 4.9 `equal`).
///
/// A value of another JSON type than the operand lifts to `NULL` and leaves
/// the comparison unknown, which is not a match; the reading negates this
/// condition totally for `unequal`, which clause 4.9 makes hold for it: "If the
/// data type of the target value and the data type of the Query Term value are
/// different, then they shall be considered unequal."
pub(crate) fn json_equal(target: &JsonTarget, rhs: &CompareValue) -> Condition {
    // clone: sea-query owns every operand, and one target is compared by several arms
    let stored = || target.value.clone();
    match rhs {
        CompareValue::Scalar(scalar) => Condition::all().add(scalar_predicate(stored(), target.datatype.clone(), scalar, CmpOp::Equal)),
        CompareValue::Range { min, max } => {
            // Both bounds compare against the one lifted stored side; lifting the
            // maximum only contributes its operand.
            let lower = lift_comparison(min, stored(), target.datatype.clone());
            let upper_operand = lift_comparison(max, stored(), target.datatype.clone()).operand;
            let stored_side = lower.stored;
            // Each bound orders the stored side under its own collation.
            let bounded = |op: CmpOp, bound: &Scalar, operand: SimpleExpr| compare(ordered_string(stored_side.clone(), bound, op), op, operand); // clone: the stored side is read by both halves
            Condition::all()
                .add(bounded(CmpOp::GreaterEq, min, lower.operand))
                .add(bounded(CmpOp::LessEq, max, upper_operand))
        }
        // A value list is a set of equality comparisons (clause 4.9 Table
        // 4.9-1).
        CompareValue::ValueList(values) => values.iter().fold(Condition::any(), |cond, value| {
            let lifted = lift_comparison(value, stored(), target.datatype.clone());
            cond.add(compare(lifted.stored, CmpOp::Equal, lifted.operand))
        }),
    }
}

/// Decide an ordering Query Term (`>`/`>=`/`<`/`<=`) against one target.
///
/// A value of another data type than the operand lifts to `NULL`, which does
/// not match: clause 4.9 "If there is no equality between the target value
/// data type and the Query Term value data type then it shall be considered as
/// not matching".
pub(crate) fn json_ordering(target: &JsonTarget, op: OrderingOp, rhs: &Scalar) -> Condition {
    // clone: the target may be read by further terms
    Condition::all().add(scalar_predicate(target.value.clone(), target.datatype.clone(), rhs, op.as_cmp_op()))
}

/// Decide a pattern Query Term (`~=`/`!~=`) against one target.
///
/// Both polarities require the element to be present (clause 4.9), which is
/// what [`present_and`] states once for every backend, and the target to be a
/// String: "If the target value data type is different than String then it
/// shall be considered as not matching". A JSON string declared as a time value
/// is a time value, not a String.
pub(crate) fn json_pattern(target: &JsonTarget, pattern: &Pattern, polarity: PatternPolarity) -> Condition {
    let text = cast_compound(jsonb_whole_text(target.value.clone()), PostgresType::Text); // clone: the target may be read by further terms
    let value = pattern_predicate(text, polarity, pattern, PatternIndexes::None);
    let string = jsonb_type_is(target.value.clone(), JsonType::String) // clone: as above
        .and(negate_total(declares_time_value(target.datatype.clone()))); // clone: as above
    Condition::all().add(present_and(target.presence.clone().and(string), value)) // clone: as above
}
