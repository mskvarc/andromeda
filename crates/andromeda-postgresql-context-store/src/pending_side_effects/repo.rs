//! Repository for the `pending_side_effects` durable queue.
//!
//! The repository implements the entity policy's `SideEffectQueue` port in
//! this crate and also exposes the transaction-scoped enqueue operation used
//! by the PostgreSQL atomic committer.

use crate::{error::ContextStoreError, pending_side_effects::payload::PersistedBatchSideEffectJob};
use andromeda_context_commit::{side_effect_job_id::SideEffectJobId, write_ordinal::WriteOrdinal, write_record::WriteRecord};
use sqlx::{Postgres, Transaction, postgres::PgPool};

/// Enqueue one deferred job under the identity of the write that produced it.
///
/// `ON CONFLICT (write_id) DO NOTHING` is what makes the enqueue idempotent: a
/// commit that is replayed under its original write identity finds its job
/// already queued and adds no second copy, so the at-least-once drain never
/// executes the same batch's side effects twice because of a retried commit.
///
/// `recorded_at` and `consequence_base` are persisted alongside the payload so
/// a worker that later leases this job can rebuild the exact
/// [`WriteRecord`](andromeda_context_commit::write_record::WriteRecord) and
/// ordinal base the enqueueing commit used, and resume its temporal and
/// notification consequences under them rather than under a freshly minted
/// identity.
const ENQUEUE_SQL: &str =
    "INSERT INTO pending_side_effects (id, write_id, recorded_at, consequence_base, payload) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (write_id) DO NOTHING";

/// Postgres-backed queue for deferred batch side-effects.
#[derive(Debug)]
pub struct PendingSideEffectsRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

impl PendingSideEffectsRepository {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> PendingSideEffectsRepository {
        PendingSideEffectsRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Borrow the pool for focused port implementations in this crate.
    pub const fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Append a side-effect job within an existing transaction. The row
    /// becomes visible to workers only when the enclosing tx commits, so a
    /// rolled-back entity write guarantees no orphaned queue entry.
    ///
    /// The row identity is minted here rather than defaulted by the database,
    /// and the identity of the committing write is stored alongside it as the
    /// deduplication key. `record` and `consequence_base` are persisted
    /// verbatim, so a worker that leases this job later can resume the
    /// enqueueing commit's identity and ordinal counter exactly.
    pub async fn enqueue_in_tx(
        tx: &mut Transaction<'_, Postgres>,
        record: &WriteRecord,
        consequence_base: WriteOrdinal,
        payload: &PersistedBatchSideEffectJob,
    ) -> Result<(), ContextStoreError> {
        let payload_json =
            serde_json::to_value(payload).map_err(|err| ContextStoreError::Internal(format!("failed to encode pending_side_effects payload: {err}")))?;
        let consequence_base = consequence_base
            .as_i32()
            .map_err(|error| ContextStoreError::Internal(format!("pending side-effect consequence base is out of range: {error}")))?;
        sqlx::query(ENQUEUE_SQL)
            .bind(SideEffectJobId::mint().as_uuid())
            .bind(record.write_id().as_uuid())
            .bind(record.recorded_at())
            .bind(consequence_base)
            .bind(payload_json)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::repo::ENQUEUE_SQL;

    #[test]
    fn the_enqueue_supplies_the_row_identity_rather_than_leaving_it_to_the_database() {
        assert!(ENQUEUE_SQL.contains("(id, write_id, recorded_at, consequence_base, payload)"));
    }

    #[test]
    fn a_replayed_write_enqueues_no_second_job() {
        assert!(ENQUEUE_SQL.contains("ON CONFLICT (write_id) DO NOTHING"));
    }
}
