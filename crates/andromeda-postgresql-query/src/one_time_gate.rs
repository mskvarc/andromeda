//! The materialised CTEs a keyed statement is gated on, and the gates over
//! them.
//!
//! A keyed statement chooses between reading a term's index and walking the
//! selection by what a bounded read found, because the planner cannot price
//! the term. The bounded read is a `MATERIALIZED` CTE, so it runs once however
//! many gates read it, and each gate an uncorrelated scalar subquery on it,
//! which PostgreSQL evaluates once, when a gate first needs it, and turns into
//! a one-time filter: a branch whose gate fails reads nothing. A statement
//! that tries several reads in turn holds one CTE for each, and a read whose
//! gates all fail before it is never run. Each branch of a gated page keeps
//! its own order and window inside the `UNION ALL` that joins them. The Entity
//! query and stage 1 of the temporal query gate their statements the same
//! way, each over its own key column, and share the bound a count reads
//! candidates to.

use andromeda_postgresql_seaquery::pg_array::{any, array_of_subquery};
use sea_query::{Alias, Asterisk, CommonTableExpression, Expr, ExprTrait, Func, Iden, Query, SelectStatement, SimpleExpr};
use std::num::NonZeroUsize;

/// The CTE holding the candidates one keyed read yields, read up to one more
/// than the statement's bound for that read.
///
/// A statement names each read by its place in the order it tries them: the
/// first `candidates`, every later one `candidates_<n>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidates(NonZeroUsize);

impl Candidates {
    /// The candidates of a statement's only read, or of the first it tries.
    pub const FIRST: Candidates = Candidates(NonZeroUsize::MIN);

    /// The candidates of the read at `position` in the order a statement
    /// tries them, the first at 0.
    #[must_use]
    pub const fn at(position: usize) -> Candidates {
        Candidates(NonZeroUsize::MIN.saturating_add(position))
    }

    /// The CTE's name.
    #[must_use]
    pub fn iden(self) -> Alias {
        match self.0.get() {
            1 => Alias::new("candidates"),
            ordinal => Alias::new(format!("candidates_{ordinal}")),
        }
    }
}

/// The CTE holding what a gated page's short walk kept: the first rows of the
/// selection in page order that every filter keeps, at most as many as the
/// page needs.
#[derive(Iden, Clone, Copy)]
#[iden = "short_walk"]
pub struct ShortWalkMatches;

/// The most candidates a count reads before it walks the selection instead.
///
/// A count needs every match, so its walk reads the whole selection, while
/// its index read costs the candidates, each rechecked or probed and looked
/// up in the selection. Among 500 000 Entities the bound's candidates count
/// in 5 to 50 ms against 12 to 690 ms walked, and stage 1 of a temporal count
/// among 1 000 000 Evolutions counts them in about 0.8 s against 1.7 to 4.5 s
/// walked. The index stays the cheaper way up to a tenth of the selection,
/// and to about seven in ten for stage 1, except for a pattern whose trigrams
/// most values share, whose index gathers every posting: 350 ms for an
/// identifier pattern against about 250 ms walked.
///
/// A count past the bound pays the read of its candidates before the walk,
/// and every count of a term most Entities hold takes that path. Among
/// 500 000 Entities 10 000 candidates cost too little to tell from the walk
/// when a value index yields them, about 20 ms as carriers of an attribute
/// name and up to 175 ms for a pattern whose trigrams most values share, and
/// a larger bound reads more before it walks. Reading every candidate in
/// stage 1 counts a Scope seven in ten Evolutions hold a twentieth slower,
/// though it counts the Scopes, patterns and geo-queries between the bound
/// and a fifth of the selection 1.3 to 5.5 times faster.
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

/// `<column> = ANY(ARRAY(<keys>))`: the rows whose key is one of the
/// candidates `keys` reads.
///
/// The planner takes the rows of `<column> IN (<keys>)` from the bounded
/// read's `LIMIT`, so under a bound of 50 000 it hashes a scan of the whole
/// selection even for a few dozen candidates: a page of 47 among 500 000
/// Entities takes 29 to 52 ms that way. An array is one value whose length it
/// does not guess, so it looks each candidate up through the key's index, and
/// the same page takes about a millisecond.
#[must_use]
pub fn narrowed_to(column: SimpleExpr, keys: SelectStatement) -> SimpleExpr {
    column.eq(any(array_of_subquery(keys)))
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
///
/// The limit is part of the statement's text, and a plan made without the
/// values takes it as the estimate of the candidates. A page of stage 1 that
/// reads 500 candidates among 1 000 000 Evolutions takes 111 ms under a limit
/// of 10 000 001 and 2.7 ms under 10 001.
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

    /// Whether `count` candidates stay within the bound, so a statement past
    /// it reads them all and never walks.
    #[must_use]
    pub const fn admits(self, count: u64) -> bool {
        match self {
            CandidateBound::AtMost(bound) => count <= bound,
            CandidateBound::Whole => true,
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
    use crate::one_time_gate::{Candidates, beyond, bounded, column_of, narrowed_to, within};
    use andromeda_postgresql_schema::current::Entities;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use pretty_assertions::assert_eq;
    use sea_query::{Expr, Iden, Query};

    #[test]
    fn a_bounded_read_takes_one_row_past_its_bound() {
        let read = bounded(Query::select().column(Candidates::FIRST.iden()).from(Candidates::FIRST.iden()).take(), 1000);
        assert_statement_snapshot!("one-time-gate-a-bounded-read-takes-one-row-past-its-bound", read);
    }

    #[test]
    fn the_gate_admits_a_read_that_stayed_within_its_bound() {
        let gate = Query::select().expr(within(Candidates::FIRST.iden(), 1000)).take();
        assert_statement_snapshot!("one-time-gate-the-gate-admits-a-read-that-stayed-within-its-bound", gate);
    }

    #[test]
    fn the_gate_past_the_bound_admits_a_read_that_stopped_short() {
        let gate = Query::select().expr(beyond(Candidates::FIRST.iden(), 1000)).take();
        assert_statement_snapshot!("one-time-gate-the-gate-past-the-bound-admits-a-read-that-stopped-short", gate);
    }

    #[test]
    fn the_candidate_column_is_the_one_the_caller_names() {
        assert_statement_snapshot!(
            "one-time-gate-the-candidate-column-is-the-one-the-caller-names",
            column_of(Candidates::FIRST.iden(), Entities::EntityKey)
        );
    }

    #[test]
    fn each_read_has_its_own_candidates() {
        assert_eq!(
            [0, 1, 2].map(|position| Candidates::at(position).iden().unquoted().to_owned()),
            ["candidates", "candidates_2", "candidates_3"]
        );
        assert_eq!(Candidates::at(0), Candidates::FIRST);
    }

    #[test]
    fn the_index_branch_narrows_to_the_candidates_as_an_array() {
        let narrowed = Query::select()
            .column(Entities::EntityKey)
            .from(Entities::Table)
            .and_where(narrowed_to(
                Expr::col(Entities::EntityKey),
                column_of(Candidates::at(1).iden(), Entities::EntityKey),
            ))
            .take();
        assert_statement_snapshot!("one-time-gate-the-index-branch-narrows-to-the-candidates-as-an-array", narrowed);
    }
}
