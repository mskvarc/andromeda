//! The `TimescaleDB` jobs a database carries.
//!
//! `TimescaleDB` runs its own jobs (the telemetry reporter, the job history
//! retention) under IDs below 1000 and numbers every other job from 1000 on,
//! whoever added it. The read here lists only the latter.

use crate::timescale::TimescaleInformation;
use sea_query::{Expr, ExprTrait, Iden, JoinType, Order, Query, SelectStatement};

/// The first job ID `TimescaleDB` assigns to a job it did not create itself.
const FIRST_USER_JOB_ID: i32 = 1000;

/// The `timescaledb_information.jobs` view.
#[derive(Iden, Clone, Copy)]
enum Jobs {
    /// View name: `jobs`
    #[iden = "jobs"]
    Table,
    /// The job's ID
    JobId,
    /// The schema of the procedure the job runs
    ProcSchema,
    /// The procedure the job runs
    ProcName,
    /// The hypertable the job works on, NULL for a job on none
    HypertableName,
    /// Whether the scheduler runs the job
    Scheduled,
}

/// The `timescaledb_information.job_stats` view.
#[derive(Iden, Clone, Copy)]
enum JobStats {
    /// View name: `job_stats`
    #[iden = "job_stats"]
    Table,
    /// The job the statistics belong to
    JobId,
    /// How the job's last run ended
    LastRunStatus,
    /// How many of the job's runs failed
    TotalFailures,
}

/// Alias `j` for the jobs view.
#[derive(Iden, Clone, Copy)]
#[iden = "j"]
struct J;

/// Alias `js` for the job statistics view.
#[derive(Iden, Clone, Copy)]
#[iden = "js"]
struct Js;

/// Every job of the database that `TimescaleDB` did not create itself, by ID:
/// its procedure, the hypertable it works on, whether it is scheduled, and how
/// its last run ended and how many runs failed, both NULL for a job that never
/// ran.
///
/// Projects `job_id`, `proc_schema`, `proc_name`, `hypertable_name`,
/// `scheduled`, `last_run_status` and `total_failures` from
/// `timescaledb_information.jobs`, left-joined to `job_stats`.
#[must_use]
pub fn user_jobs_statement() -> SelectStatement {
    Query::select()
        .column((J, Jobs::JobId))
        .column((J, Jobs::ProcSchema))
        .column((J, Jobs::ProcName))
        .column((J, Jobs::HypertableName))
        .column((J, Jobs::Scheduled))
        .column((Js, JobStats::LastRunStatus))
        .column((Js, JobStats::TotalFailures))
        .from_as((TimescaleInformation, Jobs::Table), J)
        .join_as(
            JoinType::LeftJoin,
            (TimescaleInformation, JobStats::Table),
            Js,
            Expr::col((Js, JobStats::JobId)).equals((J, Jobs::JobId)),
        )
        .and_where(Expr::col((J, Jobs::JobId)).gte(FIRST_USER_JOB_ID))
        .order_by((J, Jobs::JobId), Order::Asc)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::timescale_job::user_jobs_statement;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn only_jobs_numbered_from_one_thousand_are_listed_with_their_last_run() {
        let (sql, values) = user_jobs_statement().build(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "j"."job_id", "j"."proc_schema", "j"."proc_name", "j"."hypertable_name", "j"."scheduled", "#,
                r#""js"."last_run_status", "js"."total_failures" "#,
                r#"FROM "timescaledb_information"."jobs" AS "j" "#,
                r#"LEFT JOIN "timescaledb_information"."job_stats" AS "js" ON "js"."job_id" = "j"."job_id" "#,
                r#"WHERE "j"."job_id" >= $1 ORDER BY "j"."job_id" ASC"#
            )
        );
        assert_eq!(values.0.len(), 1, "{sql}");
    }
}
