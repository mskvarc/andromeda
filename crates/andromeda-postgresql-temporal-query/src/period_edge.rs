//! The edge of one attribute stream's page of whole periods (clauses 4.5.19
//! and 6.3.10).
//!
//! A page holds at most a count of periods and at most a budget of raw
//! instances per stream, and at least one whole period whatever it holds. The
//! edge is found before any row is aggregated, in three reads that each stop
//! early:
//!
//! 1. The count edge, by a skip-scan: from the first instance of the window,
//!    each probe takes the first instance at or after the end of the period
//!    the previous probe fell in, one descent of the stream's axis index
//!    whatever the density, so empty periods cost nothing. A recursive CTE of
//!    at most count + 1 probes either runs out of instances or finds the
//!    period the page stops before.
//! 2. The budget edge: the instance one past the budget, read before the count
//!    edge with an `OFFSET`. The page stops at the start of the period it
//!    falls in, or after the first period when that period alone holds more.
//! 3. The edge is the nearer of the two, and the stream continues beyond the
//!    page exactly when there is one.
//!
//! Paging backwards runs the same reads from the most recent instance.
//!
//! The probes place every period of the page that holds an instance, one
//! computation each. On a calendar grid, where placing an instant costs
//! several interval products, the edge also lists those periods, ascending, so
//! the read that aggregates the page groups its rows by them instead of
//! placing every row again.
//!
//! The probes read the represented instances as the window and the filters
//! admit them, deletion records included: they place the edge, which is a
//! period boundary whatever the instances it counted were, and reading only
//! the index keeps them off the heap.

use crate::{
    aggregation_grid::AggregationGrid,
    stream_page::{StreamContinues, Walk},
    stream_probes::{Found, Instant, StreamProbes},
};
use andromeda_postgresql_seaquery::{
    aggregate::array_agg_ordered,
    literal::int_literal,
    open_instant::{or_end_of_time, or_start_of_time},
    scalar_subquery::scalar_subquery,
};
use sea_query::{CommonTableExpression, Expr, ExprTrait, Func, Iden, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, UnionType, WithClause};
use std::num::{NonZeroU32, NonZeroU64};

/// Alias `stream_edge`: the edge of a stream's page of periods and whether
/// the stream runs on beyond it.
#[derive(Iden, Clone, Copy)]
#[iden = "stream_edge"]
pub(crate) struct StreamEdge;

/// Column `cut`: the instant a stream's page of periods stops before
/// (forwards) or starts at (backwards), NULL when the page holds every
/// period.
#[derive(Iden, Clone, Copy)]
#[iden = "cut"]
struct Cut;

/// The recursive CTE of the skip-scan's probes.
#[derive(Iden, Clone, Copy)]
#[iden = "probes"]
struct Probes;

/// Column `probe`: the position of a probe, from one.
#[derive(Iden, Clone, Copy)]
#[iden = "probe"]
struct Probe;

/// Column `period_start`: the start of the period a probe fell in.
#[derive(Iden, Clone, Copy)]
#[iden = "period_start"]
struct PeriodStart;

/// Column `period_end`: the end of the period a probe fell in.
#[derive(Iden, Clone, Copy)]
#[iden = "period_end"]
struct PeriodEnd;

/// Alias `found_period`: the period of the instance a probe found.
#[derive(Iden, Clone, Copy)]
#[iden = "found_period"]
struct FoundPeriod;

/// Alias `edge_cut`: the edge a stream's probes found.
#[derive(Iden, Clone, Copy)]
#[iden = "edge_cut"]
struct EdgeCut;

/// Alias `edge_bound`: the edge a stream's probes found, beside the bound it
/// sets on the instants of the page.
#[derive(Iden, Clone, Copy)]
#[iden = "edge_bound"]
struct EdgeBound;

/// Alias `budget_bound`: the bound the count edge sets on the instants the
/// budget probe reads.
#[derive(Iden, Clone, Copy)]
#[iden = "budget_bound"]
struct BudgetBound;

/// Column `bound`: an edge as a bound on the instants kept, the end or the
/// start of time when there is no edge.
#[derive(Iden, Clone, Copy)]
#[iden = "bound"]
struct Bound;

/// Column `period_starts`: the starts of the periods a stream's probes found,
/// ascending, the period one past the page's count included.
#[derive(Iden, Clone, Copy)]
#[iden = "period_starts"]
struct PeriodStarts;

/// Column `period_ends`: the ends of the periods a stream's probes found, in
/// the order of their starts.
#[derive(Iden, Clone, Copy)]
#[iden = "period_ends"]
struct PeriodEnds;

