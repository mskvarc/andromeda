//! The rows of one attribute stream an aggregated read aggregates (clauses
//! 4.5.19 and 6.3.10).
//!
//! A period is the element of the aggregated representation, so a page holds
//! whole periods. The edge of a stream's page ([`crate::period_edge`]) is
//! found first; the rows on its kept side are then read through the same
//! index, each part of the stream on its own, and grouped by period. Paging
//! backwards with a `lastN` above the broker's maximum keeps the last `lastN`
//! instances of what lies from the edge on. A read that is one complete
//! answer, a whole-range aggregation or a `lastN` within the maximum, reads
//! every instance or the last `lastN` without an edge.
//!
//! On a calendar grid the edge lists the periods its probes found, and the
//! page is grouped by those ([`crate::listed_period_page`]): a page of whole
//! periods reads the rows of each listed period on its own, and a page keeping
//! the last `lastN` instances reads them as on the exact grid.

use crate::{
    aggregation_grid::AggregationGrid,
    alias::Ah,
    listed_period_page::within_page_period,
    period_edge::{CountEdge, PagedWalk, ProbedPeriods, page_edge, stream_bound},
    stream_page::{Walk, latest_instances},
    stream_parts::{admitted, over_parts},
    stream_probes::StreamProbes,
};
use andromeda_ngsi_ld_query::temporal::{compiled::CompiledTemporalQuery, instance_page::InstancePage, period_grid::PeriodGrid};
use andromeda_postgresql_query::history_source::HistoryBranch;
use sea_query::{Asterisk, Condition, Expr, SelectStatement};
use std::num::NonZeroU32;

/// The reads of one stream an aggregated statement joins.
pub(crate) struct AggregatedStreamRead {
    /// The edge of the stream's page, joined laterally under
    /// [`crate::period_edge::StreamEdge`]
    /// before the rows; `None` when the read is one complete answer.
    pub(crate) edge: Option<SelectStatement>,
    /// The rows the page aggregates, read laterally under `ah`.
    pub(crate) rows: StreamRows,
}

/// The rows one stream's page aggregates, and how they reach their periods.
pub(crate) enum StreamRows {
    /// Every row, placed in its period on the grid, or in the one whole-range
    /// period when there is no grid.
    OnTheGrid(SelectStatement),
    /// Every row, placed among the periods the page's edge listed.
    AmongListedPeriods(SelectStatement),
    /// The rows of one period the page's edge listed, read for each of them.
    OfEachListedPeriod(SelectStatement),
}

/// What an aggregated read of the stream under `st` joins, for the page the
/// query compiled.
///
/// `condition` admits the represented instances and places the edge;
/// `aggregated` narrows them to the ones an aggregate folds. Without a period
/// grid the read is one whole-range period and is never paged, so a page
/// reads every instance, or the `lastN` it pages. An instance page, which an
/// aggregated read never compiles to, counts its size as periods.
pub(crate) fn aggregated_stream_read(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    condition: &Condition,
    aggregated: &Condition,
    grid: Option<&AggregationGrid>,
) -> AggregatedStreamRead {
    let paging = match compiled.instances.page {
        InstancePage::Whole => Paging::Unpaged(None),
        InstancePage::LastN { n } => Paging::Unpaged(Some(n)),
        InstancePage::Periods { budget } => Paging::Paged(PagedWalk {
            walk: Walk::Forward,
            periods: budget.periods,
            instances: Some(budget.instances),
            last_n: None,
        }),
        InstancePage::Forward { size } => Paging::Paged(PagedWalk {
            walk: Walk::Forward,
            periods: size,
            instances: None,
            last_n: None,
        }),
        InstancePage::PeriodsBackward { budget, requested } => Paging::Paged(PagedWalk {
            walk: Walk::Backward,
            periods: budget.periods,
            instances: Some(budget.instances),
            last_n: Some(requested),
        }),
        InstancePage::Backward { size, requested } => Paging::Paged(PagedWalk {
            walk: Walk::Backward,
            periods: size,
            instances: None,
            last_n: Some(requested),
        }),
    };
    let probes = StreamProbes { branch, compiled, condition };
    match (grid, paging) {
        (Some(grid), Paging::Paged(paged)) => paged_read(&probes, aggregated, grid, paged),
        (None, Paging::Paged(PagedWalk { last_n, .. }) | Paging::Unpaged(last_n)) | (Some(_), Paging::Unpaged(last_n)) => AggregatedStreamRead {
            edge: None,
            rows: StreamRows::OnTheGrid(unpaged_rows(branch, compiled, aggregated, last_n)),
        },
    }
}

