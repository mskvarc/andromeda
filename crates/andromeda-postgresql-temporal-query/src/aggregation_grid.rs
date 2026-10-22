//! The period grid of an aggregation (clause 4.5.19.1): which period an
//! instant falls in, and where that period ends.
//!
//! Periods tile the time axis from an origin, the query's `timeAt` or, for a
//! retrieval without one, the earliest instant of the Entity's history. A
//! duration of days and time has an exact grid, which `TimescaleDB`'s
//! `time_bucket` computes and orders a chunk scan by. A duration with a year or
//! month part has a calendar grid: `time_bucket` keeps only the year and month
//! of its origin for such a duration and refuses one that also carries days or
//! time, so period k is computed from the origin instead, `[origin + k·P,
//! origin + (k+1)·P)` in UTC, its index estimated and then corrected. A
//! calendar period ends at the next period's start, which a month-end clamp
//! makes differ from its start plus the period.

use andromeda_ngsi_ld_query::temporal::{aggregation_period::AggregationPeriod, period_grid::PeriodGrid};
use andromeda_postgresql_seaquery::{
    calendar_period::{calendar_period_estimate, calendar_period_index, calendar_period_start},
    cast::cast,
    literal::int_literal,
    pg_type::PostgresType,
    timescale::time_bucket,
};
use sea_query::{Expr, ExprTrait, Iden, IntoIden, Query, SelectStatement, SimpleExpr};

/// The start instant of a period, projected by [`AggregationGrid::period_of`].
#[derive(Iden, Clone, Copy)]
#[iden = "bucket_start"]
pub(crate) struct BucketStart;

/// The end instant of a calendar period, projected by
/// [`AggregationGrid::period_of`].
#[derive(Iden, Clone, Copy)]
#[iden = "bucket_end"]
pub(crate) struct BucketEnd;

/// The inputs of a calendar grid, projected once for the expressions that read
/// them several times.
#[derive(Iden, Clone, Copy)]
#[iden = "calendar_grid"]
struct CalendarGrid;

/// The period projected by [`CalendarGrid`].
#[derive(Iden, Clone, Copy)]
#[iden = "period"]
struct GridPeriod;

/// The origin projected by [`CalendarGrid`].
#[derive(Iden, Clone, Copy)]
#[iden = "origin"]
struct GridOrigin;

/// The estimated period index of an instant on a calendar grid.
#[derive(Iden, Clone, Copy)]
#[iden = "period_estimate"]
struct PeriodEstimate;

/// The corrected period index of an instant on a calendar grid.
#[derive(Iden, Clone, Copy)]
#[iden = "period_index"]
struct PeriodIndex;

/// The periods of one aggregation, anchored at an origin.
#[derive(Clone, Debug)]
pub(crate) struct AggregationGrid {
    /// The duration of each period.
    period: AggregationPeriod,
    /// The instant period zero starts at.
    origin: SimpleExpr,
}

impl AggregationGrid {
    /// The periods of `period` anchored at `origin`.
    pub(crate) const fn new(period: AggregationPeriod, origin: SimpleExpr) -> AggregationGrid {
        AggregationGrid { period, origin }
    }

    /// Whether the periods tile an exact grid or a calendar one.
    pub(crate) const fn grid(&self) -> PeriodGrid {
        self.period.grid()
    }

    /// The period bound as an interval.
    pub(crate) fn width(&self) -> SimpleExpr {
        cast(Expr::val(self.period.as_iso8601()), PostgresType::Interval)
    }

    /// The period `instant` falls in, as a select projecting its
    /// [`BucketStart`] and, on a calendar grid, its [`BucketEnd`]: one
    /// computation a caller joins laterally and reads through
    /// [`AggregationGrid::start_under`] and [`AggregationGrid::end_under`].
    ///
    /// On the exact grid the end is not projected beside the start, so a
    /// statement grouping by the start alone may still project the end.
    pub(crate) fn period_of(&self, instant: SimpleExpr) -> SelectStatement {
        match self.period.grid() {
            PeriodGrid::Exact => Query::select()
                // The origin is cast so the call names the overload anchored at an
                // instant, whatever type the origin was bound with.
                // clone: the origin is read by every period the grid places.
                .expr_as(
                    time_bucket(self.width(), instant, cast(self.origin.clone(), PostgresType::Timestamptz)),
                    BucketStart,
                )
                .to_owned(),
            PeriodGrid::Calendar => self.calendar_period_of(instant),
        }
    }