/// Whether the edge of a stream's page lists the periods its probes found.
#[derive(Clone, Copy)]
pub(crate) enum ProbedPeriods {
    /// Listed beside the edge, read through [`listed_starts`] and
    /// [`listed_ends`].
    Listed,
    /// Not listed: the page's rows are placed on the grid.
    Unlisted,
}

/// Whether the window can hold more periods than a page, so the count edge
/// may fall inside it.
#[derive(Clone, Copy)]
pub(crate) enum CountEdge {
    /// The window may hold more: the probes walk the stream to find the edge.
    Probed,
    /// No grid of the period lays more periods than a page over the window,
    /// so the count edge lies beyond it and only the first probe, which the
    /// budget edge reads, is taken.
    OutOfReach,
}

/// How one stream's page of periods is walked: the direction, the periods
/// it holds at most, the instances when the page bounds them, and the
/// `lastN` a backward page keeps.
#[derive(Clone, Copy)]
pub(crate) struct PagedWalk {
    /// The direction the page walks the stream in.
    pub(crate) walk: Walk,
    /// The periods a page holds at most.
    pub(crate) periods: u32,
    /// The raw instances a page aggregates at most; `None` when only the
    /// periods are counted.
    pub(crate) instances: Option<NonZeroU64>,
    /// The last instances a backward page keeps of what lies from its edge
    /// on.
    pub(crate) last_n: Option<NonZeroU32>,
}

/// The skip-scan's probes: the first instance `walk` reaches, then, unless
/// `count` puts the count edge out of reach, from each probe's period the
/// first instance beyond it, at most `periods` + 1 probes, each beside the
/// start and end of the period it fell in.
fn probes_of_stream(probes_of: &StreamProbes<'_>, grid: &AggregationGrid, walk: Walk, periods: u32, count: CountEdge) -> CommonTableExpression {
    let found_period = |select: &mut SelectStatement| {
        select
            .expr_as(AggregationGrid::start_under(FoundPeriod), PeriodStart)
            .expr_as(grid.end_under(FoundPeriod), PeriodEnd)
            .join_lateral(JoinType::Join, grid.period_of(Expr::col((Found, Instant))), FoundPeriod, SimpleExpr::from(true));
    };
    let mut first = Query::select();
    first.expr_as(int_literal(1), Probe).from_subquery(probes_of.instant(walk, None, 0), Found);
    found_period(&mut first);

    let from = match walk {
        Walk::Forward => Expr::col((Probes, PeriodEnd)),
        Walk::Backward => Expr::col((Probes, PeriodStart)),
    };
    let mut next = Query::select();
    next.expr_as(Expr::col((Probes, Probe)).add(int_literal(1)), Probe).from(Probes).join_lateral(
        JoinType::Join,
        probes_of.next_instant(walk, &from, grid.width()),
        Found,
        SimpleExpr::from(true),
    );
    found_period(&mut next);
    next.and_where(Expr::col((Probes, Probe)).lte(int_literal(i64::from(periods))))
        .and_where(Expr::col((Found, Instant)).is_not_null());

    match count {
        CountEdge::Probed => {
            first.union(UnionType::All, next);
        }
        CountEdge::OutOfReach => {}
    }
    CommonTableExpression::new()
        .query(first)
        .columns([Probe.into_iden(), PeriodStart.into_iden(), PeriodEnd.into_iden()])
        .table_name(Probes)
        .to_owned()
}

/// A bound of probe `position` the skip-scan found, NULL when it found fewer.
fn probe_bound(position: i64, bound: impl Iden + 'static) -> SimpleExpr {
    let mut probe = Query::select();
    probe
        .expr(Expr::col((Probes, bound)))
        .from(Probes)
        .and_where(Expr::col((Probes, Probe)).eq(int_literal(position)));
    scalar_subquery(probe)
}

/// `edge` as a bound on the instants a page walking `walk` keeps: the end of
/// time forwards and its start backwards when there is no edge.
fn open_bound(walk: Walk, edge: SimpleExpr) -> SimpleExpr {
    match walk {
        Walk::Forward => or_end_of_time(edge),
        Walk::Backward => or_start_of_time(edge),
    }
}

