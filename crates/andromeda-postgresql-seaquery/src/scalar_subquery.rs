//! A `SELECT` of one row and one column used as a value.

use sea_query::{QueryStatementBuilder, SelectStatement, SimpleExpr};

/// `(select)` as an expression: the value of its one column in its one row,
/// NULL when it returns no row.
///
/// The select is embedded as a sub-statement, so the values it binds number
/// with the enclosing statement's.
#[must_use]
pub fn scalar_subquery(select: SelectStatement) -> SimpleExpr {
    SimpleExpr::SubQuery(None, Box::new(select.into_sub_query_statement()))
}

#[cfg(test)]
mod tests {
    use crate::scalar_subquery::scalar_subquery;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};

    #[test]
    fn a_scalar_subquery_is_parenthesised_and_binds_with_its_statement() {
        let inner = Query::select()
            .column(Alias::new("cut"))
            .from(Alias::new("edges"))
            .and_where(Expr::col(Alias::new("probe")).eq(7))
            .to_owned();
        let (sql, values) = Query::select().expr(scalar_subquery(inner)).to_owned().build(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT (SELECT "cut" FROM "edges" WHERE "probe" = $1)"#);
        assert_eq!(values.0.len(), 1, "{sql}");
    }
}
