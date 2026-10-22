//! What a visit reports of `TimescaleDB`'s state in the tenant database.
//!
//! The broker converts history to the columnstore itself, on a sweep of each
//! open tenant, and creates no `TimescaleDB` job, so no tenant's history
//! depends on a background worker. Two observations check that this holds: a
//! rowstore chunk past the horizon by more than the sweep allows is overdue,
//! which shows a sweep falling behind or a tenant no broker has opened since
//! its chunk fell due; and any job numbered past `TimescaleDB`'s own was added
//! by something else, and runs only where the database has a scheduler worker.

use crate::{
    candidate::history_table::HistoryTable,
    catalog_counter::{CatalogCounter, non_negative},
    columnstore_edition::{ColumnstoreEdition, columnstore_edition},
    error::MaintenanceError,
    expected_deployment::ColumnstoreConversion,
    observation::{Finding, ObservationDecision, ObservationLevel},
    state::RegisteredTenant,
};
use andromeda_postgresql_seaquery::{
    columnstore_chunk::{OldestRangeEnd, OverdueChunks, overdue_rowstore_chunks_statement},
    statement::Statement,
    timescale_job::user_jobs_statement,
};
use chrono::{DateTime, Utc};
use sea_query::{Expr, Iden};
use sqlx::{PgConnection, Row, postgres::PgRow};

/// The rowstore chunks of one history hypertable past the overdue cutoff.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct OverdueConversion {
    /// The hypertable.
    pub(crate) table: HistoryTable,
    /// How many of its chunks are overdue.
    pub(crate) chunks: u64,
    /// The earliest range end among them, `None` when none is.
    pub(crate) oldest_range_end: Option<DateTime<Utc>>,
}

impl OverdueConversion {
    /// The finding overdue chunks report.
    pub(crate) const fn finding(self) -> Option<Finding> {
        if self.chunks > 0 {
            Some(Finding::warning(ObservationDecision::ColumnstoreOverdue))
        } else {
            None
        }
    }
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

/// Report the tenant's overdue history chunks, on an edition with a
/// columnstore, and every `TimescaleDB` job the broker did not create.
pub(crate) async fn report_timescale_state(
    connection: &mut PgConnection,
    tenant: &RegisteredTenant,
    conversion: ColumnstoreConversion,
    now: DateTime<Utc>,
) -> Result<(), MaintenanceError> {
    if let (ColumnstoreEdition::Licensed, Some(cutoff)) = (columnstore_edition(connection).await?, conversion.overdue_cutoff(now)) {
        for table in HistoryTable::ALL {
            let overdue = overdue_conversion(connection, table, cutoff).await?;
            if let Some(finding) = overdue.finding() {
                observation_event!(finding, tenant = tenant.log_name(), database = %tenant.database, relation = table.relation_name(), overdue_chunks = overdue.chunks, oldest_range_end = ?overdue.oldest_range_end, cutoff = %cutoff, "PostgreSQL history chunks past the columnstore horizon are still in the rowstore");
            }
        }
    }
    for job in user_jobs(connection).await? {
        observation_event!(UNEXPECTED_JOB, tenant = tenant.log_name(), database = %tenant.database, job_id = job.job_id, procedure = job.procedure.as_str(), hypertable = ?job.hypertable, scheduled = job.scheduled, last_run_status = ?job.last_run_status, total_failures = ?job.total_failures, "TimescaleDB job the broker did not create");
    }
    Ok(())
}

/// Read the rowstore chunks of `table` whose range ended at or before
/// `cutoff`.
async fn overdue_conversion(connection: &mut PgConnection, table: HistoryTable, cutoff: DateTime<Utc>) -> Result<OverdueConversion, MaintenanceError> {
    let row = overdue_rowstore_chunks_statement(table.relation_name(), Expr::val(cutoff))
        .to_query()
        .fetch_one(connection)
        .await?;
    Ok(OverdueConversion {
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
        timescale_observation::{OverdueConversion, UNEXPECTED_JOB},
    };
    use chrono::Utc;
    use pretty_assertions::assert_eq;

    #[test]
    fn an_overdue_rowstore_chunk_reports_columnstore_overdue_as_a_warning() {
        let overdue = |chunks| OverdueConversion {
            table: HistoryTable::AttributeHistory,
            chunks,
            oldest_range_end: (chunks > 0).then(Utc::now),
        };
        assert_eq!(overdue(0).finding(), None);
        assert_eq!(overdue(2).finding(), Some(Finding::warning(ObservationDecision::ColumnstoreOverdue)));
    }

    #[test]
    fn a_job_the_broker_did_not_create_reports_timescaledb_job_unexpected_as_a_warning() {
        assert_eq!(UNEXPECTED_JOB.decision_name(), "timescaledb-job-unexpected");
        assert_eq!(UNEXPECTED_JOB.level, ObservationLevel::Warning);
    }
}
