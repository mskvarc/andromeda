//! Direct PostgreSQL implementation of the entity side-effect queue port.

use crate::{
    jsonb_text::JsonbText,
    pending_side_effects::{
        payload::{PersistedBatchSideEffectJob, PersistedChangeEvent},
        repo::PendingSideEffectsRepository,
    },
    store_fault::{persistence_failure, undecodable},
};
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
use andromeda_entity::{
    error::EntityError,
    side_effect_queue::{DeadLetterAttempt, LeasedSideEffect, SideEffectQueue},
};
use andromeda_ngsi_ld_entity::reads::{attribute_change::AttributeChange, attribute_instance_key::AttributeInstanceKey};
use andromeda_ngsi_ld_model::{entity_change::EntityChange, notification::SysAttrsSnapshot};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_schema::queue::PendingSideEffects;
use andromeda_postgresql_seaquery::interval::make_interval_secs;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{
    CaseStatement,
    DeleteStatement,
    Expr,
    ExprTrait,
    Keyword,
    LockBehavior,
    LockType,
    Order,
    PostgresQueryBuilder,
    Query,
    SimpleExpr,
    UpdateStatement,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Row, postgres::PgRow};
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

/// The statement name the side-effect lease claim is observed under.
const SIDE_EFFECT_CLAIM: StatementName = StatementName("pending-side-effect-claim");
/// The statement name every claimant-guarded release of a failed attempt
/// (retry, dead-letter) is observed under.
///
/// A replay that succeeds releases nothing: its commit removes the job as the
/// first statement of the replay transaction.
const SIDE_EFFECT_RELEASE: StatementName = StatementName("pending-side-effect-release");

#[async_trait]
impl SideEffectQueue for PendingSideEffectsRepository {
    async fn lease_batch(&self, worker_id: &LeaseHolder, batch_size: i32, lease_seconds: i32) -> Result<Vec<LeasedSideEffect>, EntityError> {
        // A negative batch size is an absent bound, not a narrower one, so it
        // clamps to zero rather than wrapping into an enormous unsigned width.
        let wanted = usize::try_from(batch_size).unwrap_or(0);
        let mut leased = Vec::with_capacity(wanted);
        // An unreadable row the claim took is retired rather than returned, so
        // the claim is repeated for the places it left: a short result then
        // means the queue ran out of due jobs, never that some were retired.
        // Every repetition retires at least one row for good, so the loop ends.
        while leased.len() < wanted {
            let remaining = wanted - leased.len();
            let claimed = self.claim_rows(worker_id, remaining, lease_seconds).await?;
            let claimed_count = claimed.len();
            for row in claimed {
                let id = SideEffectJobId::from_uuid(stored_column(&row, "id")?);
                match leased_effect(id, &row) {
                    Ok(effect) => leased.push(effect),
                    Err(err) => {
                        let detail = err.to_string();
                        error!(error = %detail, job_id = %id, event = "pending_side_effect_payload_unreadable");
                        self.retire_unreadable(id, worker_id, &detail).await;
                    }
                }
            }
            if claimed_count < remaining {
                break;
            }
        }
        Ok(leased)
    }

