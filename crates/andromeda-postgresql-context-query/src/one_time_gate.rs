//! The materialised CTEs a keyed statement is gated on, and the gates over
//! them.
//!
//! A keyed statement chooses between reading a term's index and walking the
//! selection by what a bounded read found, because the planner cannot price
//! the term. The bounded read is a `MATERIALIZED` CTE, so it runs once however
//! many gates read it, and each gate an uncorrelated scalar subquery on it,
//! which PostgreSQL evaluates once and turns into a one-time filter: a branch
//! whose gate fails reads nothing.

use andromeda_postgresql_schema::current::Entities;
use sea_query::{Asterisk, CommonTableExpression, Expr, ExprTrait, Func, Iden, Query, SelectStatement, SimpleExpr};

/// The CTE holding the candidates a keyed term's index yields, read up to
/// one more than the statement's bound.
#[derive(Iden, Clone, Copy)]
#[iden = "candidates"]
pub(crate) struct Candidates;

/// `(SELECT COUNT(*) FROM <cte>)`.
pub(crate) fn count_of(cte: impl Iden + 'static) -> SimpleExpr {
    Query::select().expr(Func::count(Expr::col(Asterisk))).from(cte).take().into()
}

/// `SELECT entity_key FROM <cte>`.
pub(crate) fn column_of(cte: impl Iden + 'static) -> SelectStatement {
    Query::select().column(Entities::EntityKey).from(cte).take()
}

/// `<name> AS MATERIALIZED (<query>)`: computed once, however many gates read
/// it.
pub(crate) fn materialized(query: SelectStatement, name: impl Iden + 'static) -> CommonTableExpression {
    CommonTableExpression::new().query(query).table_name(name).materialized(true).to_owned()
}

/// `<query> LIMIT <bound> + 1`: enough rows to tell whether more than `bound`
/// qualify, and every row when they do not.
pub(crate) fn bounded(mut query: SelectStatement, bound: u64) -> SelectStatement {
    query.limit(bound + 1);
    query
}

/// `(SELECT COUNT(*) FROM <cte>) <= <bound>`: the bounded read holds every row
/// that qualifies.
pub(crate) fn within(cte: impl Iden + 'static, bound: u64) -> SimpleExpr {
    count_of(cte).lte(bound)
}

#[cfg(test)]
mod tests {
    use crate::one_time_gate::{Candidates, bounded, within};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn a_bounded_read_takes_one_row_past_its_bound() {
        let read = bounded(Query::select().column(Candidates).from(Candidates).take(), 1000);
        assert_eq!(read.to_string(PostgresQueryBuilder), r#"SELECT "candidates" FROM "candidates" LIMIT 1001"#);
    }

    #[test]
    fn the_gate_admits_a_read_that_stayed_within_its_bound() {
        let gate = Query::select().expr(within(Candidates, 1000)).take();
        assert_eq!(gate.to_string(PostgresQueryBuilder), r#"SELECT (SELECT COUNT(*) FROM "candidates") <= 1000"#);
    }
}
