//! A clause 4.9 Query Term decided against one JSON value.
//!
//! Two kinds of target value are only reachable as JSON: a member of a
//! compound value (`address[city]`), read out of a `value_object`, and every
//! target a temporal `q` reaches through a history instance rebuilt as an
//! expanded document. Both read one JSONB value per candidate and lift it to
//! the comparison domain the operand names, so the comparisons are written
//! once here and each reading only says where its values come from.

use crate::{
    jsonb::lift_comparison,
    operator::{compare, pattern as pattern_predicate},
    q_predicate::string_ordering::ordered_string,
};
use andromeda_ngsi_ld_query::{
    filter::{
        predicate::{CmpOp, OrderingOp, PatternPolarity, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use andromeda_postgresql_seaquery::{cast::cast_compound, literal::text_literal, operator::present_and, pg_operator::PgBinaryOperator, pg_type::PostgresType};
use sea_query::{Condition, Expr as SeaExpr, Func, SimpleExpr};

/// One JSON target value: a Property's value, and a Relationship's object
/// where the reading can hold one.
#[derive(Debug, Clone)]
pub(crate) struct JsonTarget {
    /// The value, as JSONB; SQL `NULL` where there is none.
    pub(crate) value: SimpleExpr,
    /// The Relationship object, as JSONB, where the reading can hold one.
    pub(crate) object: Option<SimpleExpr>,
    /// Whether the value is present at all.
    pub(crate) presence: SimpleExpr,
}

/// Read a JSONB value out as its whole text: `(jsonb) #>> '{}'`.
///
/// The empty `text[]` path yields the value itself as text, which is what every
/// `q` comparison lifts from.
fn whole_text(jsonb: SimpleExpr) -> SimpleExpr {
    jsonb.json_path_text(text_literal("{}"))
}

/// Compare a stored text expression against one scalar operand.
///
/// Everything that decides the comparison domain (the cast a number or boolean
/// needs, the guarded `timestamptz` comparison a `DateTime` needs, the
/// normalisation a `Time` needs) lives in [`lift_comparison`], so this reading
/// and the CSR renderer compare the same way. A String ordering compares under
/// the collation [`ordered_string`] picks.
fn scalar_predicate(stored_text: SimpleExpr, scalar: &Scalar, op: CmpOp) -> SimpleExpr {
    let lifted = lift_comparison(scalar, stored_text);
    compare(ordered_string(lifted.stored, scalar, op), op, lifted.operand)
}

/// The text an equality compares: the value, or the Relationship object where
/// the reading holds one.
///
/// Clause 4.9 admits a Relationship's object as a comparison target for the
/// equality operators and requires a Relationship operand to non-match every
/// other operator, which is why the object is read here and nowhere else.
fn equality_text(target: &JsonTarget) -> SimpleExpr {
    // clone: sea-query owns every operand, and one target is compared by several arms
    match &target.object {
        Some(object) => Func::coalesce([whole_text(target.value.clone()), whole_text(object.clone())]).into(),
        None => whole_text(target.value.clone()),
    }
}

/// Decide an equality Query Term (`==`/`!=`) against one target.
pub(crate) fn json_equality(target: &JsonTarget, membership: SetMembership, rhs: &CompareValue) -> Condition {
    match rhs {
        CompareValue::Scalar(scalar) => Condition::all().add(scalar_predicate(equality_text(target), scalar, membership.as_cmp_op())),
        CompareValue::Range { min, max } => {
            let stored = whole_text(target.value.clone()); // clone: the target is read by every arm
            // Both bounds compare against the one lifted stored side; lifting the
            // maximum only contributes its operand.
            let lower = lift_comparison(min, stored.clone()); // clone: the stored side is compared against both bounds
            let upper_operand = lift_comparison(max, stored).operand;
            let stored_side = lower.stored;
            // Each bound orders the stored side under its own collation.
            let bounded = |op: CmpOp, bound: &Scalar, operand: SimpleExpr| compare(ordered_string(stored_side.clone(), bound, op), op, operand); // clone: the stored side is read by both halves
            // A missing element makes the extraction NULL, and every comparison
            // against NULL is unknown, so presence is required structurally in
            // both polarities (clause 4.9).
            match membership {
                SetMembership::Within => Condition::all()
                    .add(bounded(CmpOp::GreaterEq, min, lower.operand))
                    .add(bounded(CmpOp::LessEq, max, upper_operand)),
                SetMembership::Outside => Condition::any()
                    .add(bounded(CmpOp::Less, min, lower.operand))
                    .add(bounded(CmpOp::Greater, max, upper_operand)),
            }
        }
        CompareValue::ValueList(values) => {
            // A value list is a set of equality comparisons (clause 4.9 Table
            // 4.9-1), so each arm reads the Relationship object exactly as the
            // single-scalar comparison does.
            let disjunction = values.iter().fold(Condition::any(), |cond, value| {
                let lifted = lift_comparison(value, equality_text(target));
                cond.add(compare(lifted.stored, CmpOp::Equal, lifted.operand))
            });
            match membership {
                SetMembership::Within => Condition::all().add(disjunction),
                SetMembership::Outside => Condition::all().add(disjunction.not()),
            }
        }
    }
}

/// Decide an ordering Query Term (`>`/`>=`/`<`/`<=`) against one target.
///
/// A Relationship's object is not read at all. Clause 4.9 requires a
/// Relationship (and a `ListRelationship`) operand to non-match every operator
/// other than equality and inequality: a URI names a resource and carries no
/// order a `<` or `>` could mean.
pub(crate) fn json_ordering(target: &JsonTarget, op: OrderingOp, rhs: &Scalar) -> Condition {
    Condition::all().add(scalar_predicate(whole_text(target.value.clone()), rhs, op.as_cmp_op())) // clone: the target may be read by further terms
}

/// Decide a pattern Query Term (`~=`/`!~=`) against one target.
///
/// Both polarities require the element to be present (clause 4.9), which is
/// what [`present_and`] states once for every backend. Only a Property's value
/// is a target: clause 4.9 defines the pattern operators over String targets
/// and requires a Relationship operand to non-match every operator other than
/// equality and inequality.
pub(crate) fn json_pattern(target: &JsonTarget, pattern: &Pattern, polarity: PatternPolarity) -> Condition {
    let text = cast_compound(whole_text(target.value.clone()), PostgresType::Text); // clone: the target may be read by further terms
    let value = pattern_predicate(text, polarity, SeaExpr::val(pattern.portable().as_str()));
    Condition::all().add(present_and(target.presence.clone(), value)) // clone: as above
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{instance_row::InstanceRow, json_value_term::json_equality, value_object_member::on_some_member};
    use andromeda_ngsi_ld_query::filter::{
        expression::Expr,
        parser::parse,
        path::QueryPath,
        predicate::{Predicate, SetMembership},
    };
    use sea_query::{PostgresQueryBuilder, Query};

    /// Clause 4.9 evaluates `address[city]` as a `MemberExpression` of the
    /// target value, which a history row holds in its own `value_object`.
    #[test]
    fn a_temporal_q_on_a_compound_value_reads_value_object() {
        let Expr::Pred(pred) = parse("\"https://example.com/address\"[city]==\"Bern\"").expect("parse q") else {
            panic!("a single predicate");
        };
        let (QueryPath::Local(path), Predicate::Equality { rhs, .. }) = (pred.path(), &pred) else {
            panic!("a local equality");
        };
        let condition = on_some_member(path, InstanceRow::History, |member| json_equality(member, SetMembership::Within, rhs));
        let sql = Query::select().cond_where(condition).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"jsonb_path_query("qh"."value_object", '$."city"[*]."@value"')"#), "{sql}");
        assert!(sql.contains("'Bern'"), "{sql}");
        assert!(!sql.contains("jsonb_build_object"), "{sql}");
    }
}