    async fn nack(&self, id: SideEffectJobId, owner: &LeaseHolder, error: &str, next_attempt_at: DateTime<Utc>) -> Result<LeaseAcknowledgement, EntityError> {
        let (sql, values) = nack_statement(id.as_uuid(), *owner, error, next_attempt_at).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            SIDE_EFFECT_RELEASE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(self.pool()),
        )
        .await
        .map_err(|error| persistence(&error))?;
        Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
    }

    async fn mark_dead(&self, id: SideEffectJobId, owner: &LeaseHolder, error: &str, attempt: DeadLetterAttempt) -> Result<LeaseAcknowledgement, EntityError> {
        let (sql, values) = mark_dead_statement(id.as_uuid(), *owner, error, attempt).build_sqlx(PostgresQueryBuilder);
        let result = observe_statement(
            SIDE_EFFECT_RELEASE,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(self.pool()),
        )
        .await
        .map_err(|error| persistence(&error))?;
        Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
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
    /// Claim up to `limit` due rows for `owner` in one statement.
    async fn claim_rows(&self, owner: &LeaseHolder, limit: usize, lease_seconds: i32) -> Result<Vec<PgRow>, EntityError> {
        let limit = u64::try_from(limit).map_err(|error| EntityError::from(PersistenceFailure::Unencodable.record(&error)))?;
        let (sql, values) = lease_batch_statement(*owner, limit, f64::from(lease_seconds)).build_sqlx(PostgresQueryBuilder);
        observe_statement(
            SIDE_EFFECT_CLAIM,
            sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(self.pool()),
        )
        .await
        .map_err(|error| persistence(&error))
    }

    /// Retire a leased row whose payload cannot be decoded.
    ///
    /// A payload no binary can read will never succeed, so the row is retired
    /// rather than nacked into an endless redelivery loop, and the failed read
    /// is counted as the attempt that exhausted it. Retirement is also what
    /// releases the lease this worker just took: until it commits, the row is
    /// leased, undecodable and invisible to every other worker, so a failure to
    /// retire is reported rather than passed over.
    async fn retire_unreadable(&self, id: SideEffectJobId, owner: &LeaseHolder, detail: &str) {
        match self.mark_dead(id, owner, detail, DeadLetterAttempt::Failed).await {
            Ok(LeaseAcknowledgement::Applied) => {}
            Ok(LeaseAcknowledgement::Lost) => error!(job_id = %id, event = "pending_side_effect_mark_dead_lease_lost"),
            Err(mark_error) => error!(error = %mark_error, job_id = %id, event = "pending_side_effect_mark_dead_failed"),
        }
    }
}

/// Decode one claimed row into the leased job the entity policy consumes.
fn leased_effect(id: SideEffectJobId, row: &PgRow) -> Result<LeasedSideEffect, EntityError> {
    let payload: JsonbText<'_> = stored_column(row, "payload")?;
    let job = decode_job(payload)?;
    let write_id = WriteId::from_uuid(stored_column(row, "write_id")?);
    let recorded_at: DateTime<Utc> = stored_column(row, "recorded_at")?;
    let record = WriteRecord::restored(write_id, recorded_at);
    let stored_base: i32 = stored_column(row, "base_ordinal")?;
    let base_ordinal = WriteOrdinal::try_from(stored_base).map_err(|error| EntityError::from(undecodable(&error)))?;
    Ok(LeasedSideEffect {
        id,
        attempts: stored_column(row, "attempt_count")?,
        job,
        record,
        base_ordinal,
    })
}

/// Read one column of a claimed row; a column that does not decode is an
/// undecodable record.
fn stored_column<'r, T>(row: &'r PgRow, column: &str) -> Result<T, EntityError>
where
    T: sqlx::Decode<'r, sqlx::Postgres> + sqlx::Type<sqlx::Postgres>,
{
    row.try_get(column).map_err(|error| EntityError::from(undecodable(&error)))
}

/// Map a driver failure into the entity port's persistence category.
#[track_caller]
fn persistence(error: &sqlx::Error) -> EntityError {
    EntityError::from(persistence_failure(error))
}

