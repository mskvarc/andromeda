//! Array membership and POSIX regex, neither of which sea-query constructs
//! natively.

use sea_query::{Expr, SimpleExpr};

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
    fn regex_match_generates_the_posix_operator() {
        let expr = regex_match(Expr::col("id").into(), Expr::val("urn:.*").into());
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains(" ~ "), "{sql}");
    }
}
