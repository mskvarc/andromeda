//! Whether each heap's planner statistics are fresh.
//!
//! Autoanalyze refreshes a table's statistics once the tuples modified since
//! its last analyze pass its effective analyze trigger. A heap whose modified
//! tuples reach twice its own trigger has been missed by autoanalyze, whatever
//! its settings, which happens when autovacuum falls behind. A recent analyze,
//! or one running, already refreshes it.

use crate::{
    autovacuum_trigger::{TriggerColumn, autovacuum_triggers_statement, progress_reported, statistics_column},
    branch::Branch,
    candidate::heap_target::HeapTarget,
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    observation::{Finding, ObservationDecision},
    observed_heap::WatchedHeap,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sea_query::{ExprTrait, Func, Iden, PostgresQueryBuilder, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::str::FromStr;

/// How recent an analyze keeps a heap's statistics fresh however many tuples
/// it has modified since.
const RECENT_ANALYZE: ChronoDuration = ChronoDuration::minutes(10);

/// Whether PostgreSQL reports an analyze running on a heap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AnalyzeProgress {
    /// `pg_stat_progress_analyze` shows an analyze of the heap, manual or by
    /// an autovacuum worker.
    Running,
    /// No analyze of the heap is in progress.
    Idle,
}

/// Whether a heap's planner statistics need a catch-up `ANALYZE`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StatisticsFreshness {
    /// Autoanalyze has missed the heap.
    Stale,
    /// The statistics are current, or an analyze is refreshing them.
    Fresh,
}

/// One heap's modified tuples against its analyze trigger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapStatistics {
    /// The heap.
    pub(crate) heap: WatchedHeap,
    /// `n_mod_since_analyze`.
    pub(crate) modified_since_analyze: u64,
    /// Modified tuples past which autovacuum analyzes the heap.
    pub(crate) analyze_trigger: u64,
    /// The later of `last_analyze` and `last_autoanalyze`, `None` when the
    /// heap was never analyzed.
    pub(crate) last_analyzed: Option<DateTime<Utc>>,
    /// Whether an analyze of the heap is running.
    pub(crate) analyze: AnalyzeProgress,
}

impl HeapStatistics {
    /// Stale when the modified tuples reach twice the heap's trigger, no
    /// analyze completed within [`RECENT_ANALYZE`] of `now` and none is
    /// running.
    pub(crate) fn freshness(self, now: DateTime<Utc>) -> StatisticsFreshness {
        let missed = self.modified_since_analyze >= self.analyze_trigger.saturating_mul(2);
        let recently_analyzed = self.last_analyzed.is_some_and(|at| now.signed_duration_since(at) < RECENT_ANALYZE);
        match (missed, recently_analyzed, self.analyze) {
            (true, false, AnalyzeProgress::Idle) => StatisticsFreshness::Stale,
            (false, _, _) | (true, true, _) | (true, false, AnalyzeProgress::Running) => StatisticsFreshness::Fresh,
        }
    }

    /// The finding stale statistics of an observed heap report. A target's
    /// stale statistics report none: they queue its catch-up `ANALYZE`.
    pub(crate) fn observation(self, now: DateTime<Utc>) -> Option<Finding> {
        match (self.heap, self.freshness(now)) {
            (WatchedHeap::Observed(_), StatisticsFreshness::Stale) => Some(Finding::warning(ObservationDecision::StatisticsStale)),
            (WatchedHeap::Observed(_), StatisticsFreshness::Fresh) | (WatchedHeap::Target(_), StatisticsFreshness::Stale | StatisticsFreshness::Fresh) => None,
        }
    }
}

/// Read every watched heap of `branch` against its analyze trigger.
pub(crate) async fn heap_statistics(connection: &mut PgConnection, branch: Branch) -> Result<Vec<HeapStatistics>, MaintenanceError> {
    let (sql, values) = heap_statistics_statement(branch).build_sqlx(PostgresQueryBuilder);
    let rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(connection).await?;
    rows.iter().map(heap_statistics_of).collect()
}

/// Read the tuples one heap of `branch` modified since its last analyze.
pub(crate) async fn modified_since_analyze(connection: &mut PgConnection, branch: Branch, heap: HeapTarget) -> Result<u64, MaintenanceError> {
    let statement = heap_statistics_statement(branch)
        .and_where(statistics_column("relname").eq(heap.relation_name()))
        .to_owned();
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?;
    Ok(heap_statistics_of(&row)?.modified_since_analyze)
}

/// The columns the freshness statement projects besides the triggers.
#[derive(Clone, Copy, Iden)]
enum FreshnessColumn {
    /// `n_mod_since_analyze`.
    ModifiedSinceAnalyze,
    /// The later of `last_analyze` and `last_autoanalyze`.
    LastAnalyzed,
    /// Whether an analyze of the heap is in progress.
    AnalyzeRunning,
}

/// Every watched heap of `branch` with its modified tuples, its analyze
/// trigger, its last analyze and whether an analyze of it is running in the
/// tenant's database.
fn heap_statistics_statement(branch: Branch) -> SelectStatement {
    autovacuum_triggers_statement(branch)
        .expr_as(statistics_column("n_mod_since_analyze"), FreshnessColumn::ModifiedSinceAnalyze)
        .expr_as(
            Func::greatest([statistics_column("last_analyze"), statistics_column("last_autoanalyze")]),
            FreshnessColumn::LastAnalyzed,
        )
        .expr_as(progress_reported("pg_stat_progress_analyze"), FreshnessColumn::AnalyzeRunning)
        .to_owned()
}