/// Claim a batch of due jobs for `owner`, returning each claimed row's payload.
///
/// One `UPDATE` whose `WHERE id IN (SELECT … FOR UPDATE SKIP LOCKED)` both selects and claims the
/// batch atomically. The candidate predicate is the queue's claim index: not
/// dead, due, and either unleased or held under an expired lease.
///
/// Reclaiming a row that still carries a lease counts an attempt. Such a row's
/// previous attempt ended without releasing it -- the worker crashed, or its
/// lease lapsed before its replay could lock the job -- so nothing else
/// recorded it, and without this count a job that never finishes inside its
/// lease would be reclaimed forever under the attempt ceiling. A row released
/// by a retry holds no lease, and its failed attempt was already counted then.
/// `attempt_count` is in no index, so the count keeps the claim heap-only.
fn lease_batch_statement(owner: LeaseHolder, limit: u64, lease_seconds: f64) -> UpdateStatement {
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
        .limit(limit)
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned();
    let counted_attempts: SimpleExpr = CaseStatement::new()
        .case(Expr::col(PendingSideEffects::LeaseUntil).is_null(), Expr::col(PendingSideEffects::AttemptCount))
        .finally(Expr::col(PendingSideEffects::AttemptCount).add(1))
        .into();

    Query::update()
        .table(PendingSideEffects::Table)
        .value(PendingSideEffects::AttemptCount, counted_attempts)
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

/// Every release of a failed attempt is scoped to the row this worker still
/// claims: the job's own id and the recorded claimant.
///
/// The lease is deliberately not compared with the clock. A replay removes its
/// job under a live lease as the first statement of its transaction and holds
/// it locked from then on, so it may run past `lease_until`; if it then fails,
/// its rollback restores the row with the lapsed lease still on it, and the
/// release must still record the failure and the backoff. A time guard would
/// find nothing, drop both, and leave the row to be reclaimed at once with no
/// backoff until the attempt ceiling dead-lettered it with no diagnostic.
///
/// The claimant alone is enough because of three invariants the drain keeps:
/// every claim rewrites `claimed_by`, so a job another worker reclaimed after
/// the lease lapsed no longer names this one and the release reaches nothing;
/// every drain mints a claimant identity of its own, so no other worker ever
/// claims under this one; and a drain runs one pass at a time and releases a
/// job before its pass ends, so this claimant never reclaims a job it has yet
/// to release.
fn claimant_guard(id: Uuid, owner: LeaseHolder) -> SimpleExpr {
    Expr::col(PendingSideEffects::Id)
        .eq(id)
        .and(Expr::col(PendingSideEffects::ClaimedBy).eq(owner.as_uuid()))
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
        .and_where(claimant_guard(id, owner))
        .to_owned()
}

/// Retire the job permanently, releasing the lease in the same statement.
///
/// An exhausted row holds no lease: `chk_pse_exhausted_state` rejects a row that
/// carries `exhausted_at` together with any claim column, so the lease is
/// released here rather than in a later statement. The diagnostic is cut to the
/// bound `chk_pse_last_error_length` states. The attempt count grows only by an
/// attempt no statement counted yet, so a dead letter keeps the exact number of
/// attempts the job received.
fn mark_dead_statement(id: Uuid, owner: LeaseHolder, error: &str, attempt: DeadLetterAttempt) -> UpdateStatement {
    let mut statement = Query::update();
    match attempt {
        DeadLetterAttempt::Failed => {
            statement.value(PendingSideEffects::AttemptCount, Expr::col(PendingSideEffects::AttemptCount).add(1));
        }
        DeadLetterAttempt::AlreadyCounted => {}
    }
    statement
        .table(PendingSideEffects::Table)
        .value(PendingSideEffects::ExhaustedAt, Expr::current_timestamp())
        .value(
            PendingSideEffects::LastError,
            bounded_failure_summary(error, FAILURE_SUMMARY_LIMIT).into_owned(),
        )
        .value(PendingSideEffects::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .and_where(claimant_guard(id, owner))
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
fn decode_job(payload: JsonbText<'_>) -> Result<BatchSideEffectJob, EntityError> {
    let persisted = PersistedBatchSideEffectJob::decode(payload.as_bytes()).map_err(|error| EntityError::from(undecodable(&error)))?;
    Ok(persisted_to_domain(persisted))
}

/// Move a validated persisted payload into the policy-owned job representation.
fn persisted_to_domain(persisted: PersistedBatchSideEffectJob) -> BatchSideEffectJob {
    let temporal_entities = persisted
        .temporal_entities
        .into_iter()
        .map(|temporal| (Arc::new(temporal.id), temporal.expanded))
        .collect();
    let change_events = persisted.change_events.into_iter().map(domain_change_event).collect();
    BatchSideEffectJob {
        temporal_entities,
        change_events,
    }
}

/// Move one persisted change event into the change it records.
fn domain_change_event(persisted: PersistedChangeEvent) -> ChangeEvent {
    let sys_attrs_snapshot = persisted.sys_attrs_snapshot.map(|s| SysAttrsSnapshot {
        created_at: s.created_at,
        modified_at: s.modified_at,
        expires_at: s.expires_at,
    });
    let instances = persisted
        .instances
        .into_iter()
        .map(|instance| AttributeChange {
            key: AttributeInstanceKey {
                attr: instance.attribute,
                dataset_id: instance.dataset_id,
            },
            kind: instance.kind,
        })
        .collect();
    ChangeEvent::builder()
        .entity_id(persisted.entity_id)
        .entity_types(persisted.entity_types)
        .change(EntityChange::new(persisted.lifecycle, instances))
        .occurred_at(persisted.occurred_at)
        .entity_snapshot(persisted.entity_snapshot)
        .previous_attributes(persisted.previous_attributes)
        .sys_attrs_snapshot(sys_attrs_snapshot)
        .origin(persisted.origin)
        .current_payload(persisted.current_payload)
        .build()
}

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::{
        port::{lease_batch_statement, mark_dead_statement, nack_statement, purge_dead_statement},
        repo::PendingSideEffectsRepository,
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_entity::side_effect_queue::{DeadLetterAttempt, SideEffectQueue};
    use chrono::Utc;
    use sea_query::{PostgresQueryBuilder, Value};
    use uuid::Uuid;

    fn assert_side_effect_queue<T: SideEffectQueue>() {}

    fn holder() -> LeaseHolder {
        LeaseHolder::from(Uuid::nil())
    }

    #[test]
    fn the_lease_claims_a_due_batch_stepping_over_rows_other_workers_hold() {
        let sql = lease_batch_statement(holder(), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "pending_side_effects""#), "{sql}");
        assert!(sql.contains(r#""exhausted_at" IS NULL"#), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains("LIMIT 10"), "{sql}");
        assert!(sql.contains("make_interval(secs =>"), "{sql}");
        assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
    }

    /// A row reclaimed while it still carries a lease was left by an attempt
    /// that neither settled nor released it, so the claim counts that attempt;
    /// a row a retry released holds no lease and its attempt was counted by the
    /// retry, so it is claimed without a second count. Without the first, a job
    /// whose replay never finishes inside its lease is reclaimed forever under
    /// the attempt ceiling.
    #[test]
    fn reclaiming_a_lapsed_lease_counts_the_attempt_that_left_it() {
        let sql = lease_batch_statement(holder(), 1, 30.0).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""attempt_count" = (CASE WHEN ("lease_until" IS NULL) THEN "attempt_count" ELSE "attempt_count" + 1 END)"#),
            "{sql}"
        );
    }

    #[test]
    fn the_claim_width_is_the_limit_it_was_asked_for() {
        let sql = lease_batch_statement(holder(), 1, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains("LIMIT 1 FOR UPDATE SKIP LOCKED"), "{sql}");
    }

    /// A replay may outlive its lease and fail afterwards; its release must
    /// still reach the row and record the failure and the backoff, so the
    /// release is guarded by the claimant alone. A job another worker has
    /// reclaimed since names that worker, and the release reaches nothing.
    #[test]
    fn every_release_is_guarded_by_the_claimant_and_not_by_the_clock() {
        for sql in [
            nack_statement(Uuid::nil(), holder(), "boom", Utc::now()).to_string(PostgresQueryBuilder),
            mark_dead_statement(Uuid::nil(), holder(), "boom", DeadLetterAttempt::Failed).to_string(PostgresQueryBuilder),
            mark_dead_statement(Uuid::nil(), holder(), "boom", DeadLetterAttempt::AlreadyCounted).to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#""id" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
            assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
            assert!(!sql.contains("CLOCK_TIMESTAMP()"), "a release after a lapsed lease must still land: {sql}");
            assert!(!sql.contains(r#""lease_until" >"#), "a release after a lapsed lease must still land: {sql}");
        }
    }

    #[test]
    fn a_nack_counts_the_attempt_and_releases_the_claim() {
        let sql = nack_statement(Uuid::nil(), holder(), "boom", Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = NULL"#), "{sql}");
    }

    #[test]
    fn marking_dead_retires_the_row_and_releases_the_lease() {
        let sql = mark_dead_statement(Uuid::nil(), holder(), "boom", DeadLetterAttempt::Failed).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""exhausted_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
    }

    /// The attempt that failed is counted as it dead-letters the job.
    #[test]
    fn a_failed_attempt_that_dead_letters_a_job_is_counted() {
        let sql = mark_dead_statement(Uuid::nil(), holder(), "boom", DeadLetterAttempt::Failed).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
    }

    /// A job dead-lettered at its ceiling without a new attempt keeps the exact
    /// count its retries and reclaims recorded.
    #[test]
    fn dead_lettering_already_counted_attempts_leaves_the_count_exact() {
        let sql = mark_dead_statement(Uuid::nil(), holder(), "boom", DeadLetterAttempt::AlreadyCounted).to_string(PostgresQueryBuilder);
        assert!(!sql.contains("attempt_count"), "{sql}");
        assert!(sql.contains(r#""exhausted_at" = CURRENT_TIMESTAMP"#), "{sql}");
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
            nack_statement(Uuid::nil(), holder(), &detail, Utc::now()),
            mark_dead_statement(Uuid::nil(), holder(), &detail, DeadLetterAttempt::Failed),
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
}
