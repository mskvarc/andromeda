//! A condition wrapped in a boolean test, `(<condition>) IS TRUE`, which
//! sea-query cannot spell and PostgreSQL plans as an opaque expression.
//!
//! The planner rewrites neither a join nor an index access out of the wrapped
//! condition; it evaluates it on each row the rest of the statement reaches.
//! Two plans the planner would otherwise pick from row estimates go away:
//!
//! - An `IN (subquery)` standing alone in a `WHERE` clause becomes a semi join
//!   whose method follows the estimates. When both sides look like one row
//!   (statistics that still describe a table before a large write, a staged
//!   set the planner cannot count), it scans one side and reads the other
//!   again per row, which costs their product. Wrapped, the membership stays a
//!   subplan PostgreSQL hashes once and probes per row, so it costs both sides
//!   added.
//! - A value condition beside an equality on a row's key can be answered from
//!   a value index when the planner believes the value rare, and then each
//!   evaluation reads every row holding the value. Wrapped, the condition
//!   filters the rows the key reaches.
//!
//! The wrapper reads NULL as false, as a `WHERE` clause does, so it changes no
//! result there; a membership's sides must also be free of NULL.

use sea_query::{Expr, SimpleExpr};

/// `(condition) IS TRUE`: `condition` evaluated on each row it reaches, never
/// pulled up into a join or answered through an index.
#[must_use]
pub fn boolean_test(condition: SimpleExpr) -> SimpleExpr {
    Expr::cust_with_exprs("($1) IS TRUE", [condition])
}

#[cfg(test)]
mod tests {
    use crate::boolean_test::boolean_test;
    use sea_query::{Alias, Expr, ExprTrait, PostgresQueryBuilder, Query};

    #[test]
    fn a_membership_is_wrapped_in_a_boolean_test() {
        let members = Query::select().column(Alias::new("key")).from(Alias::new("staged")).take();
        let (sql, values) = Query::select()
            .expr(boolean_test(Expr::col(Alias::new("key")).in_subquery(members)))
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
            .expr(boolean_test(Expr::col(Alias::new("key")).in_subquery(members)))
            .build(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT ("key" IN (SELECT "key" FROM "staged" WHERE "kind" = $1)) IS TRUE"#);
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn a_disjunction_of_value_tests_is_wrapped_whole() {
        let value = Expr::col(Alias::new("value_string")).eq("red").or(Expr::col(Alias::new("value_uri")).eq("red"));
        let (sql, values) = Query::select()
            .expr(Expr::val(1))
            .and_where(Expr::col(Alias::new("entity_key")).eq(5))
            .and_where(boolean_test(value))
            .build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT $1 WHERE "entity_key" = $2 AND (("value_string" = $3 OR "value_uri" = $4) IS TRUE)"#
        );
        assert_eq!(values.0.len(), 4);
    }
}
