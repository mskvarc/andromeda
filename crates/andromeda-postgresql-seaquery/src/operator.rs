//! The generic SQL operators sea-query has no native form for: array
//! containment, POSIX regex, and the presence-and-value conjunction.

use sea_query::{Expr, SimpleExpr};

/// Combine a presence test with a value test.
///
/// Presence is conjoined in both polarities, so a value test cannot silently
/// hold for a row that lacks the value entirely. Every rendering that tests a
/// value builds its predicate here, which is what stops one of them disjoining
/// presence instead.
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
    fn regex_match_generates_the_posix_operator() {
        let expr = regex_match(Expr::col("id").into(), Expr::val("urn:.*").into());
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(" ~ "), "{sql}");
    }
}
