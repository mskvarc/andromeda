//! `OutboxStore` trait implementation for `OutboxRepository`.

use crate::{
    error::AndromedaError,
    service::outbox::{
        entry::{OutboxEntry, OutboxEntryId},
        store::OutboxStore,
    },
    worker_instance_id::WorkerInstanceId,
};
use andromeda_postgresql_context_store::outbox::{
    repo::OutboxRepository,
    rows::{OutboxRecord, OutboxRowError},
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::error;

#[async_trait]
impl OutboxStore for OutboxRepository {
    async fn claim_pending(&self, worker_id: &WorkerInstanceId, batch_size: i32, max_attempts: i32) -> Result<Vec<OutboxEntry>, AndromedaError> {
        let records = OutboxRepository::claim_pending(self, worker_id.as_ref(), batch_size, max_attempts).await?;
        Ok(records.into_iter().filter_map(into_entry).collect())
    }

    async fn mark_delivered(&self, id: OutboxEntryId) -> Result<u64, AndromedaError> {
        let result = sqlx::query("UPDATE notification_outbox SET delivered_at = NOW() WHERE id = $1")
            .bind(id.into_inner())
            .execute(self.pool())
            .await?;
        Ok(result.rows_affected())
    }

    async fn mark_failed(&self, id: OutboxEntryId, err: &str, next_attempt_at: DateTime<Utc>) -> Result<u64, AndromedaError> {
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
        .bind(err)
        .bind(next_attempt_at)
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected())
    }

    async fn delete_delivered_before(&self, cutoff: DateTime<Utc>) -> Result<u64, AndromedaError> {
        let result = sqlx::query("DELETE FROM notification_outbox WHERE delivered_at IS NOT NULL AND delivered_at < $1")
            .bind(cutoff)
            .execute(self.pool())
            .await?;
        Ok(result.rows_affected())
    }
}

fn into_entry(record: Result<OutboxRecord, OutboxRowError>) -> Option<OutboxEntry> {
    match record {
        Ok(record) => Some(record_to_entry(record)),
        Err(error) => {
            error!(error = %error, event = "outbox_row_unreadable");
            None
        }
    }
}

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
