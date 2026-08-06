//! The PostgreSQL notification outbox.
//!
//! Rows are parsed into the policy's entry type by [`super::mapper`] before
//! they leave: a row that cannot be read back is dropped with a log line
//! rather than failing the claim, so one unreadable entry cannot stall every
//! other pending notification.

use crate::{
    core::error::AndromedaError,
    repository::outbox::{mapper::row_to_domain, rows::OutboxEntryRow},
    service::outbox::{
        entry::{NewOutboxEntry, OutboxEntry, OutboxEntryId, WorkerInstanceId},
        store::OutboxStore,
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::{QueryBuilder, postgres::PgPool};
use tracing::error;

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
    /// Builds a single multi-row `INSERT ... VALUES (...), (...), ...` via
    /// [`sqlx::QueryBuilder::push_values`], collapsing N round-trips into one.
    pub async fn insert_batch_in_tx(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, entries: &[&NewOutboxEntry]) -> Result<(), AndromedaError> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            "INSERT INTO notification_outbox \
             (subscription_id, entity_id, trigger, entity_snapshot, previous_snapshot, changed_attrs) ",
        );
        builder.push_values(entries, |mut b, entry| {
            b.push_bind(entry.subscription_id.to_string())
                .push_bind(entry.entity_id.to_string())
                .push_bind(entry.trigger.to_string())
                .push_bind(&entry.entity_snapshot)
                .push_bind(&entry.previous_snapshot)
                // The column is `text[]`; the domain name type is not an sqlx `Encode`, so it is rendered here at the persistence boundary.
                .push_bind(entry.changed_attrs.iter().map(ToString::to_string).collect::<Vec<_>>());
        });
        builder.build().execute(&mut **tx).await?;

        Ok(())
    }
}

#[async_trait]
impl OutboxStore for OutboxRepository {
    /// Uses `FOR UPDATE SKIP LOCKED` so several workers can share one outbox.
    async fn claim_pending(&self, worker_id: &WorkerInstanceId, batch_size: i32, max_attempts: i32) -> Result<Vec<OutboxEntry>, AndromedaError> {
        let rows = sqlx::query_as::<_, OutboxEntryRow>(
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

        Ok(rows.into_iter().filter_map(into_entry).collect())
    }

    /// Mark an outbox entry as successfully delivered. Returns rows affected.
    async fn mark_delivered(&self, id: OutboxEntryId) -> Result<u64, AndromedaError> {
        let id_val = id.into_inner();
        let result = sqlx::query("UPDATE notification_outbox SET delivered_at = NOW() WHERE id = $1")
            .bind(id_val)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    /// Mark an outbox entry as failed, incrementing the attempt counter and
    /// scheduling a retry at `next_attempt_at`. Clears the claim so another
    /// worker can pick it up. Returns rows affected.
    async fn mark_failed(&self, id: OutboxEntryId, error: &str, next_attempt_at: DateTime<Utc>) -> Result<u64, AndromedaError> {
        let id_val = id.into_inner();
        let result = sqlx::query(
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
        Ok(result.rows_affected())
    }

    /// Delete delivered outbox entries older than the given cutoff timestamp.
    /// Returns the number of rows removed.
    async fn delete_delivered_before(&self, cutoff: DateTime<Utc>) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM notification_outbox WHERE delivered_at IS NOT NULL AND delivered_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
}

/// Parse one claimed row, logging and dropping it if its stored shape is
/// unreadable. Dropping is deliberate: the entry stays claimed and will be
/// reaped, and one bad row does not stall the rest of the batch.
fn into_entry(row: OutboxEntryRow) -> Option<OutboxEntry> {
    match row_to_domain(row) {
        Ok(entry) => Some(entry),
        Err(error) => {
            error!(error = %error, event = "outbox_row_unreadable");
            None
        }
    }
}