/// Whether a stream is read as a page of periods or as one complete answer.
#[derive(Clone, Copy)]
enum Paging {
    /// Every instance, or the last `lastN`.
    Unpaged(Option<NonZeroU32>),
    /// A page of whole periods.
    Paged(PagedWalk),
}

/// Every aggregated instance of the stream under `st`, or the last `last_n`.
fn unpaged_rows(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, aggregated: &Condition, last_n: Option<NonZeroU32>) -> SelectStatement {
    match last_n {
        Some(n) => latest_instances(branch, compiled, aggregated, n, every_column),
        None => over_parts(branch, |part| {
            let mut rows = admitted(part, aggregated);
            every_column(&mut rows);
            rows
        }),
    }
}

/// The edge and the rows of one stream's page of periods.
///
/// The last `lastN` instances are counted across periods, so a page keeping
/// them reads them whole whatever the grid.
fn paged_read(probes: &StreamProbes<'_>, aggregated: &Condition, grid: &AggregationGrid, paged: PagedWalk) -> AggregatedStreamRead {
    let StreamProbes { branch, compiled, .. } = *probes;
    // clone: the page's rows are the aggregated instances on the kept side of the edge.
    let kept = aggregated.clone().add(probes.kept_side(paged.walk, &stream_bound()));
    let every_kept = |kept: &Condition| {
        over_parts(branch, |part| {
            let mut rows = admitted(part, kept);
            every_column(&mut rows);
            rows
        })
    };
    let count = match compiled.representation.aggr_period_duration {
        Some(period) if period.spans_at_most(compiled.window.interval, paged.periods) => CountEdge::OutOfReach,
        Some(_) | None => CountEdge::Probed,
    };
    let (periods, rows) = match (grid.grid(), paged.last_n) {
        (PeriodGrid::Exact, Some(n)) => (
            ProbedPeriods::Unlisted,
            StreamRows::OnTheGrid(latest_instances(branch, compiled, &kept, n, every_column)),
        ),
        (PeriodGrid::Exact, None) => (ProbedPeriods::Unlisted, StreamRows::OnTheGrid(every_kept(&kept))),
        (PeriodGrid::Calendar, Some(n)) => (
            ProbedPeriods::Listed,
            StreamRows::AmongListedPeriods(latest_instances(branch, compiled, &kept, n, every_column)),
        ),
        (PeriodGrid::Calendar, None) => (
            ProbedPeriods::Listed,
            StreamRows::OfEachListedPeriod(every_kept(&kept.add(within_page_period(compiled.window.timeproperty)))),
        ),
    };
    // A calendar grid lists every period its probes found, and no calendar
    // period spans a known count, so its edge is always probed.
    let count = match periods {
        ProbedPeriods::Listed => CountEdge::Probed,
        ProbedPeriods::Unlisted => count,
    };
    AggregatedStreamRead {
        edge: Some(page_edge(probes, grid, paged, periods, count)),
        rows,
    }
}