    /// The calendar period `instant` falls in: the grid inputs projected once,
    /// the index estimated, then corrected, and the period running from its
    /// start to the next period's start.
    fn calendar_period_of(&self, instant: SimpleExpr) -> SelectStatement {
        let grid_period = || SimpleExpr::from(Expr::col((CalendarGrid, GridPeriod)));
        let grid_origin = || SimpleExpr::from(Expr::col((CalendarGrid, GridOrigin)));
        let mut grid = Query::select();
        // clone: the origin is read by every period the grid places.
        grid.expr_as(self.width(), GridPeriod)
            .expr_as(cast(self.origin.clone(), PostgresType::Timestamptz), GridOrigin);
        let mut estimated = Query::select();
        estimated
            .column((CalendarGrid, GridPeriod))
            .column((CalendarGrid, GridOrigin))
            // clone: the estimate and its correction both read the instant.
            .expr_as(calendar_period_estimate(grid_period(), grid_origin(), instant.clone()), PeriodEstimate)
            .from_subquery(grid, CalendarGrid);
        let mut indexed = Query::select();
        indexed
            .column((CalendarGrid, GridPeriod))
            .column((CalendarGrid, GridOrigin))
            .expr_as(
                calendar_period_index(grid_period(), grid_origin(), instant, Expr::col((CalendarGrid, PeriodEstimate))),
                PeriodIndex,
            )
            .from_subquery(estimated, CalendarGrid);
        let index = || SimpleExpr::from(Expr::col((CalendarGrid, PeriodIndex)));
        Query::select()
            .expr_as(calendar_period_start(grid_period(), grid_origin(), index()), BucketStart)
            .expr_as(calendar_period_start(grid_period(), grid_origin(), index().add(int_literal(1))), BucketEnd)
            .from_subquery(indexed, CalendarGrid)
            .to_owned()
    }

    /// The start of the period [`AggregationGrid::period_of`] projected under
    /// `alias`.
    pub(crate) fn start_under(alias: impl IntoIden) -> SimpleExpr {
        Expr::col((alias.into_iden(), BucketStart))
    }

    /// The end of the period [`AggregationGrid::period_of`] projected under
    /// `alias`: the start plus the period on the exact grid, the next period's
    /// start on the calendar grid.
    pub(crate) fn end_under(&self, alias: impl IntoIden) -> SimpleExpr {
        let alias = alias.into_iden();
        match self.period.grid() {
            PeriodGrid::Exact => Expr::col((alias, BucketStart)).add(self.width()),
            PeriodGrid::Calendar => Expr::col((alias, BucketEnd)),
        }
    }

    /// What a statement groups the periods projected under `alias` by: the
    /// start, and on the calendar grid the end it projected beside it, which
    /// the start determines.
    pub(crate) fn keys_under(&self, alias: impl IntoIden) -> Vec<SimpleExpr> {
        let alias = alias.into_iden();
        match self.period.grid() {
            // clone: the start is named by both keys.
            PeriodGrid::Exact => vec![Expr::col((alias, BucketStart))],
            PeriodGrid::Calendar => vec![Expr::col((alias.clone(), BucketStart)), Expr::col((alias, BucketEnd))],
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::aggregation_grid::AggregationGrid;
    use andromeda_ngsi_ld_query::temporal::aggregation_period::AggregationPeriod;
    use andromeda_postgresql_seaquery::{cast::cast, pg_type::PostgresType};
    use sea_query::{Alias, Expr, PostgresQueryBuilder};

    fn grid(period: &str) -> AggregationGrid {
        let period = AggregationPeriod::parse(period).expect("a valid period").expect("a non-zero period");
        AggregationGrid::new(period, cast(Expr::val("2026-03-15T00:00:00Z"), PostgresType::Timestamptz))
    }

    fn period_of(period: &str) -> String {
        grid(period).period_of(Expr::col(Alias::new("t"))).to_string(PostgresQueryBuilder)
    }

    /// An exact period is `time_bucket` at the origin, its end the start plus
    /// the period.
    #[test]
    fn an_exact_period_is_the_time_bucket_at_the_origin() {
        let sql = period_of("PT1H");
        pretty_assertions::assert_eq!(
            sql,
            r#"SELECT time_bucket('PT1H'::interval, "t", '2026-03-15T00:00:00Z'::timestamptz::timestamptz) AS "bucket_start""#
        );
        let end = sea_query::Query::select()
            .expr(grid("PT1H").end_under(Alias::new("p")))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(end.contains(r#""p"."bucket_start" + ('PT1H'::interval)"#), "{end}");
    }

    /// A calendar period is numbered from the origin in UTC and ends at the
    /// next period's start.
    #[test]
    fn a_calendar_period_is_numbered_from_the_origin_and_ends_at_the_next_start() {
        let sql = period_of("P1M");
        assert!(!sql.contains("time_bucket"), "{sql}");
        assert!(sql.contains("AT TIME ZONE 'UTC'"), "{sql}");
        assert!(sql.contains(r#"AS "bucket_start""#), "{sql}");
        assert!(sql.contains(r#"AS "bucket_end""#), "{sql}");
        assert!(sql.contains(r#""calendar_grid"."period_index" + (1)"#), "{sql}");
        let keys = grid("P1M").keys_under(Alias::new("p")).len();
        pretty_assertions::assert_eq!(keys, 2);
    }
}
