//! Direct PostgreSQL implementation of the entity side-effect queue port.

use crate::pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository};
use andromeda_context_commit::{batch_side_effect_job::BatchSideEffectJob, change_event::ChangeEvent};
use andromeda_entity::{
    error::EntityError,
    side_effect_queue::{LeasedSideEffect, SideEffectLeaseOwner, SideEffectQueue},
};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::Row;
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

#[async_trait]
impl SideEffectQueue for PendingSideEffectsRepository {
    async fn lease_batch(&self, worker_id: &SideEffectLeaseOwner, batch_size: i32, lease_seconds: i32) -> Result<Vec<LeasedSideEffect>, EntityError> {
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
        .bind(worker_id.as_ref())
        .bind(batch_size)
        .bind(f64::from(lease_seconds))
        .fetch_all(self.pool())
        .await
        .map_err(persistence)?;

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

    async fn ack(&self, id: Uuid) -> Result<bool, EntityError> {
        let result = sqlx::query("DELETE FROM pending_side_effects WHERE id = $1")
            .bind(id)
            .execute(self.pool())
            .await
            .map_err(persistence)?;
        Ok(result.rows_affected() == 1)
    }

    async fn nack(&self, id: Uuid, error: &str, next_attempt_at: DateTime<Utc>) -> Result<bool, EntityError> {
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
        .await
        .map_err(persistence)?;
        Ok(result.rows_affected() == 1)
    }

    async fn mark_dead(&self, id: Uuid, error: &str) -> Result<bool, EntityError> {
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
        .await
        .map_err(persistence)?;
        Ok(result.rows_affected() == 1)
    }
}

/// Map a driver failure into the entity port's persistence category.
fn persistence(error: sqlx::Error) -> EntityError {
    EntityError::persistence(error.to_string())
}

/// Decode one persisted queue payload into the semantic side-effect job.
fn decode_job(payload: Value) -> Result<BatchSideEffectJob, EntityError> {
    let persisted = PersistedBatchSideEffectJob::decode(payload).map_err(|error| EntityError::persistence(error.to_string()))?;
    Ok(persisted_to_domain(persisted))
}

/// Move a validated persisted payload into the policy-owned job representation.
fn persisted_to_domain(persisted: PersistedBatchSideEffectJob) -> BatchSideEffectJob {
    let temporal_entities = persisted
        .temporal_entities
        .into_iter()
        .map(|temporal| (Arc::new(temporal.id), Arc::new(temporal.expanded)))
        .collect();

    let change_events = persisted
        .change_events
        .into_iter()
        .map(|pce| {
            let sys_attrs_snapshot = pce.sys_attrs_snapshot.map(|s| SysAttrsSnapshot {
                created_at: s.created_at,
                modified_at: s.modified_at,
                expires_at: s.expires_at,
            });
            ChangeEvent::builder()
                .entity_id(pce.entity_id)
                .entity_types(pce.entity_types)
                .trigger_reason(pce.trigger_reason)
                .changed_attributes(pce.changed_attributes)
                .occurred_at(pce.occurred_at)
                .entity_snapshot(pce.entity_snapshot)
                .previous_attributes(pce.previous_attributes)
                .sys_attrs_snapshot(sys_attrs_snapshot)
                .origin(pce.origin)
                .build()
        })
        .collect();

    BatchSideEffectJob {
        temporal_entities,
        change_events,
    }
}

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::repo::PendingSideEffectsRepository;
    use andromeda_entity::side_effect_queue::SideEffectQueue;

    fn assert_side_effect_queue<T: SideEffectQueue>() {}

    #[test]
    fn pending_side_effects_repository_directly_implements_the_entity_queue_port() {
        assert_side_effect_queue::<PendingSideEffectsRepository>();
    }
}
