//! The PostgreSQL `ltree` match operator for sea-query.

use sea_query::{Expr, SimpleExpr};

/// ltree: `col ~ lquery` - tests if ltree matches lquery pattern.
pub fn ltree_matches(col: SimpleExpr, lquery: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("$1 ~ $2::lquery", [col, lquery])
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn ltree_matches_uses_lquery_cast() {
        let expr = ltree_matches(Expr::col("scope_paths").into(), Expr::val("A.B.*").into());
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("::lquery"), "{sql}");
    }
}
