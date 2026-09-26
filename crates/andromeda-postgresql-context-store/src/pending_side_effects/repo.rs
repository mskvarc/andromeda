//! Repository for the `pending_side_effects` durable queue.
//!
//! The repository implements the entity policy's `SideEffectQueue` port in
//! this crate and also exposes the transaction-scoped enqueue operation used
//! by the PostgreSQL atomic committer.

use crate::{error::ContextStoreError, pending_side_effects::payload::PersistedBatchSideEffectJob};
use andromeda_context_commit::{side_effect_job_id::SideEffectJobId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::queue::PendingSideEffects;
use chrono::{DateTime, Utc};
use sea_query::{Expr, InsertStatement, OnConflict, PostgresQueryBuilder, Query, error::Error as SeaQueryError};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{Postgres, Transaction, postgres::PgPool};
use uuid::Uuid;

/// Insert one deferred job under the identity of the write that produced it.
///
/// `ON CONFLICT (write_id) DO NOTHING` is what makes the enqueue idempotent: a
/// commit that is replayed under its original write identity finds its job
/// already queued and adds no second copy, so the at-least-once drain never
/// executes the same batch's side effects twice because of a retried commit.
///
/// `recorded_at` and `base_ordinal` are persisted alongside the payload so
/// a worker that later leases this job can rebuild the exact
/// [`WriteRecord`](andromeda_context_commit::write_record::WriteRecord) and
/// ordinal base the enqueueing commit used, and resume its temporal and
/// notification consequences under them rather than under a freshly minted
/// identity.
fn enqueue_statement(
    target: &ContextTarget,
    id: Uuid,
    write_id: Uuid,
    recorded_at: DateTime<Utc>,
    base_ordinal: i32,
    payload: Value,
) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(table_of(target, PendingSideEffects::Table))
        .columns([
            PendingSideEffects::Id,
            PendingSideEffects::WriteId,
            PendingSideEffects::RecordedAt,
            PendingSideEffects::BaseOrdinal,
            PendingSideEffects::Payload,
        ])
        .values([Expr::val(id), Expr::val(write_id), recorded_at.into(), base_ordinal.into(), Expr::val(payload)])?
        .on_conflict(OnConflict::column(PendingSideEffects::WriteId).do_nothing().to_owned());
    Ok(statement.clone())
}

/// Postgres-backed queue for deferred batch side-effects.
#[derive(Debug)]
pub struct PendingSideEffectsRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl PendingSideEffectsRepository {
    /// Create a new repository backed by the given connection pool.
    #[must_use]
    pub fn new(pool: &PgPool) -> PendingSideEffectsRepository {
        PendingSideEffectsRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Borrow the pool for focused port implementations in this crate.
    #[must_use]
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Append a side-effect job within an existing transaction. The row
    /// becomes visible to workers only when the enclosing tx commits, so a
    /// rolled-back entity write guarantees no orphaned queue entry.
    ///
    /// The row identity is minted here rather than defaulted by the database,
    /// and the identity of the committing write is stored alongside it as the
    /// deduplication key. `record` and `base_ordinal` are persisted
    /// verbatim, so a worker that leases this job later can resume the
    /// enqueueing commit's identity and ordinal counter exactly.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Internal`] when the payload cannot be
    /// encoded as JSON, when the consequence base does not fit the column's
    /// signed range, or when the insert statement cannot be built, and
    /// [`ContextStoreError::Database`] when the insert fails.
    pub async fn enqueue_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        target: &ContextTarget,
        record: &WriteRecord,
        base_ordinal: WriteOrdinal,
        payload: &PersistedBatchSideEffectJob,
    ) -> Result<(), ContextStoreError> {
        let payload_json =
            serde_json::to_value(payload).map_err(|err| ContextStoreError::Internal(format!("failed to encode pending_side_effects payload: {err}")))?;
        let base_ordinal = base_ordinal
            .as_i32()
            .map_err(|error| ContextStoreError::Internal(format!("pending side-effect consequence base is out of range: {error}")))?;
        let statement = enqueue_statement(
            target,
            SideEffectJobId::mint().as_uuid(),
            record.write_id().as_uuid(),
            record.recorded_at(),
            base_ordinal,
            payload_json,
        )
        .map_err(|error| ContextStoreError::Internal(format!("failed to build pending_side_effects insert: {error}")))?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut **tx).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::repo::enqueue_statement;
    use andromeda_context_space::context_target::ContextTarget;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use serde_json::json;
    use uuid::Uuid;

    fn rendered() -> String {
        enqueue_statement(&ContextTarget::Live, Uuid::nil(), Uuid::nil(), Utc::now(), 0, json!({}))
            .expect("arity")
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_enqueue_supplies_the_row_identity_rather_than_leaving_it_to_the_database() {
        let sql = rendered();
        assert!(
            sql.contains(r#""id""#)
                && sql.contains(r#""write_id""#)
                && sql.contains(r#""recorded_at""#)
                && sql.contains(r#""base_ordinal""#)
                && sql.contains(r#""payload""#),
            "{sql}"
        );
    }

    #[test]
    fn a_replayed_write_enqueues_no_second_job() {
        let sql = rendered();
        assert!(sql.contains(r#"ON CONFLICT ("write_id") DO NOTHING"#), "{sql}");
    }
}
