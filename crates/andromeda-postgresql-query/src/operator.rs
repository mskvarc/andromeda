//! How clause 4.9's comparison and pattern operators are spelled in PostgreSQL.

use andromeda_ngsi_ld_query::filter::predicate::{CmpOp, PatternPolarity};
use andromeda_postgresql_seaquery::{operator::negate_total, pg_operator::PgBinaryOperator};
use sea_query::{BinOper, Condition, Expr, ExprTrait, SimpleExpr};

/// Build a clause 4.9 comparison between two already-lifted operands.
///
/// Shared by every rendering of clause 4.9, so a predicate cannot compare with
/// one operator against `current_attribute_values`, another against the payload
/// and a third against a registration's properties. The operator token differs
/// from the clause 4.9 spelling only at `==`, which SQL writes `=`, and `!=`,
/// which is kept over the equivalent `<>`.
#[must_use]
pub fn compare(lhs: SimpleExpr, op: CmpOp, rhs: SimpleExpr) -> SimpleExpr {
    let lhs = Expr::expr(lhs);
    match op {
        CmpOp::Equal => lhs.eq(rhs),
        CmpOp::Unequal => lhs.binary(BinOper::Custom("!="), rhs),
        CmpOp::Greater => lhs.gt(rhs),
        CmpOp::GreaterEq => lhs.gte(rhs),
        CmpOp::Less => lhs.lt(rhs),
        CmpOp::LessEq => lhs.lte(rhs),
    }
}

/// Whether a comparison does not hold, counting an unknown as a non-match.
///
/// SQL's `NOT` propagates unknown: a comparison against a column the row leaves
/// empty is NULL, and `NOT NULL` is NULL, which the enclosing `WHERE` drops.
/// Clause 4.9's `unequal` requires the opposite reading — "If the data type of
/// the target value and the data type of the Query Term value are different,
/// then they shall be considered unequal" — so a target the comparison could not
/// even be evaluated against is a match, not a silent rejection.
#[must_use]
pub fn does_not_hold(comparison: Condition) -> SimpleExpr {
    negate_total(SimpleExpr::from(comparison))
}

/// Build a clause 4.9 pattern predicate between a target and a POSIX regex.
#[must_use]
pub fn pattern(lhs: SimpleExpr, polarity: PatternPolarity, rhs: SimpleExpr) -> SimpleExpr {
    match polarity {
        PatternPolarity::Match => lhs.posix_matches(rhs),
        PatternPolarity::NotMatch => lhs.posix_not_matches(rhs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_query::filter::predicate::{OrderingOp, SetMembership};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    fn operands() -> (SimpleExpr, SimpleExpr) {
        (Expr::col(sea_query::Alias::new("stored")), Expr::val("x"))
    }

    #[test]
    fn equality_is_the_one_operator_sql_spells_differently_from_clause_4_9() {
        let (lhs, rhs) = operands();
        assert!(render(compare(lhs, SetMembership::Within.as_cmp_op(), rhs)).contains(" = "));
        assert_eq!(SetMembership::Within.ngsi_token(), "==");
    }

    #[test]
    fn inequality_keeps_the_bang_equals_spelling() {
        let (lhs, rhs) = operands();
        assert!(render(compare(lhs, SetMembership::Outside.as_cmp_op(), rhs)).contains(" != "));
    }

    #[test]
    fn each_ordering_operator_renders_its_own_token() {
        for (op, token) in [
            (OrderingOp::Greater, " > "),
            (OrderingOp::GreaterEq, " >= "),
            (OrderingOp::Less, " < "),
            (OrderingOp::LessEq, " <= "),
        ] {
            let (lhs, rhs) = operands();
            assert!(render(compare(lhs, op.as_cmp_op(), rhs)).contains(token), "{op:?}");
        }
    }

    #[test]
    fn a_pattern_predicate_renders_the_posix_operator_of_its_polarity() {
        let (lhs, rhs) = operands();
        assert!(render(pattern(lhs, PatternPolarity::Match, rhs)).contains(" ~ "));
        let (lhs, rhs) = operands();
        assert!(render(pattern(lhs, PatternPolarity::NotMatch, rhs)).contains(" !~ "));
    }
}
