//! Persistence layer for the notification outbox.
//!
//! Provides transactional batch inserts (for atomic dual-write with entity
//! mutations) and delivery-lifecycle operations (claim, mark delivered/failed,
//! cleanup).

use crate::{
    core::error::AndromedaError,
    domain::{
        notification::TriggerReason,
        outbox::{NewOutboxEntry, OutboxEntry, OutboxEntryId, WorkerInstanceId},
        uri::Uri,
    },
};
use chrono::{DateTime, Utc};
use sqlx::{Row, postgres::PgPool};
use std::str::FromStr;

/// Repository for notification outbox persistence operations.
#[derive(Debug)]
pub struct OutboxRepository {
    pool: PgPool,
}

impl OutboxRepository {
    /// Create a new outbox repository backed by the given connection pool.
    pub fn new(pool: &PgPool) -> OutboxRepository {
        OutboxRepository { pool: pool.clone() } // clone: Arc ref-count bump (PgPool is Arc-based)
    }

    /// Insert a batch of outbox entries within an existing transaction.
    ///
    /// Inserts entries sequentially within the caller's transaction. While not
    /// using UNNEST (SQLx lacks support for 2D arrays needed for `TEXT[][]`),
    /// the single transaction minimises round-trip overhead.
    pub async fn insert_batch_in_tx(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, entries: &[NewOutboxEntry]) -> Result<(), AndromedaError> {
        for entry in entries {
            sqlx::query(
                r#"
                INSERT INTO notification_outbox
                    (subscription_id, entity_id, trigger, entity_snapshot, previous_snapshot, changed_attrs)
                VALUES ($1, $2, $3, $4, $5, $6)
                "#,
            )
            .bind(entry.subscription_id.to_string())
            .bind(entry.entity_id.to_string())
            .bind(entry.trigger.to_string())
            .bind(&entry.entity_snapshot)
            .bind(&entry.previous_snapshot)
            .bind(&entry.changed_attrs)
            .execute(&mut **tx)
            .await?;
        }

        Ok(())
    }

    /// Claim a batch of pending outbox entries for delivery.
    ///
    /// Uses `FOR UPDATE SKIP LOCKED` for safe concurrent worker access.
    /// Updates `claimed_at` and `claimed_by` on claimed rows and returns them.
    /// Only claims entries with `attempt_count < max_attempts`.
    pub async fn claim_pending(&self, worker_id: &WorkerInstanceId, batch_size: i32, max_attempts: i32) -> Result<Vec<OutboxEntry>, AndromedaError> {
        let rows = sqlx::query(
            r#"
            WITH claimable AS (
                SELECT id
                FROM notification_outbox
                WHERE delivered_at IS NULL
                  AND attempt_count < $3
                  AND next_attempt_at <= NOW()
                ORDER BY next_attempt_at
                FOR UPDATE SKIP LOCKED
                LIMIT $2
            )
            UPDATE notification_outbox o
            SET claimed_at = NOW(), claimed_by = $1
            FROM claimable c
            WHERE o.id = c.id
            RETURNING o.id, o.subscription_id, o.entity_id, o.trigger,
                      o.entity_snapshot, o.previous_snapshot, o.changed_attrs,
                      o.created_at, o.claimed_at, o.claimed_by,
                      o.delivered_at, o.delivery_error, o.attempt_count,
                      o.next_attempt_at
            "#,
        )
        .bind(worker_id.as_str())
        .bind(batch_size)
        .bind(max_attempts)
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(row_to_outbox_entry).collect()
    }

    /// Mark an outbox entry as successfully delivered.
    pub async fn mark_delivered(&self, id: OutboxEntryId) -> Result<(), AndromedaError> {
        let id_val = id.into_inner();
        sqlx::query("UPDATE notification_outbox SET delivered_at = NOW() WHERE id = $1")
            .bind(id_val)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Mark an outbox entry as failed, incrementing the attempt counter and
    /// scheduling a retry at `next_attempt_at`. Clears the claim so another
    /// worker can pick it up.
    pub async fn mark_failed(&self, id: OutboxEntryId, error: &str, next_attempt_at: DateTime<Utc>) -> Result<(), AndromedaError> {
        let id_val = id.into_inner();
        sqlx::query(
            r#"
            UPDATE notification_outbox
            SET attempt_count = attempt_count + 1,
                delivery_error = $2,
                next_attempt_at = $3,
                claimed_at = NULL,
                claimed_by = NULL
            WHERE id = $1
            "#,
        )
        .bind(id_val)
        .bind(error)
        .bind(next_attempt_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete delivered outbox entries older than the given cutoff timestamp.
    /// Returns the number of rows removed.
    pub async fn delete_delivered_before(&self, cutoff: DateTime<Utc>) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM notification_outbox WHERE delivered_at IS NOT NULL AND delivered_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

/// Convert a database row to an [`OutboxEntry`].
fn row_to_outbox_entry(row: &sqlx::postgres::PgRow) -> Result<OutboxEntry, AndromedaError> {
    let trigger_str: String = row.get("trigger");
    let trigger = TriggerReason::from_str(&trigger_str).map_err(|e| AndromedaError::internal(format!("Invalid trigger reason '{}': {}", trigger_str, e)))?;

    let subscription_id_str: String = row.get("subscription_id");
    let subscription_id: Uri = subscription_id_str
        .parse()
        .map_err(|e| AndromedaError::internal(format!("Invalid subscription URI '{}': {}", subscription_id_str, e)))?;

    let entity_id_str: String = row.get("entity_id");
    let entity_id: Uri = entity_id_str
        .parse()
        .map_err(|e| AndromedaError::internal(format!("Invalid entity URI '{}': {}", entity_id_str, e)))?;

    let claimed_by: Option<WorkerInstanceId> = row.get::<Option<String>, _>("claimed_by").map(WorkerInstanceId::from);

    Ok(OutboxEntry {
        id: OutboxEntryId::from(row.get::<i64, _>("id")),
        subscription_id,
        entity_id,
        trigger,
        entity_snapshot: row.get("entity_snapshot"),
        previous_snapshot: row.get("previous_snapshot"),
        changed_attrs: row.get("changed_attrs"),
        created_at: row.get("created_at"),
        claimed_at: row.get("claimed_at"),
        claimed_by,
        delivered_at: row.get("delivered_at"),
        delivery_error: row.get("delivery_error"),
        attempt_count: row.get("attempt_count"),
        next_attempt_at: row.get("next_attempt_at"),
    })
}

// Note: OutboxRepository methods require a database connection for testing.
// Integration tests covering these methods are in the main integration test suite.
