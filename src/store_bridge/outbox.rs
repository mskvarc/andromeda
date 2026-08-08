//! `OutboxStore` trait implementation for `OutboxRepository`.

use crate::{
    core::error::AndromedaError,
    service::outbox::{
        entry::{OutboxEntry, OutboxEntryId, WorkerInstanceId},
        store::OutboxStore,
    },
};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::NotificationTrigger;
use andromeda_postgresql_context_store::outbox::{repo::OutboxRepository, rows::OutboxEntryRow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::{error::Error, str::FromStr};
use tracing::error;

#[async_trait]
impl OutboxStore for OutboxRepository {
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
        .fetch_all(self.pool())
        .await?;

        Ok(rows.into_iter().filter_map(into_entry).collect())
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

fn into_entry(row: OutboxEntryRow) -> Option<OutboxEntry> {
    match row_to_domain(row) {
        Ok(entry) => Some(entry),
        Err(err) => {
            error!(error = %err, event = "outbox_row_unreadable");
            None
        }
    }
}

fn row_to_domain(row: OutboxEntryRow) -> Result<OutboxEntry, Box<dyn Error>> {
    let trigger = NotificationTrigger::from_str(&row.trigger)?;
    let subscription_id: Uri = row
        .subscription_id
        .parse()
        .map_err(|_| format!("invalid subscription URI '{}'", row.subscription_id))?;
    let entity_id: Uri = row.entity_id.parse().map_err(|_| format!("invalid entity URI '{}'", row.entity_id))?;
    let changed_attrs = row.changed_attrs.iter().map(|raw| raw.parse()).collect::<Result<Vec<_>, _>>()?;

    Ok(OutboxEntry::builder()
        .id(OutboxEntryId::from(row.id))
        .subscription_id(subscription_id)
        .entity_id(entity_id)
        .trigger(trigger)
        .entity_snapshot(row.entity_snapshot)
        .previous_snapshot(row.previous_snapshot)
        .changed_attrs(changed_attrs)
        .created_at(row.created_at)
        .claimed_at(row.claimed_at)
        .claimed_by(row.claimed_by.map(WorkerInstanceId::from))
        .delivered_at(row.delivered_at)
        .delivery_error(row.delivery_error)
        .attempt_count(row.attempt_count)
        .next_attempt_at(row.next_attempt_at)
        .build())
}
