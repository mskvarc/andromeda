//! A membership test PostgreSQL runs as a hashed subplan, never as a join:
//! `(<value> IN (<subquery>)) IS TRUE`, which sea-query cannot spell.
//!
//! The planner turns an `IN (subquery)` standing alone in a `WHERE` clause into
//! a semi join and picks its join method from the row estimates. When both
//! sides look like one row (statistics that still describe a table before a
//! large write, a staged set the planner cannot count), the cheapest plan scans
//! one side and reads the other again for each of its rows, which costs their
//! product. Wrapped in a boolean test the membership is not pulled up into a
//! join: it stays a subplan, which PostgreSQL hashes once and probes per row,
//! so the test costs both sides added rather than multiplied. Every value of
//! either side must be free of NULL, which makes the wrapper change no result.

use sea_query::{Expr, SimpleExpr};

/// `(membership) IS TRUE`: `membership`, an `IN (subquery)` test, evaluated
/// as a hashed subplan.
#[must_use]
pub fn hashed_membership(membership: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("($1) IS TRUE", [membership])
}

#[cfg(test)]
mod tests {
    use crate::hashed_membership::hashed_membership;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};

    #[test]
    fn the_membership_is_wrapped_in_a_boolean_test() {
        let members = Query::select().column(Alias::new("key")).from(Alias::new("staged")).take();
        let (sql, values) = Query::select()
            .expr(hashed_membership(Expr::col(Alias::new("key")).in_subquery(members)))
            .build(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT ("key" IN (SELECT "key" FROM "staged")) IS TRUE"#);
        assert_eq!(values.0.len(), 0);
    }

    #[test]
    fn a_value_the_membership_binds_stays_bound() {
        let members = Query::select()
            .column(Alias::new("key"))
            .from(Alias::new("staged"))
            .and_where(Expr::col(Alias::new("kind")).eq(7))
            .take();
        let (sql, values) = Query::select()
            .expr(hashed_membership(Expr::col(Alias::new("key")).in_subquery(members)))
            .build(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT ("key" IN (SELECT "key" FROM "staged" WHERE "kind" = $1)) IS TRUE"#);
        assert_eq!(values.0.len(), 1);
    }
}
