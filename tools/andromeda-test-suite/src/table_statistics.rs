//! The live and dead rows of one table of a tenant database, read from `pg_stat_user_tables` over a direct connection
//! to the PostgreSQL server of the broker holding the tenant, with the server's administrative login. An operator
//! reads the same view to see whether a vacuum has caught up with a deletion.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    database_name::DatabaseName,
};
use andromeda_postgresql_seaquery::{statement::Statement, system_catalog::PgStatUserTables};
use derive_more::Display;
use sea_query::{Expr, ExprTrait, Iden, Query};
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use std::time::Duration;
use strum::IntoStaticStr;

/// The longest a reading waits for its one connection.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// The schema holding a tenant's live relations.
const LIVE_SCHEMA: &str = "public";

/// A table of a tenant's live branch whose statistics a test reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum TenantTable {
    /// `current_attributes`, one row per attribute instance.
    CurrentAttributes,
}

/// The rows of a table as the server's statistics estimate them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
#[display("{live} live, {dead} dead")]
pub struct TupleCounts {
    /// `n_live_tup`.
    pub live: u64,
    /// `n_dead_tup`.
    pub dead: u64,
}

impl TupleCounts {
    /// Whether the dead rows make up less than `percent` of all rows. A table without rows has no dead share.
    #[must_use]
    pub const fn dead_share_below(self, percent: u64) -> bool {
        self.dead == 0 || self.dead.saturating_mul(100) < self.live.saturating_add(self.dead).saturating_mul(percent)
    }
}

/// Why a table's statistics could not be read.
#[derive(Debug, thiserror::Error)]
pub enum TableStatisticsError {
    /// The stack names no server for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The server could not be reached or the statement failed.
    #[error("the tenant database could not be read")]
    Server(#[from] sqlx::Error),
    /// The database holds no such table.
    #[error("the tenant database holds no table {0}")]
    NoTable(&'static str),
    /// A count the server answered is below zero.
    #[error("the server counted {0} rows")]
    NegativeCount(i64),
}

/// A direct connection to one tenant database on the server of one broker.
#[derive(Debug, Clone)]
pub struct TableStatistics {
    /// One connection, opened on first use.
    pool: PgPool,
}

impl TableStatistics {
    /// The tenant database `database` on the server of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`TableStatisticsError::Config`] when the stack names no such server.
    pub fn of(role: BrokerRole, database: &DatabaseName) -> Result<TableStatistics, TableStatisticsError> {
        let config = SuiteConfig::get().map_err(Clone::clone)?;
        let mut server = config.pooling()?.server(role)?.clone();
        server.set_path(database.as_str());
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_lazy(server.as_str())?;
        Ok(TableStatistics { pool })
    }

    /// The live and dead rows of `table` now.
    ///
    /// # Errors
    ///
    /// Returns [`TableStatisticsError`] when the database cannot be read or holds no such table.
    pub async fn tuples(&self, table: TenantTable) -> Result<TupleCounts, TableStatisticsError> {
        let name: &'static str = table.into();
        let statement = Query::select()
            .column(PgStatUserTables::NLiveTup)
            .column(PgStatUserTables::NDeadTup)
            .from(PgStatUserTables::Table)
            .and_where(Expr::col(PgStatUserTables::Schemaname).eq(LIVE_SCHEMA))
            .and_where(Expr::col(PgStatUserTables::Relname).eq(name))
            .to_owned();
        let row = statement
            .to_query()
            .fetch_optional(&self.pool)
            .await?
            .ok_or(TableStatisticsError::NoTable(name))?;
        let count = |column: PgStatUserTables| -> Result<u64, TableStatisticsError> {
            let value: i64 = row.try_get(column.unquoted())?;
            u64::try_from(value).map_err(|_| TableStatisticsError::NegativeCount(value))
        };
        Ok(TupleCounts {
            live: count(PgStatUserTables::NLiveTup)?,
            dead: count(PgStatUserTables::NDeadTup)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::table_statistics::{TenantTable, TupleCounts};
    use googletest::prelude::*;

    #[gtest]
    fn a_table_with_a_tenth_of_its_rows_dead_is_not_below_ten_percent() {
        expect_that!(TupleCounts { live: 90, dead: 10 }.dead_share_below(10), eq(false));
    }

    #[gtest]
    fn a_vacuumed_table_is_below_any_share() {
        expect_that!(TupleCounts { live: 48_000, dead: 0 }.dead_share_below(10), eq(true));
    }

    #[gtest]
    fn an_empty_table_has_no_dead_share() {
        expect_that!(TupleCounts { live: 0, dead: 0 }.dead_share_below(10), eq(true));
    }

    #[gtest]
    fn the_table_name_is_the_relation_name() {
        let name: &'static str = TenantTable::CurrentAttributes.into();
        expect_that!(name, eq("current_attributes"));
    }
}
