//! Direct PostgreSQL implementation of the entity side-effect queue port.

use crate::pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository};
use andromeda_context_commit::{
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    lease_acknowledgement::LeaseAcknowledgement,
    side_effect_job_id::SideEffectJobId,
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_entity::{
    error::EntityError,
    side_effect_queue::{LeasedSideEffect, SideEffectLeaseOwner, SideEffectQueue},
};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use std::sync::Arc;
use tracing::error;

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
            RETURNING p.id, p.attempts, p.payload, p.write_id, p.recorded_at, p.consequence_base
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
            let id = SideEffectJobId::from_uuid(row.get("id"));
            match leased_effect(id, &row) {
                Ok(effect) => leased.push(effect),
                Err(err) => {
                    let detail = err.to_string();
                    error!(error = %detail, job_id = %id, event = "pending_side_effect_payload_unreadable");
                    self.retire_unreadable(id, worker_id, &detail).await;
                }
            }
        }
        Ok(leased)
    }

    async fn ack(&self, id: SideEffectJobId, owner: &SideEffectLeaseOwner) -> Result<LeaseAcknowledgement, EntityError> {
        let result = sqlx::query("DELETE FROM pending_side_effects WHERE id = $1 AND locked_by = $2 AND locked_until > NOW()")
            .bind(id.as_uuid())
            .bind(owner.as_ref())
            .execute(self.pool())
            .await
            .map_err(persistence)?;
        Ok(lease_acknowledgement(result.rows_affected()))
    }

    async fn nack(
        &self,
        id: SideEffectJobId,
        owner: &SideEffectLeaseOwner,
        error: &str,
        next_attempt_at: DateTime<Utc>,
    ) -> Result<LeaseAcknowledgement, EntityError> {
        // Both lease columns are released together: `chk_pse_lock_coherent`
        // rejects a row that holds one of `locked_until` and `locked_by`.
        let result = sqlx::query(
            r#"
            UPDATE pending_side_effects
            SET attempts = attempts + 1,
                last_error = $3,
                next_attempt_at = $4,
                locked_until = NULL,
                locked_by = NULL
            WHERE id = $1 AND locked_by = $2 AND locked_until > NOW()
            "#,
        )
        .bind(id.as_uuid())
        .bind(owner.as_ref())
        .bind(error)
        .bind(next_attempt_at)
        .execute(self.pool())
        .await
        .map_err(persistence)?;
        Ok(lease_acknowledgement(result.rows_affected()))
    }

    async fn mark_dead(&self, id: SideEffectJobId, owner: &SideEffectLeaseOwner, error: &str) -> Result<LeaseAcknowledgement, EntityError> {
        // A dead row holds no lease: `chk_pse_dead_state` rejects `dead = TRUE`
        // together with either lock column, so the lease is released in the same
        // statement that retires the row rather than in a later one.
        let result = sqlx::query(
            r#"
            UPDATE pending_side_effects
            SET attempts = attempts + 1,
                dead = TRUE,
                last_error = $3,
                locked_until = NULL,
                locked_by = NULL
            WHERE id = $1 AND locked_by = $2 AND locked_until > NOW()
            "#,
        )
        .bind(id.as_uuid())
        .bind(owner.as_ref())
        .bind(error)
        .execute(self.pool())
        .await
        .map_err(persistence)?;
        Ok(lease_acknowledgement(result.rows_affected()))
    }
}

impl PendingSideEffectsRepository {
    /// Retire a leased row whose payload cannot be decoded.
    ///
    /// A payload no binary can read will never succeed, so the row is retired
    /// rather than nacked into an endless redelivery loop. Retirement is also
    /// what releases the lease this worker just took: until it commits, the row
    /// is leased, undecodable and invisible to every other worker, so a failure
    /// to retire is reported rather than passed over.
    async fn retire_unreadable(&self, id: SideEffectJobId, owner: &SideEffectLeaseOwner, detail: &str) {
        match self.mark_dead(id, owner, detail).await {
            Ok(LeaseAcknowledgement::Applied) => {}
            Ok(LeaseAcknowledgement::Lost) => error!(job_id = %id, event = "pending_side_effect_mark_dead_lease_lost"),
            Err(mark_error) => error!(error = %mark_error, job_id = %id, event = "pending_side_effect_mark_dead_failed"),
        }
    }
}

/// Decide whether a lease-guarded update reached the row this worker owned.
///
/// The guarded predicate matches at most one row, so a zero count means the
/// worker's lease had expired and another worker holds the job.
const fn lease_acknowledgement(rows_affected: u64) -> LeaseAcknowledgement {
    if rows_affected == 0 {
        LeaseAcknowledgement::Lost
    } else {
        LeaseAcknowledgement::Applied
    }
}

/// Decode one claimed row into the leased job the entity policy consumes.
fn leased_effect(id: SideEffectJobId, row: &PgRow) -> Result<LeasedSideEffect, EntityError> {
    let job = decode_job(row.get("payload"))?;
    let write_id = WriteId::from_uuid(row.get("write_id"));
    let recorded_at: DateTime<Utc> = row.get("recorded_at");
    let record = WriteRecord::restored(write_id, recorded_at);
    let stored_base: i32 = row.get("consequence_base");
    let consequence_base =
        WriteOrdinal::try_from(stored_base).map_err(|error| EntityError::internal(format!("pending side-effect consequence base is unreadable: {error}")))?;
    Ok(LeasedSideEffect {
        id,
        attempts: row.get("attempts"),
        job,
        record,
        consequence_base,
    })
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
    use crate::pending_side_effects::{port::lease_acknowledgement, repo::PendingSideEffectsRepository};
    use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
    use andromeda_entity::side_effect_queue::SideEffectQueue;

    fn assert_side_effect_queue<T: SideEffectQueue>() {}

    #[test]
    fn pending_side_effects_repository_directly_implements_the_entity_queue_port() {
        assert_side_effect_queue::<PendingSideEffectsRepository>();
    }

    /// A lease-guarded statement that reaches zero rows found either no such
    /// job or a job another worker's lease now owns; either way this worker's
    /// attempt did not land and must be reported as lost rather than applied.
    #[test]
    fn a_guarded_update_that_reaches_no_row_reports_the_lease_as_lost() {
        pretty_assertions::assert_eq!(lease_acknowledgement(0), LeaseAcknowledgement::Lost);
    }

    #[test]
    fn a_guarded_update_that_reaches_its_row_reports_the_lease_as_held() {
        pretty_assertions::assert_eq!(lease_acknowledgement(1), LeaseAcknowledgement::Applied);
    }
}
