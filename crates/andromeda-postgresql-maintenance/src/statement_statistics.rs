//! Whether `pg_stat_statements` still holds every statement the broker runs.
//!
//! `pg_stat_statements` keys its entries by database, so one database per
//! tenant multiplies the broker's statements by the tenant count. Once
//! `pg_stat_statements.max` entries exist, the least used are evicted, and the
//! statistics every measurement of the broker's statements reads become
//! partial: with a few hundred distinct statements per tenant database, a few
//! dozen tenants exceed the default of 5,000 entries. The broker never creates
//! the extension: an operator who wants statement statistics creates it in a
//! tenant database, and the count, which is cluster-wide, is read there. A
//! database without the extension reports nothing. Evictions are counted since
//! the statistics were last reset: the report repeats until an operator raises
//! `pg_stat_statements.max`, restarts the server and calls
//! `pg_stat_statements_reset()`.

use crate::{
    catalog_counter::{CatalogCounter, non_negative},
    error::MaintenanceError,
    observation::{Finding, ObservationDecision, ObservationLevel},
    state::RegisteredTenant,
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    introspection::current_setting,
    maintenance::to_regclass,
    pg_type::PostgresType,
    setting::Setting,
    statement::Statement,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Iden, Query, SelectStatement};
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
    let installed = presence_statement().to_query_scalar::<bool>().fetch_one(&mut *connection).await?;
    if !installed {
        return Ok(());
    }
    let row = statement_statistics_statement().to_query().fetch_one(connection).await?;
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

/// Whether the database resolves `pg_stat_statements_info`, which only the
/// extension creates.
///
/// `to_regclass` answers `NULL` for a name that resolves to nothing, so the
/// probe reads `false` rather than failing where the extension is absent, and
/// the statistics statement, which would fail there, never runs.
fn presence_statement() -> SelectStatement {
    Query::select().expr(to_regclass(Expr::val(INFO_VIEW)).is_not_null()).to_owned()
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
        .expr_as(
            cast(current_setting(Setting::PgStatStatementsMax), PostgresType::Bigint),
            InfoColumn::MaxEntries,
        )
        .from(Alias::new(INFO_VIEW))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{
        observation::{Finding, ObservationDecision},
        statement_statistics::{StatementStatistics, presence_statement, statement_statistics_statement},
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

    #[test]
    fn the_presence_probe_asks_whether_the_info_view_resolves_to_a_relation() {
        assert_eq!(
            presence_statement().to_string(PostgresQueryBuilder),
            "SELECT to_regclass('pg_stat_statements_info') IS NOT NULL"
        );
    }
}