/// Project every column a part of a stream carries.
fn every_column(select: &mut SelectStatement) {
    select.expr(Expr::col((Ah, Asterisk)));
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::aggregation::build_aggregation_query;
    use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, temporal_format::TemporalFormat, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        temporal::{
            aggregation_period::AggregationPeriod,
            compiled::CompiledTemporalQuery,
            instance_page::InstancePage,
            instance_selection::InstanceSelection,
            period_budget::PeriodBudget,
            representation::Representation,
            window::{TemporalWindow, TimeInterval},
        },
    };
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use andromeda_postgresql_query::{
        dictionary::query_names::QueryNames,
        history_source::{HistoryBranch, HistoryOf},
    };
    use sea_query::{Condition, Expr, PostgresQueryBuilder};
    use std::num::{NonZeroU32, NonZeroU64};

    /// A page of 1,000 periods and 1,000,000 instances.
    fn budget() -> PeriodBudget {
        PeriodBudget {
            periods: 1000,
            instances: NonZeroU64::new(1_000_000).expect("a positive budget"),
        }
    }

    /// The `sum` of `period` over 2026 on `axis`, read as `page`.
    fn aggregated(period: &str, axis: TimeProp, page: InstancePage) -> CompiledTemporalQuery {
        CompiledTemporalQuery::builder()
            .entities(EntitySelection::builder().build())
            .window(
                TemporalWindow::builder()
                    .timeproperty(axis)
                    .interval(TimeInterval::Between {
                        from: "2026-01-01T00:00:00Z".parse().expect("a valid instant"),
                        to: "2027-01-01T00:00:00Z".parse().expect("a valid instant"),
                    })
                    .build(),
            )
            .instances(InstanceSelection::builder().page(page).build())
            .representation(
                Representation::builder()
                    .format(TemporalFormat::AggregatedValues)
                    .aggr_methods(vec![AggrMethod::Sum])
                    .aggr_period_duration(AggregationPeriod::parse(period).expect("a valid period"))
                    .sys_attrs(SysAttrsInclusion::Strip)
                    .build(),
            )
            .build()
    }

    fn sql_on(branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> String {
        build_aggregation_query(
            branch,
            &HistoryOf::Entity(Expr::val("urn:ngsi-ld:Vehicle:1")),
            compiled,
            &Condition::all(),
            &QueryNames::default(),
        )
        .to_string(PostgresQueryBuilder)
    }

    fn sql_of(compiled: &CompiledTemporalQuery) -> String {
        sql_on(&HistoryBranch::live_recorded(), compiled)
    }

    /// The edge lateral of the statement, up to the rows it bounds.
    fn edge_of(sql: &str) -> &str {
        sql.split(r#"AS "stream_edge""#).next().expect("the edge lateral")
    }

    /// The skip-scan: each probe one descent of the stream's axis index, from
    /// the end of the period the previous probe fell in, at most one past the
    /// period count. The first probe reads from the start of the window; every
    /// other one searches the next period first and beyond it only when that
    /// one is empty, so a compressed chunk decompresses only the batches
    /// overlapping the search.
    #[test]
    fn the_period_edge_is_found_by_skipping_through_the_stream_axis_index() {
        let sql = sql_of(&aggregated("PT1H", TimeProp::Observed, InstancePage::Periods { budget: budget() }));
        let edge = edge_of(&sql);
        assert!(edge.contains(r#"WITH RECURSIVE "probes" ("probe", "period_start", "period_end") AS"#), "{sql}");
        pretty_assertions::assert_eq!(edge.matches(r#"ORDER BY "ah"."observed_at" ASC LIMIT 1)"#).count(), 3, "{sql}");
        assert!(
            edge.contains(r#""ah"."observed_at" >= "probes"."period_end" AND "ah"."observed_at" < "probes"."period_end" + ('PT1H'::interval)"#),
            "{sql}"
        );
        assert!(edge.contains(r#""ah"."observed_at" >= "probes"."period_end" + ('PT1H'::interval)"#), "{sql}");
        assert!(edge.contains(r#"COALESCE((SELECT "found"."instant""#), "{sql}");
        assert!(edge.contains(r#""found"."instant" IS NOT NULL"#), "{sql}");
        assert!(edge.contains(r#"WHERE "probes"."probe" <= (1000)"#), "{sql}");
        assert!(
            edge.contains(r#"SELECT LEAST((SELECT "probes"."period_start" FROM "probes" WHERE "probes"."probe" = (1001))"#),
            "{sql}"
        );
        assert!(edge.contains(r#"time_bucket('PT1H'::interval, "found"."instant""#), "{sql}");
        assert!(edge.contains(r#""found_period"."bucket_start" + ('PT1H'::interval) AS "period_end""#), "{sql}");
    }

    /// A day holds at most 25 hourly periods, so a page of 1,000 cannot stop
    /// inside it: the edge takes the first probe alone, which the instance
    /// budget's edge reads, and walks no period after it.
    #[test]
    fn a_window_holding_fewer_periods_than_the_page_reads_the_first_probe_alone() {
        let mut compiled = aggregated("PT1H", TimeProp::Observed, InstancePage::Periods { budget: budget() });
        compiled.window.interval = TimeInterval::Between {
            from: "2026-01-01T00:00:00Z".parse().expect("a valid instant"),
            to: "2026-01-02T00:00:00Z".parse().expect("a valid instant"),
        };
        let sql = sql_of(&compiled);
        let edge = edge_of(&sql);
        assert!(
            edge.contains(r#"WITH RECURSIVE "probes" ("probe", "period_start", "period_end") AS (SELECT 1 AS "probe""#),
            "{sql}"
        );
        assert!(!edge.contains(r#""probes"."probe" + (1)"#), "{sql}");
        assert!(!edge.contains(r#""probes"."probe" <= (1000)"#), "{sql}");
        assert!(edge.contains(r#""probes"."probe" = (1001)"#), "{sql}");
    }

    /// On the calendar grid the next probe starts at the next period's start,
    /// counted from the origin, rather than at the start plus the period.
    #[test]
    fn a_calendar_period_edge_steps_from_the_origin() {
        let sql = sql_of(&aggregated("P1M", TimeProp::Observed, InstancePage::Periods { budget: budget() }));
        let edge = edge_of(&sql);
        assert!(!edge.contains("time_bucket"), "{sql}");
        assert!(edge.contains(r#""found_period"."bucket_end" AS "period_end""#), "{sql}");
        assert!(edge.contains(r#""calendar_grid"."period_index" + (1)"#), "{sql}");
        assert!(edge.contains(r#""ah"."observed_at" >= "probes"."period_end""#), "{sql}");
    }

    /// On the calendar grid the edge lists the periods its probes found, and
    /// the page reads the rows of each listed period on its own, bounded on
    /// both sides, placing none of them on the grid.
    #[test]
    fn a_calendar_page_of_whole_periods_reads_each_listed_period_on_its_own() {
        let sql = sql_of(&aggregated("P1M", TimeProp::Observed, InstancePage::Periods { budget: budget() }));
        let edge = edge_of(&sql);
        assert!(
            edge.contains(r#"(SELECT array_agg("probes"."period_start" ORDER BY "probes"."period_start") FROM "probes") AS "period_starts""#),
            "{sql}"
        );
        assert!(
            edge.contains(r#"(SELECT array_agg("probes"."period_end" ORDER BY "probes"."period_start") FROM "probes") AS "period_ends""#),
            "{sql}"
        );
        assert!(edge.contains(r#""edge_bound"."period_starts", "edge_bound"."period_ends""#), "{sql}");
        let rows = sql.split(r#"AS "stream_edge""#).nth(1).expect("the rows after the edge");
        assert!(rows.contains(r#"UNNEST("stream_edge"."period_starts") AS "bucket_start""#), "{sql}");
        assert!(
            rows.contains(concat!(
                r#""ah"."observed_at" < "stream_edge"."bound" "#,
                r#"AND ("ah"."observed_at" >= "page_period"."bucket_start" "#,
                r#"AND "ah"."observed_at" < "page_period"."bucket_end")"#
            )),
            "{sql}"
        );
        assert!(!rows.contains("calendar_grid"), "{sql}");
        assert!(!rows.contains("GROUP BY"), "{sql}");
    }

    /// A calendar page keeping its last `lastN` instances counts them across
    /// periods, so it reads them whole and places each among the periods the
    /// edge listed rather than on the grid.
    #[test]
    fn a_calendar_page_keeping_its_last_n_places_its_instances_among_the_listed_periods() {
        let page = InstancePage::PeriodsBackward {
            budget: budget(),
            requested: NonZeroU32::new(5000).expect("a positive count"),
        };
        let sql = sql_of(&aggregated("P1M", TimeProp::Observed, page));
        assert!(edge_of(&sql).contains(r#"AS "period_starts""#), "{sql}");
        let rows = sql.split(r#"AS "stream_edge""#).nth(1).expect("the rows after the edge");
        assert!(rows.contains("LIMIT 5000"), "{sql}");
        assert!(
            rows.contains(r#"width_bucket("ah"."observed_at", "stream_edge"."period_starts") AS "period_position""#),
            "{sql}"
        );
        assert!(!rows.contains("calendar_grid"), "{sql}");
    }

    /// On the exact grid `time_bucket` places the rows in the order the axis
    /// index returns them, so the edge lists no period.
    #[test]
    fn an_exact_page_lists_no_period() {
        let sql = sql_of(&aggregated("PT1H", TimeProp::Observed, InstancePage::Periods { budget: budget() }));
        assert!(!sql.contains("period_starts"), "{sql}");
        assert!(!sql.contains("page_period"), "{sql}");
    }

    /// The budget probe reads the instance one past the budget before the count
    /// edge, every part limited to that many, and the page stops at the start
    /// of the period it falls in.
    #[test]
    fn the_instance_budget_cuts_at_the_start_of_the_period_it_falls_in() {
        let sql = sql_of(&aggregated("P1D", TimeProp::Observed, InstancePage::Periods { budget: budget() }));
        let edge = edge_of(&sql);
        assert!(edge.contains(r#"ORDER BY "ah"."observed_at" ASC LIMIT 1000001"#), "{sql}");
        assert!(edge.contains(r#"ORDER BY "found"."instant" ASC LIMIT 1 OFFSET 1000000"#), "{sql}");
        assert!(
            edge.contains(r#"SELECT COALESCE((SELECT "probes"."period_start" FROM "probes" WHERE "probes"."probe" = (1001)), 'infinity'::timestamptz) AS "bound" OFFSET 0"#),
            "{sql}"
        );
        assert!(edge.contains(r#""ah"."observed_at" < "budget_bound"."bound""#), "{sql}");
        assert!(edge.contains(r#"GREATEST("found_period"."bucket_start","#), "{sql}");
    }

    /// A period is never split: when the first period alone holds more than the
    /// budget, the page ends after it rather than before it.
    #[test]
    fn a_first_period_over_the_budget_is_read_whole() {
        let sql = sql_of(&aggregated("P1D", TimeProp::Observed, InstancePage::Periods { budget: budget() }));
        assert!(
            edge_of(&sql).contains(r#"GREATEST("found_period"."bucket_start", (SELECT "probes"."period_end" FROM "probes" WHERE "probes"."probe" = (1)))"#),
            "{sql}"
        );
    }

    /// Every part of a live stream (its recorded rows and its queued
    /// entries) is read ordered and limited on its own before the parts are
    /// joined, so each limit reaches its index; on `createdAt` the order is
    /// the column the stream-led index keys.
    #[test]
    fn each_part_of_a_live_stream_is_ordered_and_limited_on_its_own() {
        let page = InstancePage::Periods { budget: budget() };
        let live = sql_on(&HistoryBranch::live(), &aggregated("PT1H", TimeProp::Created, page));
        let ordered = r#"ORDER BY "ah"."created_at" ASC"#;
        pretty_assertions::assert_eq!(edge_of(&live).matches(&format!("{ordered} LIMIT 1000001")).count(), 2, "{live}");
        pretty_assertions::assert_eq!(edge_of(&live).matches(&format!("{ordered} LIMIT 1)")).count(), 6, "{live}");
        let recorded = sql_of(&aggregated("PT1H", TimeProp::Observed, page));
        pretty_assertions::assert_eq!(
            edge_of(&recorded).matches(r#"ORDER BY "ah"."observed_at" ASC LIMIT 1000001"#).count(),
            1,
            "{recorded}"
        );
    }

    /// The rows the page aggregates are those before its edge, grouped by
    /// `time_bucket` so the stream's axis index returns them in period order.
    #[test]
    fn the_page_groups_the_rows_before_its_cut_by_time_bucket() {
        let sql = sql_of(&aggregated("PT1H", TimeProp::Observed, InstancePage::Periods { budget: budget() }));
        let rows = sql.split(r#"AS "stream_edge""#).nth(1).expect("the rows after the edge");
        assert!(rows.contains(r#""ah"."observed_at" < "stream_edge"."bound""#), "{sql}");
        assert!(rows.contains(r#"time_bucket('PT1H'::interval, "ah"."observed_at""#), "{sql}");
        assert!(rows.contains(r#"GROUP BY "bucket"."bucket_start""#), "{sql}");
        assert!(sql.contains(r#""stream_edge"."stream_continues" AS "stream_continues""#), "{sql}");
        assert!(
            edge_of(&sql).contains(r#"COALESCE("edge_cut"."cut", 'infinity'::timestamptz) AS "bound""#),
            "{sql}"
        );
        assert!(edge_of(&sql).contains(r#""edge_bound"."cut" IS NOT NULL AS "stream_continues""#), "{sql}");
    }

    /// Paging backwards with a `lastN` above the maximum walks from the most
    /// recent instance, keeps the last `lastN` instances from the edge on, and
    /// is complete once it holds that many.
    #[test]
    fn a_backward_page_keeps_the_last_n_instances_from_its_edge() {
        let page = InstancePage::PeriodsBackward {
            budget: budget(),
            requested: NonZeroU32::new(5000).expect("a positive count"),
        };
        let sql = sql_of(&aggregated("PT1H", TimeProp::Observed, page));
        let edge = edge_of(&sql);
        assert!(edge.contains(r#"ORDER BY "ah"."observed_at" DESC LIMIT 1)"#), "{sql}");
        assert!(edge.contains(r#""ah"."observed_at" < "probes"."period_start""#), "{sql}");
        assert!(
            edge.contains(r#"SELECT GREATEST((SELECT "probes"."period_end" FROM "probes" WHERE "probes"."probe" = (1001))"#),
            "{sql}"
        );
        assert!(edge.contains(r#"ORDER BY "found"."instant" DESC LIMIT 1 OFFSET 4999"#), "{sql}");
        assert!(edge.contains(r#""edge_bound"."cut" IS NOT NULL AND (NOT EXISTS("#), "{sql}");
        assert!(edge.contains(r#""ah"."observed_at" >= "edge_bound"."bound""#), "{sql}");
        let rows = sql.split(r#"AS "stream_edge""#).nth(1).expect("the rows after the edge");
        assert!(rows.contains(r#""ah"."observed_at" >= "stream_edge"."bound""#), "{sql}");
        assert!(rows.contains("LIMIT 5000"), "{sql}");
    }

    /// A `lastN` within the maximum and a whole-range aggregation are complete
    /// answers, read without an edge.
    #[test]
    fn a_complete_answer_is_read_without_an_edge() {
        let last_n = InstancePage::LastN {
            n: NonZeroU32::new(5).expect("a positive count"),
        };
        let sql = sql_of(&aggregated("PT1H", TimeProp::Observed, last_n));
        assert!(!sql.contains("stream_edge"), "{sql}");
        assert!(sql.contains(r#"ORDER BY "ah"."observed_at" DESC, "ah"."recorded_at" DESC"#), "{sql}");
        assert!(sql.contains("LIMIT 5"), "{sql}");
        let whole = sql_of(&aggregated("", TimeProp::Observed, InstancePage::Whole));
        assert!(!whole.contains("stream_edge"), "{whole}");
        assert!(!whole.contains("probes"), "{whole}");
    }
}
