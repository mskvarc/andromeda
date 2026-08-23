//! Direct PostgreSQL implementation of the entity side-effect queue port.

use crate::{
    context_space::resolve_context_target,
    pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository},
};
use andromeda_context_commit::{
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    lease_acknowledgement::LeaseAcknowledgement,
    side_effect_job_id::SideEffectJobId,
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_context_space::context_space_key::ContextSpaceKey;
use andromeda_entity::{
    error::EntityError,
    side_effect_queue::{LeasedSideEffect, SideEffectLeaseOwner, SideEffectQueue},
};
use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
use andromeda_postgresql_schema::queue::PendingSideEffects;
use andromeda_postgresql_seaquery::interval::make_interval_secs;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{DeleteStatement, Expr, ExprTrait, Keyword, LockBehavior, LockType, Order, PostgresQueryBuilder, Query, SimpleExpr, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use serde_json::Value;
use sqlx::{Row, postgres::PgRow};
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

/// The statement name the side-effect lease claim is observed under.
const SIDE_EFFECT_CLAIM: StatementName = StatementName("pending-side-effect-claim");
/// The statement name every side-effect acknowledgement (ack, nack, dead) is observed under.
const SIDE_EFFECT_ACKNOWLEDGE: StatementName = StatementName("pending-side-effect-acknowledge");

#[async_trait]
impl SideEffectQueue for PendingSideEffectsRepository {
    async fn lease_batch(&self, worker_id: &SideEffectLeaseOwner, batch_size: i32, lease_seconds: i32) -> Result<Vec<LeasedSideEffect>, EntityError> {
        let (sql, values) = lease_batch_statement(worker_id.as_ref(), batch_size, f64::from(lease_seconds)).build_sqlx(PostgresQueryBuilder);
        let rows = observe_statement(
            SIDE_EFFECT_CLAIM,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(self.pool()),
        )
        .await
        .map_err(persistence)?;

        let mut leased = Vec::with_capacity(rows.len());
        for row in rows {
            let id = SideEffectJobId::from_uuid(row.get("id"));
            let raw_context_key: i64 = row.get("context_space_key");
            let decoded = match ContextSpaceKey::try_from(raw_context_key) {
                Ok(key) => resolve_context_target(self.pool(), key)
                    .await
                    .map_err(|error| EntityError::persistence(error.to_string())),
                Err(error) => Err(EntityError::persistence(error.to_string())),
            };
            let target = match decoded {
                Ok(target) => target,
                Err(error) => {
                    let detail = error.to_string();
                    error!(error = %detail, job_id = %id, event = "pending_side_effect_target_unreadable");
                    self.retire_unreadable(id, worker_id, &detail).await;
                    continue;
                }
            };
            match leased_effect(id, target, &row) {
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
        let (sql, values) = ack_statement(id.as_uuid(), owner.as_ref()).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            SIDE_EFFECT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(self.pool()),
        )
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
        let (sql, values) = nack_statement(id.as_uuid(), owner.as_ref(), error, next_attempt_at).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            SIDE_EFFECT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(self.pool()),
        )
        .await
        .map_err(persistence)?;
        Ok(lease_acknowledgement(result.rows_affected()))
    }

    async fn mark_dead(&self, id: SideEffectJobId, owner: &SideEffectLeaseOwner, error: &str) -> Result<LeaseAcknowledgement, EntityError> {
        let (sql, values) = mark_dead_statement(id.as_uuid(), owner.as_ref(), error).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            SIDE_EFFECT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(self.pool()),
        )
        .await
        .map_err(persistence)?;
        Ok(lease_acknowledgement(result.rows_affected()))
    }

    async fn purge_dead_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EntityError> {
        let (sql, values) = purge_dead_statement(cutoff).build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(self.pool())
            .await
            .map_err(persistence)?;
        Ok(result.rows_affected())
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
fn leased_effect(id: SideEffectJobId, target: andromeda_context_space::context_target::ContextTarget, row: &PgRow) -> Result<LeasedSideEffect, EntityError> {
    let job = decode_job(target, row.get("payload"))?;
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

/// Claim a batch of due jobs for `owner`, returning each claimed row's payload.
///
/// The original single data-modifying CTE (`WITH claimable AS (SELECT … FOR
/// UPDATE SKIP LOCKED) UPDATE … RETURNING`) is rebuilt as one `UPDATE` whose
/// `WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED)` both selects and claims the
/// batch atomically. The candidate predicate is the queue's claim index: not
/// dead, due, and either unleased or held under an expired lease.
fn lease_batch_statement(owner: &str, batch_size: i32, lease_seconds: f64) -> UpdateStatement {
    let candidates = Query::select()
        .column(PendingSideEffects::Id)
        .from(PendingSideEffects::Table)
        .and_where(Expr::col(PendingSideEffects::Dead).eq(false))
        .and_where(Expr::col(PendingSideEffects::NextAttemptAt).lte(Expr::current_timestamp()))
        .and_where(
            Expr::col(PendingSideEffects::LockedUntil)
                .is_null()
                .or(Expr::col(PendingSideEffects::LockedUntil).lte(Expr::current_timestamp())),
        )
        .order_by(PendingSideEffects::NextAttemptAt, Order::Asc)
        .limit(<i32 as Ord>::max(batch_size, 0) as u64)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned();

    Query::update()
        .table(PendingSideEffects::Table)
        .value(
            PendingSideEffects::LockedUntil,
            Expr::current_timestamp().add(make_interval_secs(Expr::val(lease_seconds))),
        )
        .value(PendingSideEffects::LockedBy, owner)
        .and_where(Expr::col(PendingSideEffects::Id).in_subquery(candidates))
        .returning(Query::returning().columns([
            PendingSideEffects::Id,
            PendingSideEffects::ContextSpaceKey,
            PendingSideEffects::Attempts,
            PendingSideEffects::Payload,
            PendingSideEffects::WriteId,
            PendingSideEffects::RecordedAt,
            PendingSideEffects::ConsequenceBase,
        ]))
        .to_owned()
}

/// Every acknowledgement is scoped to the row this worker still holds the lease
/// on: the job's own id, the recorded owner, and a lease that has not expired.
fn lease_guard(id: Uuid, owner: &str) -> SimpleExpr {
    Expr::col(PendingSideEffects::Id)
        .eq(id)
        .and(Expr::col(PendingSideEffects::LockedBy).eq(owner))
        .and(Expr::col(PendingSideEffects::LockedUntil).gt(Expr::current_timestamp()))
}

/// Remove the acknowledged job under the lease this worker holds.
fn ack_statement(id: Uuid, owner: &str) -> DeleteStatement {
    Query::delete()
        .from_table(PendingSideEffects::Table)
        .and_where(lease_guard(id, owner))
        .to_owned()
}

/// Count the attempt, record its diagnostic, and release the lease for a retry.
///
/// Both lease columns are released together: `chk_pse_lock_coherent` rejects a
/// row that holds one of `locked_until` and `locked_by`.
fn nack_statement(id: Uuid, owner: &str, error: &str, next_attempt_at: DateTime<Utc>) -> UpdateStatement {
    Query::update()
        .table(PendingSideEffects::Table)
        .value(PendingSideEffects::Attempts, Expr::col(PendingSideEffects::Attempts).add(1))
        .value(PendingSideEffects::LastError, error)
        .value(PendingSideEffects::NextAttemptAt, next_attempt_at)
        .value(PendingSideEffects::LockedUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::LockedBy, SimpleExpr::Keyword(Keyword::Null))
        .and_where(lease_guard(id, owner))
        .to_owned()
}

/// Retire the job permanently, releasing the lease in the same statement.
///
/// A dead row holds no lease: `chk_pse_dead_state` rejects `dead = TRUE`
/// together with either lock column, so the lease is released here rather than
/// in a later statement.
fn mark_dead_statement(id: Uuid, owner: &str, error: &str) -> UpdateStatement {
    Query::update()
        .table(PendingSideEffects::Table)
        .value(PendingSideEffects::Attempts, Expr::col(PendingSideEffects::Attempts).add(1))
        .value(PendingSideEffects::Dead, true)
        .value(PendingSideEffects::LastError, error)
        .value(PendingSideEffects::LockedUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::LockedBy, SimpleExpr::Keyword(Keyword::Null))
        .and_where(lease_guard(id, owner))
        .to_owned()
}

/// Remove dead-lettered jobs created before `cutoff`.
///
/// `idx_pse_dead` is the partial index on `created_at WHERE dead` this range
/// scan runs on; a dead row holds no lease (`chk_pse_dead_state`), so no
/// worker can be mid-flight on a row this removes.
fn purge_dead_statement(cutoff: DateTime<Utc>) -> DeleteStatement {
    Query::delete()
        .from_table(PendingSideEffects::Table)
        .and_where(Expr::col(PendingSideEffects::Dead).eq(true))
        .and_where(Expr::col(PendingSideEffects::CreatedAt).lt(cutoff))
        .to_owned()
}

/// Decode one persisted queue payload into the semantic side-effect job.
fn decode_job(target: andromeda_context_space::context_target::ContextTarget, payload: Value) -> Result<BatchSideEffectJob, EntityError> {
    let persisted = PersistedBatchSideEffectJob::decode(payload).map_err(|error| EntityError::persistence(error.to_string()))?;
    Ok(persisted_to_domain(target, persisted))
}

/// Move a validated persisted payload into the policy-owned job representation.
fn persisted_to_domain(target: andromeda_context_space::context_target::ContextTarget, persisted: PersistedBatchSideEffectJob) -> BatchSideEffectJob {
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
                .changed_dataset_id(pce.changed_dataset_id)
                .occurred_at(pce.occurred_at)
                .entity_snapshot(pce.entity_snapshot)
                .previous_attributes(pce.previous_attributes)
                .sys_attrs_snapshot(sys_attrs_snapshot)
                .origin(pce.origin)
                .build()
        })
        .collect();

    BatchSideEffectJob {
        target,
        temporal_entities,
        change_events,
    }
}

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::{
        port::{ack_statement, lease_acknowledgement, lease_batch_statement, mark_dead_statement, nack_statement, purge_dead_statement},
        repo::PendingSideEffectsRepository,
    };
    use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
    use andromeda_entity::side_effect_queue::SideEffectQueue;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    fn assert_side_effect_queue<T: SideEffectQueue>() {}

    #[test]
    fn the_lease_claims_a_due_batch_stepping_over_rows_other_workers_hold() {
        let sql = lease_batch_statement("w", 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "pending_side_effects""#), "{sql}");
        assert!(sql.contains(r#""dead" = FALSE"#), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains("make_interval(secs =>"), "{sql}");
        assert!(sql.contains(r#""locked_by" = 'w'"#), "{sql}");
    }

    #[test]
    fn every_acknowledgement_is_guarded_by_the_live_lease() {
        for sql in [
            ack_statement(Uuid::nil(), "w").to_string(PostgresQueryBuilder),
            nack_statement(Uuid::nil(), "w", "boom", Utc::now()).to_string(PostgresQueryBuilder),
            mark_dead_statement(Uuid::nil(), "w", "boom").to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#""locked_by" = 'w'"#), "{sql}");
            assert!(sql.contains(r#""locked_until" > CURRENT_TIMESTAMP"#), "{sql}");
        }
    }

    #[test]
    fn a_nack_counts_the_attempt_and_releases_both_lease_columns() {
        let sql = nack_statement(Uuid::nil(), "w", "boom", Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""attempts" = "attempts" + 1"#), "{sql}");
        assert!(sql.contains(r#""locked_until" = NULL"#), "{sql}");
        assert!(sql.contains(r#""locked_by" = NULL"#), "{sql}");
    }

    #[test]
    fn marking_dead_retires_the_row_and_releases_the_lease() {
        let sql = mark_dead_statement(Uuid::nil(), "w", "boom").to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""dead" = TRUE"#), "{sql}");
        assert!(sql.contains(r#""locked_until" = NULL"#), "{sql}");
    }

    #[test]
    fn the_dead_letter_reap_removes_only_dead_rows_older_than_the_cutoff() {
        let sql = purge_dead_statement(Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "pending_side_effects""#), "{sql}");
        assert!(sql.contains(r#""dead" = TRUE"#), "{sql}");
        assert!(sql.contains(r#""created_at" <"#), "{sql}");
        assert!(!sql.contains("locked_by"), "the reap must not be lease-guarded: {sql}");
    }

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
