//! Whether `pg_stat_statements` still holds every statement the broker runs.
//!
//! `pg_stat_statements` keys its entries by database, so one database per
//! tenant multiplies the broker's statements by the tenant count. Once
//! `pg_stat_statements.max` entries exist, the least used are evicted, and the
//! statistics every measurement of the broker's statements reads become
//! partial: with a few hundred distinct statements per tenant database, a few
//! dozen tenants exceed the default of 5,000 entries. The count is
//! cluster-wide and read in the tenant's database, where
//! the migrations create the extension when the server preloads it. Evictions
//! are counted since the statistics were last reset: the report repeats until
//! an operator raises `pg_stat_statements.max`, restarts the server and calls
//! `pg_stat_statements_reset()`.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    observation::{Finding, ObservationDecision, ObservationLevel},
    scalar_query::select_bool,
    state::RegisteredTenant,
};
use andromeda_postgresql_seaquery::{cast::cast, introspection::current_setting, maintenance::to_regclass, pg_type::PostgresType};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Iden, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{PgConnection, Row};

/// The `pg_stat_statements_info` view the extension creates.
const INFO_VIEW: &str = "pg_stat_statements_info";

/// What `pg_stat_statements_info` reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StatementStatistics {
    /// Entries evicted since the statistics were last reset.
    pub(crate) evictions: u64,
    /// `pg_stat_statements.max`.
    pub(crate) max_entries: u64,
    /// When the statistics were last reset.
    pub(crate) reset_at: DateTime<Utc>,
}

impl StatementStatistics {
    /// The finding evictions report.
    pub(crate) const fn finding(self) -> Option<Finding> {
        if self.evictions > 0 {
            Some(Finding::warning(ObservationDecision::StatementStatisticsEvicted))
        } else {
            None
        }
    }
}

/// Report evictions from `pg_stat_statements`, when the tenant's database
/// carries the extension.
pub(crate) async fn report_statement_statistics(connection: &mut PgConnection, tenant: &RegisteredTenant) -> Result<(), MaintenanceError> {
    let installed = select_bool(connection, Query::select().expr(to_regclass(Expr::val(INFO_VIEW)).is_not_null()).to_owned()).await?;
    if !installed {
        return Ok(());
    }
    let (sql, values) = statement_statistics_statement().build_sqlx(PostgresQueryBuilder);
    let row = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(connection).await?;
    let statistics = StatementStatistics {
        evictions: non_negative(row.try_get(InfoColumn::Dealloc.to_string().as_str())?, CatalogCounter::StatementEvictions)?,
        max_entries: non_negative(row.try_get(InfoColumn::MaxEntries.to_string().as_str())?, CatalogCounter::StatementEvictions)?,
        reset_at: row.try_get(InfoColumn::StatsReset.to_string().as_str())?,
    };
    if let Some(finding) = statistics.finding() {
        observation_event!(finding, tenant = tenant.log_name(), database = %tenant.database, evictions = statistics.evictions, max_entries = statistics.max_entries, reset_at = %statistics.reset_at, "pg_stat_statements has evicted entries since its statistics were reset");
    }
    Ok(())
}

/// The columns the statement projects.
#[derive(Clone, Copy, Iden)]
enum InfoColumn {
    /// `dealloc`.
    Dealloc,
    /// `stats_reset`.
    StatsReset,
    /// `pg_stat_statements.max`.
    MaxEntries,
}

/// The evictions, the last reset and the entry limit of `pg_stat_statements`.
fn statement_statistics_statement() -> SelectStatement {
    Query::select()
        .column(InfoColumn::Dealloc)
        .column(InfoColumn::StatsReset)
        .expr_as(cast(current_setting("pg_stat_statements.max"), PostgresType::Bigint), InfoColumn::MaxEntries)
        .from(Alias::new(INFO_VIEW))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{
        observation::{Finding, ObservationDecision},
        statement_statistics::{StatementStatistics, statement_statistics_statement},
    };
    use chrono::Utc;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn an_eviction_reports_statement_statistics_evicted_as_a_warning() {
        let statistics = |evictions| StatementStatistics {
            evictions,
            max_entries: 5_000,
            reset_at: Utc::now(),
        };
        assert_eq!(statistics(0).finding(), None);
        assert_eq!(
            statistics(166).finding(),
            Some(Finding::warning(ObservationDecision::StatementStatisticsEvicted))
        );
    }

    #[test]
    fn the_statement_reads_the_evictions_beside_the_entry_limit() {
        assert_eq!(
            statement_statistics_statement().to_string(PostgresQueryBuilder),
            r#"SELECT "dealloc", "stats_reset", current_setting('pg_stat_statements.max')::bigint AS "max_entries" FROM "pg_stat_statements_info""#
        );
    }
}
