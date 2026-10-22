//! The materialised CTEs a keyed statement is gated on, and the gates over
//! them.
//!
//! A keyed statement chooses between reading a term's index and walking the
//! selection by what a bounded read found, because the planner cannot price
//! the term. The bounded read is a `MATERIALIZED` CTE, so it runs once however
//! many gates read it, and each gate an uncorrelated scalar subquery on it,
//! which PostgreSQL evaluates once and turns into a one-time filter: a branch
//! whose gate fails reads nothing. Each branch of a gated page keeps its own
//! order and window inside the `UNION ALL` that joins them. The Entity query
//! and stage 1 of the temporal query gate their statements the same way, each
//! over its own key column, and share the bound a count reads candidates to.

use sea_query::{Alias, Asterisk, CommonTableExpression, Expr, ExprTrait, Func, Iden, Query, SelectStatement, SimpleExpr};

/// The CTE holding the candidates a keyed term's index yields, read up to
/// one more than the statement's bound.
#[derive(Iden, Clone, Copy)]
#[iden = "candidates"]
pub struct Candidates;

/// The CTE holding what a gated page's short walk kept: the first rows of the
/// selection in page order that every filter keeps, at most as many as the
/// page needs.
#[derive(Iden, Clone, Copy)]
#[iden = "short_walk"]
pub struct ShortWalkMatches;

/// The most rechecked or probed candidates a count reads before it walks the
/// selection instead.
///
/// A count needs every match, so its bound is the one past which counting
/// the candidates, each rechecked or probed and looked up in the selection,
/// costs more than walking the selection.
pub const COUNT_CANDIDATE_BOUND: u64 = 10_000;

/// `(SELECT COUNT(*) FROM <cte>)`.
#[must_use]
pub fn count_of(cte: impl Iden + 'static) -> SimpleExpr {
    Query::select().expr(Func::count(Expr::col(Asterisk))).from(cte).take().into()
}

/// `SELECT <column> FROM <cte>`: the keys of the candidates, in the key
/// column the walk they narrow carries.
#[must_use]
pub fn column_of(cte: impl Iden + 'static, column: impl Iden + 'static) -> SelectStatement {
    Query::select().column(column).from(cte).take()
}

/// `<name> AS MATERIALIZED (<query>)`: computed once, however many gates read
/// it.
#[must_use]
pub fn materialized(query: SelectStatement, name: impl Iden + 'static) -> CommonTableExpression {
    CommonTableExpression::new().query(query).table_name(name).materialized(true).to_owned()
}

/// `SELECT * FROM (<branch>) AS <alias>`, so a branch of a `UNION ALL` keeps
/// its own order and window inside the union.
#[must_use]
pub fn wrapped(branch: SelectStatement, alias: &str) -> SelectStatement {
    Query::select().column(Asterisk).from_subquery(branch, Alias::new(alias)).take()
}

/// `<query> LIMIT <bound> + 1`: enough rows to tell whether more than `bound`
/// qualify, and every row when they do not.
#[must_use]
pub fn bounded(mut query: SelectStatement, bound: u64) -> SelectStatement {
    query.limit(bound + 1);
    query
}

/// `(SELECT COUNT(*) FROM <cte>) <= <bound>`: the bounded read holds every row
/// that qualifies.
#[must_use]
pub fn within(cte: impl Iden + 'static, bound: u64) -> SimpleExpr {
    count_of(cte).lte(bound)
}

/// `(SELECT COUNT(*) FROM <cte>) > <bound>`: the bounded read stopped short of
/// a row that qualifies.
#[must_use]
pub fn beyond(cte: impl Iden + 'static, bound: u64) -> SimpleExpr {
    count_of(cte).gt(bound)
}

/// How many candidates a keyed statement reads before it walks the selection
/// instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateBound {
    /// At most this many: one more tells the statement to walk.
    AtMost(u64),
    /// Every candidate: the statement never walks, because a walk's plan
    /// cannot be trusted against the term.
    Whole,
}

impl CandidateBound {
    /// The candidates `query` yields, read as far as the bound needs.
    #[must_use]
    pub fn read(self, query: SelectStatement) -> SelectStatement {
        match self {
            CandidateBound::AtMost(bound) => bounded(query, bound),
            CandidateBound::Whole => query,
        }
    }

    /// Whether a statement past `self` never reads more candidates than one
    /// past `other` would.
    #[must_use]
    pub const fn reads_no_more_than(self, other: CandidateBound) -> bool {
        match (self, other) {
            (CandidateBound::AtMost(mine), CandidateBound::AtMost(theirs)) => mine <= theirs,
            (CandidateBound::AtMost(_) | CandidateBound::Whole, CandidateBound::Whole) => true,
            (CandidateBound::Whole, CandidateBound::AtMost(_)) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::one_time_gate::{Candidates, beyond, bounded, column_of, within};
    use andromeda_postgresql_schema::temporal::TemporalEntities;
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

    #[test]
    fn the_gate_past_the_bound_admits_a_read_that_stopped_short() {
        let gate = Query::select().expr(beyond(Candidates, 1000)).take();
        assert_eq!(gate.to_string(PostgresQueryBuilder), r#"SELECT (SELECT COUNT(*) FROM "candidates") > 1000"#);
    }

    #[test]
    fn the_candidate_column_is_the_one_the_caller_names() {
        assert_eq!(
            column_of(Candidates, TemporalEntities::TemporalEntityKey).to_string(PostgresQueryBuilder),
            r#"SELECT "temporal_entity_key" FROM "candidates""#
        );
    }
}
