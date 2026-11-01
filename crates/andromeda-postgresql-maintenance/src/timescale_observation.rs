//! What a visit reports of `TimescaleDB`'s state in the tenant database.
//!
//! The broker converts history to the columnstore and ages it out itself, on
//! sweeps of each tenant it has loaded, and creates no `TimescaleDB` job, so no
//! tenant's history depends on a background worker. A dormant tenant is loaded
//! when its history falls due. Three observations check that this holds: a
//! rowstore chunk past the conversion horizon, or any chunk past the retention
//! age, by more than the sweep allows is overdue, which shows a sweep falling
//! behind or failing; and any job numbered past `TimescaleDB`'s own was added
//! by something else, and runs only where the database has a scheduler worker.

use crate::{
    candidate::history_table::HistoryTable,
    catalog_counter::{CatalogCounter, non_negative},
    columnstore_edition::{ColumnstoreEdition, columnstore_edition},
    error::MaintenanceError,
    expected_deployment::{ColumnstoreConversion, HistoryRetention},
    observation::{Finding, ObservationDecision, ObservationLevel},
    state::RegisteredTenant,
};
use andromeda_postgresql_seaquery::{
    columnstore_chunk::{OldestRangeEnd, OverdueChunks, overdue_rowstore_chunks_statement},
    statement::Statement,
    timescale::aged_chunks_statement,
    timescale_job::user_jobs_statement,
};
use chrono::{DateTime, Utc};
use sea_query::{Expr, Iden, SelectStatement};
use sqlx::{PgConnection, Row, postgres::PgRow};

/// The chunks of one history hypertable a sweep should have acted on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverdueHistory {
    /// The hypertable.
    pub(crate) table: HistoryTable,
    /// How many of its chunks are overdue.
    pub(crate) chunks: u64,
    /// The earliest range end among them, `None` when none is.
    pub(crate) oldest_range_end: Option<DateTime<Utc>>,
}

impl OverdueHistory {
    /// The finding overdue chunks report under `decision`.
    pub(crate) const fn finding(self, decision: ObservationDecision) -> Option<Finding> {
        if self.chunks > 0 { Some(Finding::warning(decision)) } else { None }
    }
}

/// What the broker's two history sweeps are configured to do.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HistorySweeps {
    /// How history is converted to the columnstore.
    pub(crate) conversion: ColumnstoreConversion,
    /// How history is aged out.
    pub(crate) retention: HistoryRetention,
}

/// One `TimescaleDB` job the broker did not create.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UnexpectedJob {
    /// Its ID.
    pub(crate) job_id: i32,
    /// The procedure it runs, as `schema.name`, for the log.
    pub(crate) procedure: String,
    /// The hypertable it works on, `None` for a job on none.
    pub(crate) hypertable: Option<String>,
    /// Whether the scheduler runs it.
    pub(crate) scheduled: bool,
    /// How its last run ended, `None` when it never ran.
    pub(crate) last_run_status: Option<String>,
    /// How many of its runs failed, `None` when it never ran.
    pub(crate) total_failures: Option<u64>,
}

/// The finding every job the broker did not create reports.
pub(crate) const UNEXPECTED_JOB: Finding = Finding::warning(ObservationDecision::TimescaledbJobUnexpected);

