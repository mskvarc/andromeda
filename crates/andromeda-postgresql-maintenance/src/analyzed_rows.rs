//! The row count a heap or a density-measured GIN is judged by.
//!
//! A heap's dead share is its dead tuples over its dead and live tuples, and a
//! GIN's density is its bytes over the rows it covers. Both row counts are
//! written by `ANALYZE` and by `VACUUM`. An analyze estimates them from a
//! random sample of the whole table, so its figure holds. A vacuum
//! extrapolates them from the pages it scanned, which under writes are the
//! pages the writes touched, and its figure drifts by a fifth and more between
//! two analyzes. A judgement on the vacuum's figure crosses its threshold in
//! both directions within minutes, and no breach run lasts.
//!
//! The count is therefore kept on the target's row with the instant of the
//! analyze it comes from, and a reading replaces it only when the table's
//! latest analyze is newer than that instant and than the table's latest
//! vacuum. Between two analyzes it lags the table by at most the rows the
//! table's analyze trigger lets change.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, Func, Iden, SelectStatement, SimpleExpr};
use sqlx::{Row, postgres::PgRow};

/// A row count as an analyze left it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AnalyzedRows {
    /// The rows.
    pub(crate) rows: u64,
    /// The instant of the analyze they come from.
    pub(crate) analyzed_at: DateTime<Utc>,
}

/// A row count read from the catalogs now, with the table's latest analyze
/// and vacuum, which tell whether a vacuum wrote it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RowCountReading {
    /// The rows the catalogs hold now.
    pub(crate) rows: u64,
    /// The later of `last_analyze` and `last_autoanalyze`, `None` before the
    /// table's first analyze or after its statistics were reset.
    pub(crate) last_analyzed: Option<DateTime<Utc>>,
    /// The later of `last_vacuum` and `last_autovacuum`, `None` before the
    /// table's first vacuum or after its statistics were reset.
    pub(crate) last_vacuumed: Option<DateTime<Utc>>,
}

impl RowCountReading {
    /// The rows to store in place of `stored`: this reading's, when the
    /// table's latest analyze is newer than `stored` and than its latest
    /// vacuum. `None` keeps `stored`.
    pub(crate) fn replacement(self, stored: Option<AnalyzedRows>) -> Option<AnalyzedRows> {
        let analyzed_at = self.last_analyzed?;
        let newer_than_stored = stored.is_none_or(|stored| analyzed_at > stored.analyzed_at);
        let newer_than_vacuum = self.last_vacuumed.is_none_or(|vacuumed| analyzed_at > vacuumed);
        (newer_than_stored && newer_than_vacuum).then_some(AnalyzedRows { rows: self.rows, analyzed_at })
    }

    /// Decode a reading of `rows` rows from a row projected with
    /// [`project_maintenance_instants`].
    pub(crate) fn of(rows: i64, counter: CatalogCounter, row: &PgRow) -> Result<RowCountReading, MaintenanceError> {
        Ok(RowCountReading {
            rows: non_negative(rows, counter)?,
            last_analyzed: row.try_get(MaintenanceInstant::LastAnalyzed.to_string().as_str())?,
            last_vacuumed: row.try_get(MaintenanceInstant::LastVacuumed.to_string().as_str())?,
        })
    }
}

/// The instants a row count reading is judged by.
#[derive(Clone, Copy, Iden)]
enum MaintenanceInstant {
    /// The later of `last_analyze` and `last_autoanalyze`.
    LastAnalyzed,
    /// The later of `last_vacuum` and `last_autovacuum`.
    LastVacuumed,
}

/// The later of the manual and the automatic instant of one operation, read
/// from the table statistics view aliased `alias`. `greatest` ignores a NULL,
/// so it is NULL only while both are.
fn latest(alias: &'static str, manual: &'static str, automatic: &'static str) -> SimpleExpr {
    Func::greatest([
        Expr::col((Alias::new(alias), Alias::new(manual))),
        Expr::col((Alias::new(alias), Alias::new(automatic))),
    ])
    .into()
}

/// The table's latest analyze, from the statistics view aliased `alias`.
pub(crate) fn latest_analyze(alias: &'static str) -> SimpleExpr {
    latest(alias, "last_analyze", "last_autoanalyze")
}

/// Project the table's latest analyze and latest vacuum, from the statistics
/// view aliased `alias`, as [`RowCountReading::of`] decodes them.
pub(crate) fn project_maintenance_instants(statement: &mut SelectStatement, alias: &'static str) {
    statement
        .expr_as(latest_analyze(alias), MaintenanceInstant::LastAnalyzed)
        .expr_as(latest(alias, "last_vacuum", "last_autovacuum"), MaintenanceInstant::LastVacuumed);
}

#[cfg(test)]
mod tests {
    use crate::analyzed_rows::{AnalyzedRows, RowCountReading, project_maintenance_instants};
    use chrono::{DateTime, TimeDelta, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::<Utc>::UNIX_EPOCH + TimeDelta::minutes(minutes)
    }

    fn stored(rows: u64, minutes: i64) -> AnalyzedRows {
        AnalyzedRows {
            rows,
            analyzed_at: at(minutes),
        }
    }

    fn reading(rows: u64, analyzed: Option<i64>, vacuumed: Option<i64>) -> RowCountReading {
        RowCountReading {
            rows,
            last_analyzed: analyzed.map(at),
            last_vacuumed: vacuumed.map(at),
        }
    }

    #[test]
    fn a_reading_after_an_analyze_with_no_later_vacuum_replaces_the_stored_rows() {
        assert_eq!(
            reading(850_000, Some(10), Some(5)).replacement(Some(stored(800_000, 1))),
            Some(stored(850_000, 10))
        );
        assert_eq!(
            reading(850_000, Some(10), None).replacement(Some(stored(800_000, 1))),
            Some(stored(850_000, 10))
        );
    }

    #[test]
    fn a_reading_after_a_later_vacuum_keeps_the_stored_rows() {
        assert_eq!(reading(1_100_000, Some(10), Some(11)).replacement(Some(stored(850_000, 10))), None);
        assert_eq!(
            reading(1_100_000, Some(12), Some(12)).replacement(Some(stored(850_000, 10))),
            None,
            "a vacuum at the same instant may have written the count"
        );
    }

    #[test]
    fn a_reading_of_the_analyze_already_stored_keeps_the_stored_rows() {
        assert_eq!(reading(860_000, Some(10), Some(5)).replacement(Some(stored(850_000, 10))), None);
    }

    #[test]
    fn a_table_first_analyzed_after_its_vacuums_stores_its_rows_and_never_before() {
        assert_eq!(reading(850_000, None, Some(5)).replacement(None), None);
        assert_eq!(reading(850_000, Some(4), Some(5)).replacement(None), None);
        assert_eq!(reading(850_000, Some(6), Some(5)).replacement(None), Some(stored(850_000, 6)));
    }

    #[test]
    fn statistics_reset_after_an_analyze_keeps_the_stored_rows() {
        assert_eq!(reading(0, None, None).replacement(Some(stored(850_000, 10))), None);
    }

    #[test]
    fn the_instants_are_the_later_of_the_manual_and_the_automatic_one() {
        let mut statement = Query::select();
        statement.from_as(Alias::new("pg_stat_user_tables"), Alias::new("s"));
        project_maintenance_instants(&mut statement, "s");
        let sql = statement.to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"GREATEST("s"."last_analyze", "s"."last_autoanalyze") AS "last_analyzed""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"GREATEST("s"."last_vacuum", "s"."last_autovacuum") AS "last_vacuumed""#),
            "{sql}"
        );
    }
}
