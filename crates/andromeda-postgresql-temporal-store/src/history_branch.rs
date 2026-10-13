//! Deciding which relations of a branch a temporal read takes history from.
//!
//! A Snapshot branch has no queue. The live branch reads its queued entries
//! beside its recorded rows while the queue holds an intent, and its recorded
//! rows alone otherwise: a write acknowledged before the read began is either
//! applied or still queued when the probe runs, so the shortcut loses nothing
//! a client could see, and the union it saves costs about as much again as
//! the statement it extends.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
use andromeda_postgresql_seaquery::literal::int_literal;
use sea_query::{Expr, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::AssertSqlSafe;

impl TemporalRepository {
    /// The relations of `target` a temporal read takes history from.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the queue probe fails.
    pub(crate) async fn history_branch(&self, target: &ContextTarget) -> Result<HistoryBranch, TemporalStoreError> {
        if let Some(schema) = schema_iden(target) {
            return Ok(HistoryBranch::recorded_in(schema));
        }
        let (sql, values) = queue_holds_intents().build_sqlx(PostgresQueryBuilder);
        let queued: bool = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_one(&self.pool).await?;
        Ok(if queued { HistoryBranch::live() } else { HistoryBranch::live_recorded() })
    }
}

/// Whether the live branch's queue holds any intent.
fn queue_holds_intents() -> SelectStatement {
    let mut intent = Query::select();
    intent.expr(int_literal(1)).from(TemporalAppendIntents::Table);
    Query::select().expr(Expr::exists(intent)).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::history_branch::queue_holds_intents;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_probe_asks_whether_the_queue_holds_any_intent() {
        pretty_assertions::assert_eq!(
            queue_holds_intents().to_string(PostgresQueryBuilder),
            r#"SELECT EXISTS(SELECT 1 FROM "temporal_append_intents")"#
        );
    }
}
