//! Running one compiled temporal statement on the read share of the tenant
//! pool: every statement here answers a temporal query.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
use sea_query::SelectStatement;
use sqlx::postgres::PgRow;

impl TemporalRepository {
    /// Execute a compiled temporal statement tree on a connection of the read
    /// share as `planning` asks, and observe it under `family`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when no connection is free
    /// within the acquire deadline, the statement fails, or the transaction
    /// it runs in cannot begin or commit.
    pub(crate) async fn execute_temporal_query(
        &self,
        family: ObservedStatement,
        statement: &SelectStatement,
        planning: StatementPlanning,
    ) -> Result<Vec<PgRow>, TemporalStoreError> {
        let mut connection = self.reads.acquire().await?;
        Ok(observe_statement(family, connection.fetch_planned(statement, planning)).await?)
    }
}
