//! Whether autovacuum keeps up: with each heap a visit watches, and across
//! the cluster's worker slots.
//!
//! The per-table reloptions the migrations declare assume autovacuum keeps
//! up. A heap whose dead tuples exceed twice its own effective trigger with no
//! vacuum running has been missed, whatever its settings. Workers are a
//! cluster-wide budget: every slot busy on several consecutive passes means
//! tables queue for a worker. Both are observations only; the catch-up vacuum
//! still qualifies on its own samples.

use crate::{
    autovacuum_trigger::{TriggerColumn, autovacuum_triggers_statement, progress_reported, statistics_column},
    branch::Branch,
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    observation::{Finding, ObservationDecision, ObservationLevel},
    observed_heap::WatchedHeap,
    state::{RegisteredTenant, autovacuum_saturation},
};
use andromeda_postgresql_seaquery::{cast::cast, introspection::current_setting, pg_type::PostgresType, setting::Setting, statement::Statement};
use andromeda_telemetry::maintenance::record_autovacuum_workers;
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, Iden, Query, SelectStatement};
use sqlx::{PgConnection, Row, postgres::PgRow};
use std::str::FromStr;
use tracing::warn;

/// Consecutive passes with every worker slot busy that report saturation.
const SATURATION_PASSES: u32 = 3;

/// Whether PostgreSQL reports a vacuum running on a heap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum VacuumProgress {
    /// `pg_stat_progress_vacuum` shows a vacuum of the heap.
    Running,
    /// No vacuum of the heap is in progress.
    Idle,
}

/// One heap's dead tuples against its autovacuum trigger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeapAutovacuum {
    /// The heap.
    pub(crate) heap: WatchedHeap,
    /// `n_dead_tup`.
    pub(crate) dead_tuples: u64,
    /// Dead tuples past which autovacuum vacuums the heap.
    pub(crate) vacuum_trigger: u64,
    /// Whether a vacuum of the heap is running.
    pub(crate) vacuum: VacuumProgress,
}

impl HeapAutovacuum {
    /// Whether autovacuum has missed the heap: its dead tuples exceed twice
    /// its trigger and no vacuum is running on it.
    pub(crate) const fn is_overdue(self) -> bool {
        matches!(self.vacuum, VacuumProgress::Idle) && self.dead_tuples > self.vacuum_trigger.saturating_mul(2)
    }

    /// The finding an overdue heap reports, a target and an observed heap
    /// alike.
    pub(crate) const fn finding(self) -> Option<Finding> {
        if self.is_overdue() {
            Some(Finding::warning(ObservationDecision::AutovacuumOverdue))
        } else {
            None
        }
    }
}

/// The cluster's autovacuum workers at one instant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AutovacuumWorkers {
    /// Autovacuum workers running.
    pub(crate) busy: u64,
    /// `autovacuum_max_workers`.
    pub(crate) max: u64,
}

impl AutovacuumWorkers {
    /// The run of full passes this observation extends: one longer when every
    /// slot is busy, zero otherwise.
    pub(crate) const fn saturated_passes_after(self, prior: u32) -> u32 {
        if self.busy >= self.max { prior.saturating_add(1) } else { 0 }
    }
}

/// Whether a run of full passes is long enough to report.
pub(crate) const fn is_saturated(saturated_passes: u32) -> bool {
    saturated_passes >= SATURATION_PASSES
}

/// Read every watched heap of `branch` against its trigger and log each
/// overdue one, returning how many were.
pub(crate) async fn report_overdue_heaps(connection: &mut PgConnection, tenant: &RegisteredTenant, branch: Branch) -> Result<usize, MaintenanceError> {
    let rows = heap_autovacuum_statement(branch).to_query().fetch_all(connection).await?;
    let heaps = rows.iter().map(heap_autovacuum_of).collect::<Result<Vec<_>, _>>()?;
    let mut overdue = 0;
    for heap in heaps {
        if let Some(finding) = heap.finding() {
            overdue += 1;
            observation_event!(finding, tenant = tenant.log_name(), database = %tenant.database, branch = ?branch, relation = heap.heap.relation_name(), dead_tuples = heap.dead_tuples, vacuum_trigger = heap.vacuum_trigger, "PostgreSQL autovacuum has not vacuumed a relation past twice its trigger");
        }
    }
    Ok(overdue)
}

