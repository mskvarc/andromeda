//! The PostgreSQL `ltree` match operator for sea-query.

use crate::{cast::cast, pg_operator::PgBinaryOperator, pg_type::PostgresType};
use sea_query::SimpleExpr;

/// ltree: `col ~ lquery` - tests if ltree matches lquery pattern.
///
/// The pattern is cast to `lquery` so the `~` operator resolves to the `ltree`
/// match rather than to POSIX regex, which shares the same token.
pub fn ltree_matches(col: SimpleExpr, lquery: SimpleExpr) -> SimpleExpr {
    col.posix_matches(cast(lquery, PostgresType::Lquery))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_query::{Expr, PostgresQueryBuilder, Query};

    #[test]
    fn ltree_matches_uses_lquery_cast() {
        let expr = ltree_matches(Expr::col("scope_paths"), Expr::val("A.B.*"));
        let sql = Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder);
        assert!(sql.contains("::lquery"), "{sql}");
    }
}
