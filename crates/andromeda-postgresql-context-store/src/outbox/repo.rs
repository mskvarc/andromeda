//! Direct PostgreSQL implementation of the notification-owned outbox port.

use crate::{
    error::ContextStoreError,
    outbox::rows::{OutboxRecord, OutboxRowError, RawOutboxRow},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use andromeda_notification::{
    error::NotificationError,
    outbox_entry::{OutboxEntry, OutboxEntryId},
    outbox_store::OutboxStore,
    worker_instance_id::WorkerInstanceId,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{QueryBuilder, postgres::PgPool};
use tracing::error;

/// Repository for notification outbox persistence operations.
#[derive(Debug)]
pub struct OutboxRepository {
    /// The tenant connection pool.
    pool: PgPool,
}

#[async_trait]
impl OutboxStore for OutboxRepository {
    async fn claim_pending(&self, worker_id: &WorkerInstanceId, batch_size: i32, max_attempts: i32) -> Result<Vec<OutboxEntry>, NotificationError> {
        let records = OutboxRepository::claim_pending(self, worker_id.as_ref(), batch_size, max_attempts)
            .await
            .map_err(|error| NotificationError::persistence(error.to_string()))?;
        Ok(records.into_iter().filter_map(into_entry).collect())
    }

    async fn mark_delivered(&self, id: OutboxEntryId) -> Result<u64, NotificationError> {
        let result = sqlx::query("UPDATE notification_outbox SET delivered_at = NOW() WHERE id = $1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|error| NotificationError::persistence(error.to_string()))?;
        Ok(result.rows_affected())
    }

    async fn mark_failed(&self, id: OutboxEntryId, failure: &str, next_attempt_at: DateTime<Utc>) -> Result<u64, NotificationError> {
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
        .bind(id.into_inner())
        .bind(failure)
        .bind(next_attempt_at)
        .execute(&self.pool)
        .await
        .map_err(|error| NotificationError::persistence(error.to_string()))?;
        Ok(result.rows_affected())
    }

    async fn mark_terminal(&self, id: OutboxEntryId, failure: &str) -> Result<u64, NotificationError> {
        let result = sqlx::query(
            r#"
            UPDATE notification_outbox
            SET attempt_count = 2147483647,
                delivery_error = $2,
                claimed_at = NULL,
                claimed_by = NULL
            WHERE id = $1
            "#,
        )
        .bind(id.into_inner())
        .bind(failure)
        .execute(&self.pool)
        .await
        .map_err(|error| NotificationError::persistence(error.to_string()))?;
        Ok(result.rows_affected())
    }

    async fn delete_delivered_before(&self, cutoff: DateTime<Utc>) -> Result<u64, NotificationError> {
        let result = sqlx::query("DELETE FROM notification_outbox WHERE delivered_at IS NOT NULL AND delivered_at < $1")
            .bind(cutoff)
            .execute(&self.pool)
            .await
            .map_err(|error| NotificationError::persistence(error.to_string()))?;
        Ok(result.rows_affected())
    }
}

/// Keep a validated outbox row and log an unreadable row without blocking its peers.
fn into_entry(record: Result<OutboxRecord, OutboxRowError>) -> Option<OutboxEntry> {
    match record {
        Ok(record) => Some(record_to_entry(record)),
        Err(error) => {
            error!(error = %error, event = "outbox_row_unreadable");
            None
        }
    }
}

/// Project one validated PostgreSQL record into the notification-owned value.
fn record_to_entry(record: OutboxRecord) -> OutboxEntry {
    OutboxEntry::builder()
        .id(OutboxEntryId::from(record.id))
        .subscription_id(record.subscription_id)
        .entity_id(record.entity_id)
        .trigger(record.trigger)
        .entity_snapshot(record.entity_snapshot)
        .previous_snapshot(record.previous_snapshot)
        .changed_attrs(record.changed_attrs)
        .created_at(record.created_at)
        .attempt_count(record.attempt_count)
        .next_attempt_at(record.next_attempt_at)
        .build()
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
    ///
    pub async fn insert_batch_in_tx(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, entries: &[OutboxInsertRecord<'_>]) -> Result<(), ContextStoreError> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut builder: QueryBuilder<'_, sqlx::Postgres> = QueryBuilder::new(
            "INSERT INTO notification_outbox \
             (subscription_id, entity_id, trigger, entity_snapshot, previous_snapshot, changed_attrs) ",
        );
        builder.push_values(entries, |mut b, entry| {
            let changed_attrs = entry.changed_attrs.iter().map(AttributeName::as_str).collect::<Vec<_>>();
            b.push_bind(entry.subscription_id.as_str())
                .push_bind(entry.entity_id.as_str())
                .push_bind(entry.trigger.to_string())
                .push_bind(entry.entity_snapshot)
                .push_bind(entry.previous_snapshot)
                .push_bind(changed_attrs);
        });
        builder.build().execute(&mut **tx).await?;

        Ok(())
    }

    /// Claim due rows and validate every domain-valued column.
    pub async fn claim_pending(
        &self,
        worker_id: &str,
        batch_size: i32,
        max_attempts: i32,
    ) -> Result<Vec<Result<OutboxRecord, OutboxRowError>>, ContextStoreError> {
        let rows = sqlx::query_as::<_, RawOutboxRow>(
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
                      o.created_at, o.attempt_count, o.next_attempt_at
            "#,
        )
        .bind(worker_id)
        .bind(batch_size)
        .bind(max_attempts)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(OutboxRecord::try_from).collect())
    }
}

/// Typed values for one outbox insert row.
#[derive(Debug)]
pub struct OutboxInsertRecord<'a> {
    /// Subscription identifier.
    pub subscription_id: &'a Uri,
    /// Entity identifier.
    pub entity_id: &'a Uri,
    /// Trigger reason.
    pub trigger: NotificationTrigger,
    /// Entity snapshot JSON.
    pub entity_snapshot: &'a Value,
    /// Previous entity snapshot JSON, if any.
    pub previous_snapshot: &'a Option<Value>,
    /// Changed attribute names.
    pub changed_attrs: &'a [AttributeName],
}
