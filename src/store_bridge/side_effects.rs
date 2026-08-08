//! `SideEffectQueue` trait implementation for `PendingSideEffectsRepository`.

use crate::{
    core::error::AndromedaError,
    event::change_event::{ChangeEvent, ChangeOrigin},
    service::entity::{
        side_effect::BatchSideEffectJob,
        side_effect_queue::{LeasedSideEffect, SideEffectQueue},
    },
};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_postgresql_context_store::pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

#[async_trait]
impl SideEffectQueue for PendingSideEffectsRepository {
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
        .fetch_all(self.pool())
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
                Err(err) => {
                    let detail = err.to_string();
                    error!(error = %detail, job_id = %id, event = "pending_side_effect_payload_unreadable");
                    if let Err(mark_error) = self.mark_dead(id, &detail).await {
                        error!(error = %mark_error, job_id = %id, event = "pending_side_effect_mark_dead_failed");
                    }
                }
            }
        }
        Ok(leased)
    }

    async fn ack(&self, id: Uuid) -> Result<bool, AndromedaError> {
        let result = sqlx::query("DELETE FROM pending_side_effects WHERE id = $1")
            .bind(id)
            .execute(self.pool())
            .await?;
        Ok(result.rows_affected() == 1)
    }

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
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }

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
        .execute(self.pool())
        .await?;
        Ok(result.rows_affected() == 1)
    }
}

fn decode_job(payload: Value) -> Result<BatchSideEffectJob, AndromedaError> {
    let persisted: PersistedBatchSideEffectJob =
        serde_json::from_value(payload).map_err(|err| AndromedaError::internal(format!("invalid pending_side_effect payload: {err}")))?;
    persisted_to_domain(persisted)
}

fn persisted_to_domain(persisted: PersistedBatchSideEffectJob) -> Result<BatchSideEffectJob, AndromedaError> {
    let temporal_entities = persisted
        .temporal_entities
        .into_iter()
        .map(|te| (Arc::from(te.id), Arc::new(te.expanded)))
        .collect();

    let change_events = persisted
        .change_events
        .into_iter()
        .map(|pce| {
            let trigger_reason = pce
                .trigger_reason
                .parse()
                .map_err(|e| AndromedaError::internal(format!("invalid trigger reason '{}': {e}", pce.trigger_reason)))?;
            let entity_id = pce
                .entity_id
                .parse()
                .map_err(|_| AndromedaError::internal(format!("invalid entity URI '{}'", pce.entity_id)))?;
            let entity_types = pce
                .entity_types
                .iter()
                .map(|t| t.parse())
                .collect::<Result<_, _>>()
                .map_err(|e| AndromedaError::internal(format!("invalid entity type: {e}")))?;
            let changed_attributes = pce
                .changed_attributes
                .iter()
                .map(|a| a.parse())
                .collect::<Result<_, _>>()
                .map_err(|e| AndromedaError::internal(format!("invalid attribute name: {e}")))?;
            let origin = match pce.origin.as_str() {
                "local" => ChangeOrigin::Local,
                "forwarded" => ChangeOrigin::Forwarded,
                other => return Err(AndromedaError::internal(format!("unknown change origin '{other}'"))),
            };
            let sys_attrs_snapshot = pce.sys_attrs_snapshot.map(|s| SysAttrsSnapshot {
                created_at: s.created_at,
                modified_at: s.modified_at,
                expires_at: s.expires_at,
            });
            Ok(ChangeEvent::builder()
                .entity_id(entity_id)
                .entity_types(entity_types)
                .trigger_reason(trigger_reason)
                .changed_attributes(changed_attributes)
                .occurred_at(pce.occurred_at)
                .entity_snapshot(pce.entity_snapshot)
                .previous_attributes(pce.previous_attributes)
                .sys_attrs_snapshot(sys_attrs_snapshot)
                .origin(origin)
                .build())
        })
        .collect::<Result<_, AndromedaError>>()?;

    Ok(BatchSideEffectJob {
        temporal_entities,
        change_events,
    })
}