/// Read the cluster's autovacuum workers, report them, and extend or reset
/// the recorded run of passes that found every slot busy.
pub(crate) async fn observe_workers(platform: &mut PgConnection) -> Result<(), MaintenanceError> {
    let row = autovacuum_workers_statement().to_query().fetch_one(&mut *platform).await?;
    let workers = AutovacuumWorkers {
        busy: non_negative(row.try_get(WorkerColumn::BusyWorkers.to_string().as_str())?, CatalogCounter::AutovacuumWorkers)?,
        max: non_negative(
            row.try_get(WorkerColumn::MaxWorkers.to_string().as_str())?,
            CatalogCounter::AutovacuumMaxWorkers,
        )?,
    };
    record_autovacuum_workers(workers.busy, workers.max);
    let prior = autovacuum_saturation::saturated_passes(platform).await?;
    let passes = workers.saturated_passes_after(prior);
    if passes != prior {
        autovacuum_saturation::record_saturated_passes(platform, passes).await?;
    }
    if is_saturated(passes) {
        warn!(
            busy_workers = workers.busy,
            max_workers = workers.max,
            saturated_passes = passes,
            decision = "autovacuum-saturated",
            "Every PostgreSQL autovacuum worker slot has been busy on consecutive maintenance passes"
        );
    }
    Ok(())
}

/// The columns the health statements project besides the trigger.
#[derive(Clone, Copy, Iden)]
enum HealthColumn {
    /// `n_dead_tup`.
    DeadTuples,
    /// Whether a vacuum of the heap is in progress.
    VacuumRunning,
}

/// The columns the worker statement projects.
#[derive(Clone, Copy, Iden)]
enum WorkerColumn {
    /// Autovacuum workers running.
    BusyWorkers,
    /// `autovacuum_max_workers`.
    MaxWorkers,
}

/// Every watched heap of `branch` with its dead tuples, its trigger and
/// whether a vacuum of it is running in the tenant's database.
fn heap_autovacuum_statement(branch: Branch) -> SelectStatement {
    autovacuum_triggers_statement(branch)
        .expr_as(statistics_column("n_dead_tup"), HealthColumn::DeadTuples)
        .expr_as(progress_reported("pg_stat_progress_vacuum"), HealthColumn::VacuumRunning)
        .to_owned()
}

/// Running autovacuum workers across the cluster beside
/// `autovacuum_max_workers`.
fn autovacuum_workers_statement() -> SelectStatement {
    Query::select()
        .expr_as(Func::count(Expr::col(Asterisk)), WorkerColumn::BusyWorkers)
        .expr_as(
            cast(current_setting(Setting::AutovacuumMaxWorkers), PostgresType::Bigint),
            WorkerColumn::MaxWorkers,
        )
        .from(Alias::new("pg_stat_activity"))
        .and_where(Expr::col(Alias::new("backend_type")).eq("autovacuum worker"))
        .to_owned()
}