/// Decode one heap row.
fn heap_statistics_of(row: &PgRow) -> Result<HeapStatistics, MaintenanceError> {
    let name = row.try_get::<String, _>(TriggerColumn::Relname.to_string().as_str())?;
    let running = row.try_get::<bool, _>(FreshnessColumn::AnalyzeRunning.to_string().as_str())?;
    Ok(HeapStatistics {
        heap: WatchedHeap::from_str(&name).map_err(MaintenanceError::UnknownTarget)?,
        modified_since_analyze: non_negative(
            row.try_get(FreshnessColumn::ModifiedSinceAnalyze.to_string().as_str())?,
            CatalogCounter::ModifiedSinceAnalyze,
        )?,
        analyze_trigger: non_negative(row.try_get(TriggerColumn::AnalyzeTrigger.to_string().as_str())?, CatalogCounter::AnalyzeTrigger)?,
        last_analyzed: row.try_get(FreshnessColumn::LastAnalyzed.to_string().as_str())?,
        analyze: if running { AnalyzeProgress::Running } else { AnalyzeProgress::Idle },
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        candidate::heap_target::HeapTarget,
        observation::{Finding, ObservationDecision},
        observed_heap::{ObservedHeap, WatchedHeap},
        statistics_freshness::{AnalyzeProgress, HeapStatistics, StatisticsFreshness, heap_statistics_statement},
    };
    use chrono::{DateTime, Duration, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn heap(modified_since_analyze: u64, analyze_trigger: u64, last_analyzed: Option<DateTime<Utc>>) -> HeapStatistics {
        HeapStatistics {
            heap: WatchedHeap::Target(HeapTarget::CurrentAttributes),
            modified_since_analyze,
            analyze_trigger,
            last_analyzed,
            analyze: AnalyzeProgress::Idle,
        }
    }

    /// Threshold 500 and scale factor 0.02 over 1,000,000 tuples give a
    /// trigger of 500 + 0.02 * 1,000,000 = 20,500, so twice it is 41,000.
    #[test]
    fn statistics_are_stale_at_twice_the_effective_trigger() {
        let now = Utc::now();
        assert_eq!(heap(40_999, 20_500, None).freshness(now), StatisticsFreshness::Fresh);
        assert_eq!(heap(41_000, 20_500, None).freshness(now), StatisticsFreshness::Stale);
    }

    /// The server defaults, threshold 50 and scale factor 0.1, over 1,000,000
    /// tuples give a trigger of 50 + 0.1 * 1,000,000 = 100,050.
    #[test]
    fn default_settings_raise_the_trigger() {
        let now = Utc::now();
        assert_eq!(heap(200_099, 100_050, None).freshness(now), StatisticsFreshness::Fresh);
        assert_eq!(heap(200_100, 100_050, None).freshness(now), StatisticsFreshness::Stale);
    }

    /// Twice a trigger of 20,500 is 41,000.
    #[test]
    fn stale_statistics_of_an_observed_heap_report_statistics_stale_and_a_targets_report_nothing() {
        let now = Utc::now();
        let observed = HeapStatistics {
            heap: WatchedHeap::Observed(ObservedHeap::WriteRecords),
            ..heap(41_000, 20_500, None)
        };
        assert_eq!(observed.observation(now), Some(Finding::warning(ObservationDecision::StatisticsStale)));
        assert_eq!(
            HeapStatistics {
                modified_since_analyze: 40_999,
                ..observed
            }
            .observation(now),
            None
        );
        assert_eq!(heap(41_000, 20_500, None).freshness(now), StatisticsFreshness::Stale);
        assert_eq!(heap(41_000, 20_500, None).observation(now), None);
    }

    #[test]
    fn recent_analyze_suppresses_staleness() {
        let now = Utc::now();
        assert_eq!(
            heap(10_000_000, 20_500, Some(now - Duration::minutes(5))).freshness(now),
            StatisticsFreshness::Fresh
        );
        assert_eq!(
            heap(10_000_000, 20_500, Some(now - Duration::minutes(10))).freshness(now),
            StatisticsFreshness::Stale
        );
    }

    #[test]
    fn running_analyze_suppresses_staleness() {
        let running = HeapStatistics {
            analyze: AnalyzeProgress::Running,
            ..heap(10_000_000, 20_500, None)
        };
        assert_eq!(running.freshness(Utc::now()), StatisticsFreshness::Fresh);
    }

    #[test]
    fn freshness_reads_modified_tuples_last_analyze_and_an_analyze_of_the_tenant_database() {
        let sql = heap_statistics_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""s"."n_mod_since_analyze" AS "modified_since_analyze""#), "{sql}");
        assert!(
            sql.contains(r#"GREATEST("s"."last_analyze", "s"."last_autoanalyze") AS "last_analyzed""#),
            "{sql}"
        );
        assert!(sql.contains(r#"EXISTS(SELECT 1 FROM "pg_stat_progress_analyze""#), "{sql}");
        assert!(sql.contains(r#""pg_stat_progress_analyze"."relid" = "s"."relid""#), "{sql}");
        assert!(sql.contains(r#""pg_stat_progress_analyze"."datname" = current_database()"#), "{sql}");
        assert!(sql.contains(r#"AS "analyze_trigger""#), "{sql}");
        assert!(sql.contains("'entity_search'"), "{sql}");
    }
}
