//! Direct PostgreSQL implementation of the entity side-effect queue port.

use crate::pending_side_effects::{payload::PersistedBatchSideEffectJob, repo::PendingSideEffectsRepository};
use andromeda_context_commit::{
    batch_side_effect_job::BatchSideEffectJob,
    change_event::ChangeEvent,
    failure_summary::{FAILURE_SUMMARY_LIMIT, bounded_failure_summary},
    lease_acknowledgement::LeaseAcknowledgement,
    lease_holder::LeaseHolder,
    side_effect_job_id::SideEffectJobId,
    write_id::WriteId,
    write_ordinal::WriteOrdinal,
    write_record::WriteRecord,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    error::EntityError,
    side_effect_queue::{LeasedSideEffect, SideEffectQueue},
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
    async fn lease_batch(&self, worker_id: &LeaseHolder, batch_size: i32, lease_seconds: i32) -> Result<Vec<LeasedSideEffect>, EntityError> {
        let (sql, values) = lease_batch_statement(*worker_id, batch_size, f64::from(lease_seconds)).build_sqlx(PostgresQueryBuilder);
        let rows = observe_statement(
            SIDE_EFFECT_CLAIM,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(self.pool()),
        )
        .await
        .map_err(|error| persistence(&error))?;

        // The side-effect queue is drained on the live branch: a Snapshot branch
        // captures synchronously and enqueues no deferred jobs, so every job
        // replays into the live branch it was enqueued from.
        let mut leased = Vec::with_capacity(rows.len());
        for row in rows {
            let id = SideEffectJobId::from_uuid(row.get("id"));
            match leased_effect(id, ContextTarget::Live, &row) {
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

    async fn ack(&self, id: SideEffectJobId, owner: &LeaseHolder) -> Result<LeaseAcknowledgement, EntityError> {
        let (sql, values) = ack_statement(id.as_uuid(), *owner).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            SIDE_EFFECT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(self.pool()),
        )
        .await
        .map_err(|error| persistence(&error))?;
        Ok(lease_acknowledgement(result.rows_affected()))
    }

    async fn nack(&self, id: SideEffectJobId, owner: &LeaseHolder, error: &str, next_attempt_at: DateTime<Utc>) -> Result<LeaseAcknowledgement, EntityError> {
        let (sql, values) = nack_statement(id.as_uuid(), *owner, error, next_attempt_at).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            SIDE_EFFECT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(self.pool()),
        )
        .await
        .map_err(|error| persistence(&error))?;
        Ok(lease_acknowledgement(result.rows_affected()))
    }

    async fn mark_dead(&self, id: SideEffectJobId, owner: &LeaseHolder, error: &str) -> Result<LeaseAcknowledgement, EntityError> {
        let (sql, values) = mark_dead_statement(id.as_uuid(), *owner, error).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            SIDE_EFFECT_ACKNOWLEDGE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(self.pool()),
        )
        .await
        .map_err(|error| persistence(&error))?;
        Ok(lease_acknowledgement(result.rows_affected()))
    }

    async fn purge_dead_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EntityError> {
        let (sql, values) = purge_dead_statement(cutoff).build_sqlx(PostgresQueryBuilder);
        let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
            .execute(self.pool())
            .await
            .map_err(|error| persistence(&error))?;
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
    async fn retire_unreadable(&self, id: SideEffectJobId, owner: &LeaseHolder, detail: &str) {
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
fn leased_effect(id: SideEffectJobId, target: ContextTarget, row: &PgRow) -> Result<LeasedSideEffect, EntityError> {
    let payload: Value = row.get("payload");
    let job = decode_job(target, &payload)?;
    let write_id = WriteId::from_uuid(row.get("write_id"));
    let recorded_at: DateTime<Utc> = row.get("recorded_at");
    let record = WriteRecord::restored(write_id, recorded_at);
    let stored_base: i32 = row.get("base_ordinal");
    let base_ordinal =
        WriteOrdinal::try_from(stored_base).map_err(|error| EntityError::internal(format!("pending side-effect consequence base is unreadable: {error}")))?;
    Ok(LeasedSideEffect {
        id,
        attempts: row.get("attempt_count"),
        job,
        record,
        base_ordinal,
    })
}

/// Map a driver failure into the entity port's persistence category.
fn persistence(error: &sqlx::Error) -> EntityError {
    EntityError::persistence(error.to_string())
}

/// Claim a batch of due jobs for `owner`, returning each claimed row's payload.
///
/// The original single data-modifying CTE (`WITH claimable AS (SELECT … FOR
/// UPDATE SKIP LOCKED) UPDATE … RETURNING`) is rebuilt as one `UPDATE` whose
/// `WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED)` both selects and claims the
/// batch atomically. The candidate predicate is the queue's claim index: not
/// dead, due, and either unleased or held under an expired lease.
fn lease_batch_statement(owner: LeaseHolder, batch_size: i32, lease_seconds: f64) -> UpdateStatement {
    let candidates = Query::select()
        .column(PendingSideEffects::Id)
        .from(PendingSideEffects::Table)
        .and_where(Expr::col(PendingSideEffects::ExhaustedAt).is_null())
        .and_where(Expr::col(PendingSideEffects::NextAttemptAt).lte(Expr::current_timestamp()))
        .and_where(
            Expr::col(PendingSideEffects::LeaseUntil)
                .is_null()
                .or(Expr::col(PendingSideEffects::LeaseUntil).lte(Expr::current_timestamp())),
        )
        .order_by(PendingSideEffects::NextAttemptAt, Order::Asc)
        // A negative batch size is an absent bound, not a narrower one, so it
        // clamps to zero rather than wrapping into an enormous unsigned width.
        .limit(u64::try_from(batch_size).unwrap_or(0))
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned();

    Query::update()
        .table(PendingSideEffects::Table)
        .value(PendingSideEffects::ClaimedAt, Expr::current_timestamp())
        .value(
            PendingSideEffects::LeaseUntil,
            Expr::current_timestamp().add(make_interval_secs(Expr::val(lease_seconds))),
        )
        .value(PendingSideEffects::ClaimedBy, owner.as_uuid())
        .and_where(Expr::col(PendingSideEffects::Id).in_subquery(candidates))
        .returning(Query::returning().columns([
            PendingSideEffects::Id,
            PendingSideEffects::AttemptCount,
            PendingSideEffects::Payload,
            PendingSideEffects::WriteId,
            PendingSideEffects::RecordedAt,
            PendingSideEffects::BaseOrdinal,
        ]))
        .to_owned()
}

/// Every acknowledgement is scoped to the row this worker still holds the lease
/// on: the job's own id, the recorded owner, and a lease that has not expired.
fn lease_guard(id: Uuid, owner: LeaseHolder) -> SimpleExpr {
    Expr::col(PendingSideEffects::Id)
        .eq(id)
        .and(Expr::col(PendingSideEffects::ClaimedBy).eq(owner.as_uuid()))
        .and(Expr::col(PendingSideEffects::LeaseUntil).gt(Expr::current_timestamp()))
}

/// Remove the acknowledged job under the lease this worker holds.
fn ack_statement(id: Uuid, owner: LeaseHolder) -> DeleteStatement {
    Query::delete()
        .from_table(PendingSideEffects::Table)
        .and_where(lease_guard(id, owner))
        .to_owned()
}

/// Count the attempt, record its diagnostic, and release the lease for a retry.
///
/// The diagnostic is cut to the bound `chk_pse_last_error_length` states, so the
/// retry never fails on its own bookkeeping.
///
/// All three claim columns are released together: `chk_pse_claim_coherent`
/// rejects a row that holds some but not all of `claimed_at`, `lease_until` and
/// `claimed_by`.
fn nack_statement(id: Uuid, owner: LeaseHolder, error: &str, next_attempt_at: DateTime<Utc>) -> UpdateStatement {
    Query::update()
        .table(PendingSideEffects::Table)
        .value(PendingSideEffects::AttemptCount, Expr::col(PendingSideEffects::AttemptCount).add(1))
        .value(
            PendingSideEffects::LastError,
            bounded_failure_summary(error, FAILURE_SUMMARY_LIMIT).into_owned(),
        )
        .value(PendingSideEffects::NextAttemptAt, next_attempt_at)
        .value(PendingSideEffects::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .and_where(lease_guard(id, owner))
        .to_owned()
}

/// Retire the job permanently, releasing the lease in the same statement.
///
/// An exhausted row holds no lease: `chk_pse_exhausted_state` rejects a row that
/// carries `exhausted_at` together with any claim column, so the lease is
/// released here rather than in a later statement. The diagnostic is cut to the
/// bound `chk_pse_last_error_length` states.
fn mark_dead_statement(id: Uuid, owner: LeaseHolder, error: &str) -> UpdateStatement {
    Query::update()
        .table(PendingSideEffects::Table)
        .value(PendingSideEffects::AttemptCount, Expr::col(PendingSideEffects::AttemptCount).add(1))
        .value(PendingSideEffects::ExhaustedAt, Expr::current_timestamp())
        .value(
            PendingSideEffects::LastError,
            bounded_failure_summary(error, FAILURE_SUMMARY_LIMIT).into_owned(),
        )
        .value(PendingSideEffects::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .and_where(lease_guard(id, owner))
        .to_owned()
}

/// Remove jobs dead-lettered before `cutoff`.
///
/// The retention clock is `exhausted_at`, the instant the job was dead-lettered:
/// a job that retried for longer than the retention window is still kept for
/// that window once it gives up. `idx_pse_exhausted_at` is the partial index on
/// `exhausted_at WHERE exhausted_at IS NOT NULL` this range scan runs on; an
/// exhausted row holds no lease (`chk_pse_exhausted_state`), so no worker can be
/// mid-flight on a row this removes.
fn purge_dead_statement(cutoff: DateTime<Utc>) -> DeleteStatement {
    Query::delete()
        .from_table(PendingSideEffects::Table)
        .and_where(Expr::col(PendingSideEffects::ExhaustedAt).is_not_null())
        .and_where(Expr::col(PendingSideEffects::ExhaustedAt).lt(cutoff))
        .to_owned()
}

/// Decode one persisted queue payload into the semantic side-effect job.
fn decode_job(target: ContextTarget, payload: &Value) -> Result<BatchSideEffectJob, EntityError> {
    let persisted = PersistedBatchSideEffectJob::decode(payload).map_err(|error| EntityError::persistence(error.to_string()))?;
    Ok(persisted_to_domain(target, persisted))
}

/// Move a validated persisted payload into the policy-owned job representation.
fn persisted_to_domain(target: ContextTarget, persisted: PersistedBatchSideEffectJob) -> BatchSideEffectJob {
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
    use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
    use andromeda_entity::side_effect_queue::SideEffectQueue;
    use chrono::Utc;
    use sea_query::{PostgresQueryBuilder, Value};
    use uuid::Uuid;

    fn assert_side_effect_queue<T: SideEffectQueue>() {}

    #[test]
    fn the_lease_claims_a_due_batch_stepping_over_rows_other_workers_hold() {
        let sql = lease_batch_statement(LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "pending_side_effects""#), "{sql}");
        assert!(sql.contains(r#""exhausted_at" IS NULL"#), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains("make_interval(secs =>"), "{sql}");
        assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
    }

    #[test]
    fn every_acknowledgement_is_guarded_by_the_live_lease() {
        for sql in [
            ack_statement(Uuid::nil(), LeaseHolder::from(Uuid::nil())).to_string(PostgresQueryBuilder),
            nack_statement(Uuid::nil(), LeaseHolder::from(Uuid::nil()), "boom", Utc::now()).to_string(PostgresQueryBuilder),
            mark_dead_statement(Uuid::nil(), LeaseHolder::from(Uuid::nil()), "boom").to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
            assert!(sql.contains(r#""lease_until" > CURRENT_TIMESTAMP"#), "{sql}");
        }
    }

    #[test]
    fn a_nack_counts_the_attempt_and_releases_the_claim() {
        let sql = nack_statement(Uuid::nil(), LeaseHolder::from(Uuid::nil()), "boom", Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = NULL"#), "{sql}");
    }

    #[test]
    fn marking_dead_retires_the_row_and_releases_the_lease() {
        let sql = mark_dead_statement(Uuid::nil(), LeaseHolder::from(Uuid::nil()), "boom").to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""exhausted_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
    }

    #[test]
    fn the_dead_letter_reap_removes_only_rows_exhausted_before_the_cutoff() {
        let sql = purge_dead_statement(Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"DELETE FROM "pending_side_effects""#), "{sql}");
        assert!(sql.contains(r#""exhausted_at" IS NOT NULL"#), "{sql}");
        assert!(sql.contains(r#""exhausted_at" <"#), "{sql}");
        assert!(!sql.contains(r#""created_at" <"#), "the reap ages a row by when it was dead-lettered: {sql}");
        assert!(!sql.contains("claimed_by"), "the reap must not be lease-guarded: {sql}");
    }

    #[test]
    fn a_failure_detail_past_the_bound_is_truncated_before_it_is_written() {
        let detail = "\u{e9}".repeat(3000);
        for statement in [
            nack_statement(Uuid::nil(), LeaseHolder::from(Uuid::nil()), &detail, Utc::now()),
            mark_dead_statement(Uuid::nil(), LeaseHolder::from(Uuid::nil()), &detail),
        ] {
            let (_, values) = statement.build(PostgresQueryBuilder);
            let written = values
                .iter()
                .find_map(|value| {
                    if let Value::String(Some(text)) = value {
                        Some(text.chars().count())
                    } else {
                        None
                    }
                })
                .expect("the statement binds the diagnostic");
            pretty_assertions::assert_eq!(written, 2048, "the diagnostic is cut to the column's bound");
        }
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