/// Report the tenant's history chunks overdue for conversion, on an edition
/// with a columnstore, and for retention, while it is enabled, and every
/// `TimescaleDB` job the broker did not create.
pub(crate) async fn report_timescale_state(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    expected: HistorySweeps,
    now: DateTime<Utc>,
) -> Result<(), MaintenanceError> {
    if let (ColumnstoreEdition::Licensed, Some(cutoff)) = (columnstore_edition(connection).await?, expected.conversion.overdue_cutoff(now)) {
        for table in HistoryTable::ALL {
            let overdue = overdue_history(
                connection,
                table,
                overdue_rowstore_chunks_statement(table.relation_name(), Expr::val(cutoff), &table.kept_chunks()),
            )
            .await?;
            if let Some(finding) = overdue.finding(ObservationDecision::ColumnstoreOverdue) {
                observation_event!(finding, tenant = tenant.log_name(), database = %tenant.database, relation = table.relation_name(), overdue_chunks = overdue.chunks, oldest_range_end = ?overdue.oldest_range_end, cutoff = %cutoff, "PostgreSQL history chunks past the columnstore horizon are still in the rowstore");
            }
        }
    }
    if let Some(cutoff) = expected.retention.overdue_cutoff(now) {
        for table in HistoryTable::ALL {
            let overdue = overdue_history(connection, table, aged_chunks_statement(table.relation_name(), Expr::val(cutoff))).await?;
            if let Some(finding) = overdue.finding(ObservationDecision::RetentionOverdue) {
                observation_event!(finding, tenant = tenant.log_name(), database = %tenant.database, relation = table.relation_name(), overdue_chunks = overdue.chunks, oldest_range_end = ?overdue.oldest_range_end, cutoff = %cutoff, "PostgreSQL history chunks past the retention age are still stored");
            }
        }
    }
    for job in user_jobs(connection).await? {
        observation_event!(UNEXPECTED_JOB, tenant = tenant.log_name(), database = %tenant.database, job_id = job.job_id, procedure = job.procedure.as_str(), hypertable = ?job.hypertable, scheduled = job.scheduled, last_run_status = ?job.last_run_status, total_failures = ?job.total_failures, "TimescaleDB job the broker did not create");
    }
    Ok(())
}

/// Read the overdue chunks of `table` that `statement` counts.
async fn overdue_history(connection: &mut PgConnection, table: HistoryTable, statement: SelectStatement) -> Result<OverdueHistory, MaintenanceError> {
    let row = statement.to_query().fetch_one(connection).await?;
    Ok(OverdueHistory {
        table,
        chunks: non_negative(row.try_get(OverdueChunks.unquoted())?, CatalogCounter::OverdueChunks)?,
        oldest_range_end: row.try_get(OldestRangeEnd.unquoted())?,
    })
}

/// Read every job of the database `TimescaleDB` did not create.
async fn user_jobs(connection: &mut PgConnection) -> Result<Vec<UnexpectedJob>, MaintenanceError> {
    let rows = user_jobs_statement().to_query().fetch_all(connection).await?;
    rows.iter().map(unexpected_job_of).collect()
}

/// Decode one job row.
fn unexpected_job_of(row: &PgRow) -> Result<UnexpectedJob, MaintenanceError> {
    let schema: String = row.try_get("proc_schema")?;
    let name: String = row.try_get("proc_name")?;
    Ok(UnexpectedJob {
        job_id: row.try_get("job_id")?,
        procedure: format!("{schema}.{name}"),
        hypertable: row.try_get("hypertable_name")?,
        scheduled: row.try_get("scheduled")?,
        last_run_status: row.try_get("last_run_status")?,
        total_failures: row
            .try_get::<Option<i64>, _>("total_failures")?
            .map(|failures| non_negative(failures, CatalogCounter::JobFailures))
            .transpose()?,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        candidate::history_table::HistoryTable,
        observation::{Finding, ObservationDecision, ObservationLevel},
        timescale_observation::{OverdueHistory, UNEXPECTED_JOB},
    };
    use chrono::Utc;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_overdue_chunk_reports_its_sweep_s_decision_as_a_warning() {
        let overdue = |chunks| OverdueHistory {
            table: HistoryTable::AttributeHistory,
            chunks,
            oldest_range_end: (chunks > 0).then(Utc::now),
        };
        for decision in [ObservationDecision::ColumnstoreOverdue, ObservationDecision::RetentionOverdue] {
            assert_eq!(overdue(0).finding(decision), None);
            assert_eq!(overdue(2).finding(decision), Some(Finding::warning(decision)));
        }
    }

    #[test]
    fn a_job_the_broker_did_not_create_reports_timescaledb_job_unexpected_as_a_warning() {
        assert_eq!(UNEXPECTED_JOB.decision_name(), "timescaledb-job-unexpected");
        assert_eq!(UNEXPECTED_JOB.level, ObservationLevel::Warning);
    }
}
