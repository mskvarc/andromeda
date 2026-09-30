//! Repository for the `pending_side_effects` durable queue.
//!
//! The repository implements the entity policy's `SideEffectQueue` port in
//! this crate and also exposes the transaction-scoped enqueue operation used
//! by the PostgreSQL atomic committer.
//!
//! The queue lives only in the live branch, in the tenant's default schema:
//! a request scoped to a Snapshot runs its side effects inline, so no statement
//! here names a branch.

use crate::{error::ContextStoreError, pending_side_effects::payload::PersistedBatchSideEffectJob};
use andromeda_context_commit::{
    job_lease::JobLease,
    lease_acknowledgement::LeaseAcknowledgement,
    lease_holder::LeaseHolder,
    side_effect_job_id::SideEffectJobId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_postgresql_schema::queue::PendingSideEffects;
use andromeda_postgresql_seaquery::clock::wall_clock;
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, InsertStatement, OnConflict, PostgresQueryBuilder, Query, error::Error as SeaQueryError};
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
fn enqueue_statement(id: Uuid, write_id: Uuid, recorded_at: DateTime<Utc>, base_ordinal: i32, payload: Value) -> Result<InsertStatement, SeaQueryError> {
    let mut statement = Query::insert();
    statement
        .into_table(PendingSideEffects::Table)
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

/// Remove a job its replay settles, while this worker's lease on it is live.
///
/// Issued as the replay transaction's first statement: the deleted row stays
/// locked until that transaction ends, which is what keeps a reclaim from
/// taking the job while its replay runs, however long that is. The lease has
/// to be live when the replay starts, though -- once it lapsed, another worker
/// may already have reclaimed and be replaying the job -- so this, unlike a
/// release of a failed attempt, compares the lease with the clock.
///
/// The lease is compared against the wall clock, not the transaction clock: a
/// replay transaction may have waited for its connection, and a lease that
/// lapsed before the statement ran must refuse it even when the transaction
/// began earlier.
fn settle_statement(id: Uuid, owner: LeaseHolder) -> DeleteStatement {
    Query::delete()
        .from_table(PendingSideEffects::Table)
        .and_where(Expr::col(PendingSideEffects::Id).eq(id))
        .and_where(Expr::col(PendingSideEffects::ClaimedBy).eq(owner.as_uuid()))
        .and_where(Expr::col(PendingSideEffects::LeaseUntil).gt(wall_clock()))
        .to_owned()
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
    /// Returns [`ContextStoreError::PayloadEncoding`] when the payload cannot be
    /// encoded as JSON, when the consequence base does not fit the column's
    /// signed range, or when the insert statement cannot be built, and
    /// [`ContextStoreError::Database`] when the insert fails.
    pub async fn enqueue_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        record: &WriteRecord,
        base_ordinal: WriteOrdinal,
        payload: &PersistedBatchSideEffectJob,
    ) -> Result<(), ContextStoreError> {
        let payload_json = serde_json::to_value(payload).map_err(ContextStoreError::PayloadEncoding)?;
        let base_ordinal = base_ordinal.as_i32()?;
        let statement = enqueue_statement(
            SideEffectJobId::mint().as_uuid(),
            record.write_id().as_uuid(),
            record.recorded_at(),
            base_ordinal,
            payload_json,
        )?;
        let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut **tx).await?;
        Ok(())
    }

    /// Remove the job `lease` names within an existing transaction, but only
    /// while its lease is still held.
    ///
    /// The replay of a job runs this as its transaction's first statement and
    /// commits its consequences after it, so the consequences and the job's
    /// removal land together or not at all. The removal locks the row, so a
    /// worker reclaiming an expired lease skips it until this transaction ends
    /// and finds it gone if it commits: the replay may outlast its lease
    /// without the job being taken from under it.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Database`] when the statement fails.
    pub async fn settle_in_tx(tx: &mut Transaction<'_, Postgres>, lease: JobLease) -> Result<LeaseAcknowledgement, ContextStoreError> {
        let (sql, values) = settle_statement(lease.job.as_uuid(), lease.holder).build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(&mut **tx).await?;
        Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
    }
}

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::repo::{enqueue_statement, settle_statement};
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use serde_json::json;
    use uuid::Uuid;

    fn rendered() -> String {
        enqueue_statement(Uuid::nil(), Uuid::nil(), Utc::now(), 0, json!({}))
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

    /// The settlement is a lease-guarded delete: it reaches the job only while
    /// this worker claims it under a live lease, and the row it deletes stays
    /// locked for the rest of the replay transaction.
    #[test]
    fn the_settlement_deletes_only_the_job_this_worker_holds_under_a_live_lease() {
        let sql = settle_statement(Uuid::nil(), LeaseHolder::from(Uuid::nil())).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"DELETE FROM "pending_side_effects" WHERE"#), "{sql}");
        assert!(sql.contains(r#""id" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
        assert!(sql.contains(r#""lease_until" > CLOCK_TIMESTAMP()"#), "{sql}");
    }
}
