//! Repository for the `pending_side_effects` durable queue.
//!
//! Concurrency model: lease-based. `lease_batch` claims ready rows via
//! `FOR UPDATE SKIP LOCKED`, stamps a `locked_until` lease, and returns
//! their payloads. On success the worker calls `ack` (delete row); on
//! failure it calls `nack` (increment attempts, schedule retry); once
//! `attempts` exceeds the configured maximum the worker calls `mark_dead`
//! which keeps the row for operator inspection.

use crate::{
    core::error::AndromedaError,
    repository::pending_side_effects::payload::PersistedBatchSideEffectJob,
    service::entity::{
        side_effect::BatchSideEffectJob,
        side_effect_queue::{LeasedSideEffect, SideEffectQueue},
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Postgres, Row, Transaction, postgres::PgPool};
use tracing::error;
use uuid::Uuid;

/// Postgres-backed queue for deferred batch side-effects.
#[derive(Debug)]
pub struct PendingSideEffectsRepository {
    pool: PgPool,
}

impl PendingSideEffectsRepository {
    /// Create a new repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> PendingSideEffectsRepository {
        PendingSideEffectsRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Append a side-effect job within an existing transaction. The row
    /// becomes visible to workers only when the enclosing tx commits, so a
    /// rolled-back entity write guarantees no orphaned queue entry.
    pub async fn enqueue_in_tx(tx: &mut Transaction<'_, Postgres>, payload: &PersistedBatchSideEffectJob) -> Result<Uuid, AndromedaError> {
        let payload_json =
            serde_json::to_value(payload).map_err(|err| AndromedaError::internal(format!("failed to encode pending_side_effects payload: {err}")))?;
        let row = sqlx::query("INSERT INTO pending_side_effects (payload) VALUES ($1) RETURNING id")
            .bind(payload_json)
            .fetch_one(&mut **tx)
            .await?;
        let id: Uuid = row.get("id");
        Ok(id)
    }
}

#[async_trait]
impl SideEffectQueue for PendingSideEffectsRepository {
    /// Claims are stamped with `locked_until = NOW() + lease_seconds`. Rows
    /// already leased by live workers are skipped; rows whose lease has expired
    /// are reclaimable, which is how stale leases from a crashed worker are
    /// recovered without explicit cleanup.
    async fn lease_batch(&self, worker_id: &str, batch_size: i32, lease_seconds: i32) -> Result<Vec<LeasedSideEffect>, AndromedaError> {
        let rows = sqlx::query(
            r#"
            WITH claimable AS (
                SELECT id
                FROM pending_side_effects
                WHERE dead = FALSE
                  AND next_attempt_at <= NOW()
                  AND (locked_until IS NULL OR locked_until <= NOW())
                ORDER BY next_attempt_at
                FOR UPDATE SKIP LOCKED
                LIMIT $2
            )
            UPDATE pending_side_effects p
            SET locked_until = NOW() + make_interval(secs => $3),
                locked_by = $1
            FROM claimable c
            WHERE p.id = c.id
            RETURNING p.id, p.attempts, p.payload
            "#,
        )
        .bind(worker_id)
        .bind(batch_size)
        .bind(f64::from(lease_seconds))
        .fetch_all(&self.pool)
        .await?;

        let mut leased = Vec::with_capacity(rows.len());
        for row in rows {
            let id: Uuid = row.get("id");
            match decode_job(row.get("payload")) {
                Ok(job) => leased.push(LeasedSideEffect {
                    id,
                    attempts: row.get("attempts"),
                    job,
                }),
                Err(error) => {
                    // A payload this process cannot read will never become
                    // readable, so retrying it would block the queue behind it
                    // for the whole retry budget.
                    let detail = error.to_string();
                    error!(error = %detail, job_id = %id, event = "pending_side_effect_payload_unreadable");
                    if let Err(mark_error) = self.mark_dead(id, &detail).await {
                        error!(error = %mark_error, job_id = %id, event = "pending_side_effect_mark_dead_failed");
                    }
                }
            }
        }
        Ok(leased)
    }

    /// Delete a successfully processed job. Returns `true` if a row was
    /// removed.
    async fn ack(&self, id: Uuid) -> Result<bool, AndromedaError> {
        let result = sqlx::query("DELETE FROM pending_side_effects WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Record a failed attempt, schedule a retry at `next_attempt_at`, and
    /// release the lease so another worker can reclaim the row. Returns
    /// `true` if a row was updated.
    async fn nack(&self, id: Uuid, error: &str, next_attempt_at: DateTime<Utc>) -> Result<bool, AndromedaError> {
        let result = sqlx::query(
            r#"
            UPDATE pending_side_effects
            SET attempts = attempts + 1,
                last_error = $2,
                next_attempt_at = $3,
                locked_until = NULL,
                locked_by = NULL
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(error)
        .bind(next_attempt_at)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Dead-letter a row whose attempts exceeded the maximum. Keeps the
    /// row in the table with `dead = TRUE` so operators can inspect the
    /// payload and error history.
    async fn mark_dead(&self, id: Uuid, error: &str) -> Result<bool, AndromedaError> {
        let result = sqlx::query(
            r#"
            UPDATE pending_side_effects
            SET attempts = attempts + 1,
                dead = TRUE,
                last_error = $2,
                locked_until = NULL,
                locked_by = NULL
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

/// Read one stored payload back into the job it describes.
fn decode_job(payload: Value) -> Result<BatchSideEffectJob, AndromedaError> {
    let persisted: PersistedBatchSideEffectJob =
        serde_json::from_value(payload).map_err(|err| AndromedaError::internal(format!("invalid pending_side_effect payload: {err}")))?;
    persisted.try_into()
}
