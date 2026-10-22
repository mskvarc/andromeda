//! The first row of an ordered read with every row tied with it, as a table
//! source: `FETCH FIRST 1 ROW WITH TIES`, which sea-query cannot spell.
//!
//! PostgreSQL answers the clause by reading the ordered input until the order
//! key changes, so an index, or a merge of index scans, that returns the rows
//! in order is read for the tied rows alone. A `rank()` window filtered to its
//! first rank answers the same rows but sorts its whole input first. sea-query
//! renders a row limit as `LIMIT n` only, so the clause is spelled here around
//! the ordered read, which is embedded as a sub-statement: every value it
//! binds stays bound and numbers with the enclosing statement.

use sea_query::{Alias, Expr, Func, IntoIden, Order, QueryStatementBuilder, SelectStatement, SimpleExpr, TableRef};

/// `((select ORDER BY order direction) FETCH FIRST 1 ROW WITH TIES) AS alias`:
/// the first row of `select` in that order and every row whose `order` equals
/// it, under `alias`.
///
/// The order is appended after any `select` already carries, so the ties are
/// judged on the whole sort key. The source is rendered as the argument of a
/// function call with an empty name, the one sea-query table source that
/// takes an arbitrary expression and an alias.
#[must_use]
pub fn first_row_with_ties(mut select: SelectStatement, order: SimpleExpr, direction: Order, alias: impl IntoIden) -> TableRef {
    select.order_by_expr(order, direction);
    let ordered = SimpleExpr::SubQuery(None, Box::new(select.into_sub_query_statement()));
    let tied = Expr::cust_with_exprs("$1 FETCH FIRST 1 ROW WITH TIES", [ordered]);
    TableRef::FunctionCall(Func::cust(Alias::new("")).arg(tied), alias.into_iden())
}

#[cfg(test)]
mod tests {
    use crate::row_ties::first_row_with_ties;
    use sea_query::{Alias, Asterisk, Expr, ExprTrait, Order, PostgresQueryBuilder, Query};

    #[test]
    fn the_ordered_read_is_limited_to_its_first_row_and_the_rows_tied_with_it() {
        let read = Query::select()
            .column(Alias::new("placed"))
            .from(Alias::new("history"))
            .and_where(Expr::col(Alias::new("stream_key")).eq(7))
            .to_owned();
        let source = first_row_with_ties(read, Expr::col(Alias::new("placed")), Order::Desc, Alias::new("tied"));
        let (sql, values) = Query::select()
            .column(Asterisk)
            .from(source)
            .and_where(Expr::col(Alias::new("placed")).lt(9))
            .to_owned()
            .build(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT * FROM ((SELECT "placed" FROM "history" WHERE "stream_key" = $1 ORDER BY "placed" DESC) FETCH FIRST 1 ROW WITH TIES) AS "tied""#,
                r#" WHERE "placed" < $2"#
            )
        );
        pretty_assertions::assert_eq!(values.0.len(), 2, "{sql}");
    }

    #[test]
    fn the_tie_order_follows_the_order_the_read_already_carries() {
        let read = Query::select()
            .column(Asterisk)
            .from(Alias::new("history"))
            .order_by(Alias::new("stream_key"), Order::Asc)
            .to_owned();
        let source = first_row_with_ties(read, Expr::col(Alias::new("placed")), Order::Desc, Alias::new("tied"));
        let sql = Query::select().column(Asterisk).from(source).to_owned().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"ORDER BY "stream_key" ASC, "placed" DESC) FETCH FIRST 1 ROW WITH TIES"#),
            "{sql}"
        );
    }
}