/// Decode one heap row.
fn heap_autovacuum_of(row: &PgRow) -> Result<HeapAutovacuum, MaintenanceError> {
    let name = row.try_get::<String, _>(TriggerColumn::Relname.to_string().as_str())?;
    let running = row.try_get::<bool, _>(HealthColumn::VacuumRunning.to_string().as_str())?;
    Ok(HeapAutovacuum {
        heap: WatchedHeap::from_str(&name).map_err(MaintenanceError::UnknownTarget)?,
        dead_tuples: non_negative(row.try_get(HealthColumn::DeadTuples.to_string().as_str())?, CatalogCounter::DeadTuples)?,
        vacuum_trigger: non_negative(
            row.try_get(TriggerColumn::VacuumTrigger.to_string().as_str())?,
            CatalogCounter::AutovacuumTrigger,
        )?,
        vacuum: if running { VacuumProgress::Running } else { VacuumProgress::Idle },
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        autovacuum_health::{AutovacuumWorkers, HeapAutovacuum, VacuumProgress, autovacuum_workers_statement, heap_autovacuum_statement, is_saturated},
        branch::Branch,
        candidate::heap_target::HeapTarget,
        observation::{Finding, ObservationDecision},
        observed_heap::{ObservedHeap, WatchedHeap},
    };
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn heap(dead_tuples: u64, vacuum: VacuumProgress) -> HeapAutovacuum {
        HeapAutovacuum {
            heap: WatchedHeap::Target(HeapTarget::SubscriptionDeliveryState),
            dead_tuples,
            vacuum_trigger: 5_500,
            vacuum,
        }
    }

    /// Threshold 500 and scale factor 0.005 over 1,000,000 tuples give a
    /// trigger of 500 + 0.005 * 1,000,000 = 5,500, so twice it is 11,000.
    #[test]
    fn heap_is_overdue_above_twice_its_effective_trigger() {
        assert!(!heap(11_000, VacuumProgress::Idle).is_overdue());
        assert!(heap(11_001, VacuumProgress::Idle).is_overdue());
    }

    /// Twice a trigger of 150 is 300.
    #[test]
    fn an_observed_heap_past_twice_its_trigger_reports_autovacuum_overdue_as_a_warning() {
        let census = |dead_tuples| HeapAutovacuum {
            heap: WatchedHeap::Observed(ObservedHeap::AttributeCensus),
            dead_tuples,
            vacuum_trigger: 150,
            vacuum: VacuumProgress::Idle,
        };
        assert_eq!(census(300).finding(), None);
        assert_eq!(census(301).finding(), Some(Finding::warning(ObservationDecision::AutovacuumOverdue)));
        assert_eq!(
            HeapAutovacuum {
                vacuum: VacuumProgress::Running,
                ..census(301)
            }
            .finding(),
            None
        );
    }

    #[test]
    fn running_vacuum_clears_overdue() {
        assert!(!heap(50_000, VacuumProgress::Running).is_overdue());
    }

    #[test]
    fn saturation_needs_three_consecutive_full_passes() {
        let full = AutovacuumWorkers { busy: 3, max: 3 };
        let first = full.saturated_passes_after(0);
        let second = full.saturated_passes_after(first);
        assert!(!is_saturated(first));
        assert!(!is_saturated(second));
        let third = full.saturated_passes_after(second);
        assert!(is_saturated(third));
        let free = AutovacuumWorkers { busy: 2, max: 3 };
        assert_eq!(free.saturated_passes_after(second), 0);
        assert!(!is_saturated(free.saturated_passes_after(second)));
    }

    #[test]
    fn heap_health_reads_dead_tuples_and_a_vacuum_of_the_tenant_database() {
        let sql = heap_autovacuum_statement(Branch::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""s"."n_dead_tup" AS "dead_tuples""#), "{sql}");
        assert!(sql.contains(r#"EXISTS(SELECT 1 FROM "pg_stat_progress_vacuum""#), "{sql}");
        assert!(sql.contains(r#""pg_stat_progress_vacuum"."relid" = "s"."relid""#), "{sql}");
        assert!(sql.contains(r#""pg_stat_progress_vacuum"."datname" = current_database()"#), "{sql}");
        assert!(sql.contains(r#"AS "vacuum_trigger""#), "{sql}");
    }

    #[test]
    fn worker_count_reads_autovacuum_workers_against_their_maximum() {
        let sql = autovacuum_workers_statement().to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT COUNT(*) AS "busy_workers", current_setting('autovacuum_max_workers')::bigint AS "max_workers" FROM "pg_stat_activity" WHERE "backend_type" = 'autovacuum worker'"#
        );
    }
}