/// The edge of the stream's page of periods, and whether the stream runs on
/// beyond it.
///
/// The count edge is the period the probe one past the period count fell in:
/// forwards the page stops before its start, backwards it starts after its
/// end. The budget edge is the period the instance one past the budget falls
/// in, read on the kept side of the count edge, or the first period when that
/// one alone holds more; the edge is the nearer of the two. A backward page
/// keeping the last `lastN` instances is complete when it already holds that
/// many. `periods` says whether the edge also lists the periods the probes
/// found, and `count` whether the count edge can fall inside the window: out
/// of reach, no probe lands one past the period count and the count edge is
/// NULL.
pub(crate) fn page_edge(probes: &StreamProbes<'_>, grid: &AggregationGrid, paged: PagedWalk, periods: ProbedPeriods, count: CountEdge) -> SelectStatement {
    let walk = paged.walk;
    let beyond_count = i64::from(paged.periods) + 1;
    let count_edge = match walk {
        Walk::Forward => probe_bound(beyond_count, PeriodStart),
        Walk::Backward => probe_bound(beyond_count, PeriodEnd),
    };
    let edge = match paged.instances {
        None => count_edge,
        Some(instances) => {
            let budget_edge: SimpleExpr = match walk {
                Walk::Forward => Func::greatest([AggregationGrid::start_under(FoundPeriod), probe_bound(1, PeriodEnd)]).into(),
                Walk::Backward => Func::least([grid.end_under(FoundPeriod), probe_bound(1, PeriodStart)]).into(),
            };
            let mut budget_bound = Query::select();
            // `OFFSET 0` keeps the bound a column the probe compares with.
            budget_bound.expr_as(open_bound(walk, count_edge.clone()), Bound).offset(0);
            let kept = probes.kept_side(walk, &Expr::col((BudgetBound, Bound)));
            let mut budget = Query::select();
            budget
                .expr(budget_edge)
                .from_subquery(budget_bound, BudgetBound)
                .join_lateral(
                    JoinType::Join,
                    probes.instant(walk, Some(&kept), instances.get()),
                    Found,
                    SimpleExpr::from(true),
                )
                .join_lateral(JoinType::Join, grid.period_of(Expr::col((Found, Instant))), FoundPeriod, SimpleExpr::from(true));
            match walk {
                Walk::Forward => Func::least([count_edge, scalar_subquery(budget)]).into(),
                Walk::Backward => Func::greatest([count_edge, scalar_subquery(budget)]).into(),
            }
        }
    };
    let mut edge_cut = Query::select();
    edge_cut.expr_as(edge, Cut).with_cte(
        WithClause::new()
            .recursive(true)
            .cte(probes_of_stream(probes, grid, walk, paged.periods, count))
            .to_owned(),
    );
    let mut edge_bound = Query::select();
    edge_bound.column((EdgeCut, Cut));
    match periods {
        ProbedPeriods::Listed => {
            edge_cut.expr_as(listed(PeriodStart), PeriodStarts).expr_as(listed(PeriodEnd), PeriodEnds);
            edge_bound.column((EdgeCut, PeriodStarts)).column((EdgeCut, PeriodEnds));
        }
        ProbedPeriods::Unlisted => {}
    }
    edge_bound
        .expr_as(open_bound(walk, Expr::col((EdgeCut, Cut))), Bound)
        .from_subquery(edge_cut, EdgeCut)
        .offset(0);

    let cut = || Expr::col((EdgeBound, Cut));
    let continues = match paged.last_n {
        None => cut().is_not_null(),
        Some(n) => {
            let kept = probes.kept_side(walk, &Expr::col((EdgeBound, Bound)));
            let filled = probes.instant(walk, Some(&kept), u64::from(n.get()) - 1);
            cut().is_not_null().and(Expr::exists(filled).not())
        }
    };
    let mut select = Query::select();
    select
        .expr_as(cut(), Cut)
        .column((EdgeBound, Bound))
        .expr_as(continues, StreamContinues)
        .from_subquery(edge_bound, EdgeBound);
    match periods {
        ProbedPeriods::Listed => {
            select.column((EdgeBound, PeriodStarts)).column((EdgeBound, PeriodEnds));
        }
        ProbedPeriods::Unlisted => {}
    }
    select
}

/// `bound` of every period the probes found, as an array in the order of the
/// periods' starts.
fn listed(bound: impl Iden + 'static) -> SimpleExpr {
    let mut periods = Query::select();
    periods
        .expr(array_agg_ordered(Expr::col((Probes, bound)), Expr::col((Probes, PeriodStart))))
        .from(Probes);
    scalar_subquery(periods)
}

/// The starts of the periods the edge under [`StreamEdge`] listed, ascending.
pub(crate) fn listed_starts() -> SimpleExpr {
    Expr::col((StreamEdge, PeriodStarts))
}

/// The ends of the periods the edge under [`StreamEdge`] listed, in the order
/// of their starts.
pub(crate) fn listed_ends() -> SimpleExpr {
    Expr::col((StreamEdge, PeriodEnds))
}

/// The bound the lateral under [`StreamEdge`] sets on the instants of the
/// stream's page: the instant the page stops before (forwards) or starts at
/// (backwards), or the end or start of time when it holds every period.
pub(crate) fn stream_bound() -> SimpleExpr {
    Expr::col((StreamEdge, Bound))
}
