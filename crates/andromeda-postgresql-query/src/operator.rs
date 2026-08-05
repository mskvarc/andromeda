//! How clause 4.9's comparison and pattern operators are spelled in
//! PostgreSQL, plus the two constructions sea-query has no native form for.

use andromeda_ngsi_ld_query::filter::predicate::{CmpOp, PatternPolarity};
use sea_query::{Expr, SimpleExpr};

/// The SQL operator a clause 4.9 comparison renders to.
///
/// Shared by every rendering of clause 4.9, so a predicate cannot compile to
/// one operator against `attribute_current`, another against the payload and a
/// third against a registration's properties. It differs from the clause 4.9
/// token only at `==`, which SQL spells `=`.
pub const fn comparison(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Equal => "=",
        CmpOp::Unequal => "!=",
        CmpOp::Greater => ">",
        CmpOp::GreaterEq => ">=",
        CmpOp::Less => "<",
        CmpOp::LessEq => "<=",
    }
}

/// The POSIX regex operator a clause 4.9 pattern predicate renders to.
pub const fn pattern_match(polarity: PatternPolarity) -> &'static str {
    match polarity {
        PatternPolarity::Match => "~",
        PatternPolarity::NotMatch => "!~",
    }
}

/// Combine a presence test with a value test, as clause 4.9 requires.
///
/// Presence is conjoined in both polarities. Clause 4.9 states that the entity
/// "shall contain the target element" for a predicate to hold, and says so for
/// `notPatternOp` explicitly: `name!~="^A.*"` selects entities whose `name` is
/// present and does not match, not entities that have no `name` at all.
///
/// Every rendering that tests a value builds its predicate here, which is what
/// stops one of them disjoining presence instead. One of them did: a
/// registration that simply lacked a property satisfied every negated `q`
/// predicate over it.
pub fn present_and(presence_sql: &str, value_sql: &str) -> String {
    format!("({presence_sql} AND {value_sql})")
}

/// Array: `value = ANY(array_col)` - tests if value is in array.
pub fn any_eq(value: SimpleExpr, array_col: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 = ANY($2)", [value, array_col])
}

/// Regex: `col ~ pattern` - tests if column matches POSIX regex.
pub fn regex_match(col: SimpleExpr, pattern: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ~ $2", [col, pattern])
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn any_eq_generates_correct_operator() {
        let expr = any_eq(Expr::val("type_iri").into(), Expr::col("types").into());
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("= ANY"), "{sql}");
    }

    #[test]
    fn a_value_test_is_always_conjoined_with_presence() {
        assert_eq!(present_and("x IS NOT NULL", "NOT (x ~ $1)"), "(x IS NOT NULL AND NOT (x ~ $1))");
    }

    #[test]
    fn equality_is_the_one_operator_sql_spells_differently_from_clause_4_9() {
        assert_eq!(comparison(CmpOp::Equal), "=");
        assert_eq!(CmpOp::Equal.ngsi_token(), "==");
        for op in [CmpOp::Unequal, CmpOp::Greater, CmpOp::GreaterEq, CmpOp::Less, CmpOp::LessEq] {
            assert_eq!(comparison(op), op.ngsi_token(), "{op:?}");
        }
    }

    #[test]
    fn a_pattern_predicate_renders_the_posix_operator_of_its_polarity() {
        assert_eq!(pattern_match(PatternPolarity::Match), "~");
        assert_eq!(pattern_match(PatternPolarity::NotMatch), "!~");
    }

    #[test]
    fn regex_match_generates_the_posix_operator() {
        let expr = regex_match(Expr::col("id").into(), Expr::val("urn:.*").into());
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(" ~ "), "{sql}");
    }
}
