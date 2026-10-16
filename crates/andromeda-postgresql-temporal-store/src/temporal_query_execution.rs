//! Running one compiled temporal statement on the read share of the tenant
//! pool: every statement here answers a temporal query.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{PostgresQueryBuilder, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, postgres::PgRow};

/// Which family of temporal read a compiled statement belongs to.
///
/// The families are the ones the temporal query planner distinguishes: the
/// bounded stage-1 candidate selection, its count, and the stage-2 history and
/// aggregation reads over the selected entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemporalStatement {
    /// Stage 1: which temporal entities the query selects.
    Stage1,
    /// Stage 1 count: how many temporal entities the query selects.
    Stage1Count,
    /// Stage 2: the history rows of one selected entity.
    History,
    /// Stage 2: the reified scope events of one selected entity (clause 4.18).
    ScopeHistory,
    /// Stage 2: the aggregated history of one selected entity.
    Aggregation,
    /// The truth of every leaf of a split query's filters over the entities a
    /// Context Source contributed to (clause 5.7.4.4).
    SplitFilterLeaves,
}

impl TemporalStatement {
    /// The name this family is observed under.
    const fn name(self) -> StatementName {
        match self {
            TemporalStatement::Stage1 => StatementName("temporal-stage1"),
            TemporalStatement::Stage1Count => StatementName("temporal-stage1-count"),
            TemporalStatement::History => StatementName("temporal-history"),
            TemporalStatement::ScopeHistory => StatementName("temporal-scope-history"),
            TemporalStatement::Aggregation => StatementName("temporal-aggregation"),
            TemporalStatement::SplitFilterLeaves => StatementName("temporal-split-filter-leaves"),
        }
    }
}

impl TemporalRepository {
    /// Execute a compiled temporal statement tree on a connection of the read
    /// share, binding its values through `sea-query-sqlx`, and observe it
    /// under `family`.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when no connection is free
    /// within the acquire deadline or the statement fails.
    pub(crate) async fn execute_temporal_query(&self, family: TemporalStatement, statement: SelectStatement) -> Result<Vec<PgRow>, TemporalStoreError> {
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        let mut connection = self.reads.acquire().await?;
        Ok(observe_statement(family.name(), sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&mut **connection)).await?)
    }
}
