//! Running one compiled temporal statement on the read share of the tenant
//! pool: every statement here answers a temporal query.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::SelectStatement;
use sqlx::postgres::PgRow;

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
    /// The instances a split query's filters read in effect when its window
    /// opens, over the entities a Context Source contributed to (clauses 4.11
    /// and 5.7.4.4).
    InEffectInstances,
    /// The Scope event in force when a split query's window opens, over the
    /// entities a Context Source contributed to (clauses 4.18 and 5.7.4.4).
    InEffectScope,
    /// The instances inside a split query's window of the Relationships its
    /// hops start from, over the entities a Context Source contributed to
    /// (clauses 4.9 and 5.7.4.4).
    WindowRelationships,
    /// Which disqualifying shapes the instances an aggregated request
    /// aggregates carry (clause 5.7.4.4).
    HeldShapes,
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
            TemporalStatement::InEffectInstances => StatementName("temporal-in-effect-instances"),
            TemporalStatement::InEffectScope => StatementName("temporal-in-effect-scope"),
            TemporalStatement::WindowRelationships => StatementName("temporal-window-relationships"),
            TemporalStatement::HeldShapes => StatementName("temporal-held-shapes"),
        }
    }
}

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
        family: TemporalStatement,
        statement: &SelectStatement,
        planning: StatementPlanning,
    ) -> Result<Vec<PgRow>, TemporalStoreError> {
        let mut connection = self.reads.acquire().await?;
        Ok(observe_statement(family.name(), connection.fetch_planned(statement, planning)).await?)
    }
}
