//! How clause 4.9's comparison and pattern operators are spelled in
//! PostgreSQL, plus the two constructions sea-query has no native form for.

use andromeda_ngsi_ld_query::filter::predicate::{CmpOp, PatternPolarity};
use sea_query::{Expr, SimpleExpr};

/// The SQL operator a clause 4.9 comparison renders to.
///
/// Shared by every rendering of clause 4.9, so a predicate cannot compile to
/// one operator against `current_attribute_values`, another against the payload and a
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

/// Array membership: `array_col @> ARRAY(SELECT value)`.
///
/// Containment rather than `value = ANY(array_col)`, which states the same
/// thing and is the more obvious spelling: GIN `array_ops` serves `@>`, `&&`
/// and `<@` and does not serve `= ANY`, so the equality form makes every type
/// selection a sequential scan no matter how selective the type is.
///
/// The single-element array is built by a scalar subquery rather than as
/// `ARRAY[$1]`: sea-query's custom-expression tokeniser reads a bracketed span
/// as a quoted identifier and copies it out verbatim, which would leave the
/// placeholder unsubstituted and silently aliased onto another bind. The
/// subquery form plans as an `InitPlan` whose result the index scan takes as
/// its condition, so the access path is the same one a literal array gets.
pub fn array_contains(array_col: SimpleExpr, value: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 @> ARRAY(SELECT $2::TEXT)", [array_col, value])
}

/// Regex: `col ~ pattern` - tests if column matches POSIX regex.
pub fn regex_match(col: SimpleExpr, pattern: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ~ $2", [col, pattern])
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    /// GIN `array_ops` has an operator class for containment and none for
    /// `= ANY`, so the rendered leaf must be the containment form.
    #[test]
    fn array_membership_renders_as_containment_over_a_single_element_array() {
        let expr = array_contains(Expr::col("types").into(), Expr::val("type_iri").into());
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("@> ARRAY(SELECT "), "{sql}");
        assert!(!sql.contains("= ANY"), "{sql}");
        assert!(sql.contains("'type_iri'"), "the value must survive substitution: {sql}");
    }

    #[test]
    fn a_value_test_is_always_conjoined_with_presence() {
        assert_eq!(present_and("x IS NOT NULL", "NOT (x ~ $1)"), "(x IS NOT NULL AND NOT (x ~ $1))");
    }

    #[test]
    fn equality_is_the_one_operator_sql_spells_differently_from_clause_4_9() {
        use andromeda_ngsi_ld_query::filter::predicate::{OrderingOp, SetMembership};
        // The comparison primitive spells `==` as SQL `=`; every other
        // operator's SQL spelling matches its clause 4.9 token.
        assert_eq!(comparison(SetMembership::Within.as_cmp_op()), "=");
        assert_eq!(SetMembership::Within.ngsi_token(), "==");
        assert_eq!(comparison(SetMembership::Outside.as_cmp_op()), SetMembership::Outside.ngsi_token());
        for op in [OrderingOp::Greater, OrderingOp::GreaterEq, OrderingOp::Less, OrderingOp::LessEq] {
            assert_eq!(comparison(op.as_cmp_op()), op.ngsi_token(), "{op:?}");
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
