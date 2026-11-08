//! Direct PostgreSQL implementation of the entity side-effect queue port.

use crate::{
    pending_side_effects::{
        claimed_job::{read_row, stored_column},
        repo::PendingSideEffectsRepository,
    },
    store_fault::persistence_failure,
};
use andromeda_context_commit::{
    failure_summary::{FAILURE_SUMMARY_LIMIT, bounded_failure_summary},
    lease_acknowledgement::LeaseAcknowledgement,
    lease_holder::LeaseHolder,
    side_effect_job_id::SideEffectJobId,
};
use andromeda_entity::{
    error::EntityError,
    side_effect_queue::{LeasedSideEffect, SideEffectQueue},
};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_attribute_instance::name_resolution::names_of;
use andromeda_postgresql_schema::queue::PendingSideEffects;
use andromeda_postgresql_seaquery::{interval::make_interval_secs, queue::is_claimed_from, statement::Statement};
use andromeda_telemetry::statement::{ObservedStatement, observe_statement};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, DeleteStatement, Expr, ExprTrait, Keyword, LockBehavior, LockType, Order, Query, SimpleExpr, UpdateStatement};
use sqlx::postgres::PgRow;
use std::num::NonZeroU32;
use tracing::error;
use uuid::Uuid;

#[async_trait]
impl SideEffectQueue for PendingSideEffectsRepository {
    async fn lease_batch(&self, worker_id: &LeaseHolder, batch_size: NonZeroU32, lease_seconds: i32) -> Result<Vec<LeasedSideEffect>, EntityError> {
        let wanted = usize::try_from(batch_size.get()).map_err(|error| EntityError::from(PersistenceFailure::Unencodable.record(&error)))?;
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
                match self.leased_effect(id, &row).await? {
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
        let result = observe_statement(
            ObservedStatement::PendingSideEffectRelease,
            nack_statement(id.as_uuid(), *owner, error, next_attempt_at).to_query().execute(self.pool()),
        )
        .await
        .map_err(|error| persistence(&error))?;
        Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
    }

    async fn purge_dead_before(&self, cutoff: DateTime<Utc>) -> Result<u64, EntityError> {
        let result = purge_dead_statement(cutoff)
            .to_query()
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
        observe_statement(
            ObservedStatement::PendingSideEffectClaim,
            lease_batch_statement(*owner, limit, f64::from(lease_seconds)).to_query().fetch_all(self.pool()),
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
        let retired = observe_statement(
            ObservedStatement::PendingSideEffectRelease,
            mark_dead_statement(id.as_uuid(), *owner, detail).to_query().execute(self.pool()),
        )
        .await;
        match retired.map(|result| LeaseAcknowledgement::of_guarded_update(result.rows_affected())) {
            Ok(LeaseAcknowledgement::Applied) => {}
            Ok(LeaseAcknowledgement::Lost) => error!(job_id = %id, event = "pending_side_effect_mark_dead_lease_lost"),
            Err(mark_error) => error!(error = %persistence(&mark_error), job_id = %id, event = "pending_side_effect_mark_dead_failed"),
        }
    }
}

impl PendingSideEffectsRepository {
    /// Decode one claimed row into the leased job the entity policy consumes,
    /// or report why it never will.
    ///
    /// The names behind every key the job's events hold are looked up once
    /// per dictionary. A dictionary that cannot be read fails the claim, and
    /// the job's lease lapses for a later one; a key no name answers is an
    /// unreadable job.
    async fn leased_effect(&self, id: SideEffectJobId, row: &PgRow) -> Result<Result<LeasedSideEffect, EntityError>, EntityError> {
        let read = match read_row(id, row) {
            Ok(read) => read,
            Err(unreadable) => return Ok(Err(unreadable)),
        };
        let (attribute_names, entity_type_names) = self.dictionaries();
        let names = names_of(&read.held_keys(), attribute_names, entity_type_names)
            .await
            .map_err(|error| EntityError::from(persistence_failure(&error)))?;
        Ok(read.assemble(&names))
    }
}

/// Map a driver failure into the entity port's persistence category.
#[track_caller]
fn persistence(error: &sqlx::Error) -> EntityError {
    EntityError::from(persistence_failure(error))
}

/// Claim a batch of due jobs for `owner`, returning each claimed row's events.
///
/// One `UPDATE` both selects and claims the batch atomically, leasing exactly
/// the ids its `FOR UPDATE SKIP LOCKED` selection picked ([`is_claimed_from`]).
/// A claim that leased more than the batch would leave the rows the drain does
/// not replay to lapse and be reclaimed, each reclaim counting an attempt, until
/// a job that was never attempted is dead-lettered. The candidate predicate is
/// the queue's claim index: not dead, due, and either unleased or held under an
/// expired lease.
///
/// Reclaiming a row that still carries a lease counts an attempt. Such a row's
/// previous attempt ended without releasing it: the worker crashed, or its
/// lease lapsed before its replay could lock the job. Nothing else
/// recorded it, and without this count the backoff and the stuck report of the
/// job's next failure would leave those attempts out. A row released by a
/// retry holds no lease, and its failed attempt was already counted then.
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
        .value(
            PendingSideEffects::LeaseUntil,
            Expr::current_timestamp().add(make_interval_secs(Expr::val(lease_seconds))),
        )
        .value(PendingSideEffects::ClaimedBy, owner.as_uuid())
        .and_where(is_claimed_from(Expr::col(PendingSideEffects::Id), candidates))
        .returning(Query::returning().columns([
            PendingSideEffects::Id,
            PendingSideEffects::AttemptCount,
            PendingSideEffects::Events,
            PendingSideEffects::WriteId,
            PendingSideEffects::RecordedAt,
            PendingSideEffects::BaseOrdinal,
            PendingSideEffects::CreatedAt,
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
/// backoff and no diagnostic.
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
/// Both claim columns are released together: `chk_pse_claim_coherent` rejects
/// a row that holds one of `lease_until` and `claimed_by` without the other.
fn nack_statement(id: Uuid, owner: LeaseHolder, error: &str, next_attempt_at: DateTime<Utc>) -> UpdateStatement {
    Query::update()
        .table(PendingSideEffects::Table)
        .value(PendingSideEffects::AttemptCount, Expr::col(PendingSideEffects::AttemptCount).add(1))
        .value(
            PendingSideEffects::LastError,
            bounded_failure_summary(error, FAILURE_SUMMARY_LIMIT).into_owned(),
        )
        .value(PendingSideEffects::NextAttemptAt, next_attempt_at)
        .value(PendingSideEffects::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .and_where(claimant_guard(id, owner))
        .to_owned()
}

/// Retire an unreadable job permanently, counting the failed read as its
/// attempt and releasing the lease in the same statement.
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
        .value(PendingSideEffects::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(PendingSideEffects::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .and_where(claimant_guard(id, owner))
        .to_owned()
}

/// Remove jobs dead-lettered before `cutoff`.
///
/// The retention clock is `exhausted_at`, the instant the job was dead-lettered:
/// a job queued for longer than the retention window is still kept for that
/// window once it is retired. `idx_pse_exhausted_at` is the partial index on
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

#[cfg(test)]
mod tests {
    use crate::pending_side_effects::{
        port::{lease_batch_statement, mark_dead_statement, nack_statement, purge_dead_statement},
        repo::PendingSideEffectsRepository,
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_entity::side_effect_queue::SideEffectQueue;
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
    /// whose replay never finishes inside its lease would retry its next
    /// failure as though those attempts never happened.
    #[test]
    fn reclaiming_a_lapsed_lease_counts_the_attempt_that_left_it() {
        let sql = lease_batch_statement(holder(), 1, 30.0).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""attempt_count" = (CASE WHEN ("lease_until" IS NULL) THEN "attempt_count" ELSE "attempt_count" + 1 END)"#),
            "{sql}"
        );
    }

    /// A claim that leased past its width would leave the jobs the drain does
    /// not replay to lapse and be reclaimed, counting an attempt each time,
    /// until jobs never attempted are dead-lettered.
    #[test]
    fn the_claim_leases_only_the_ids_its_limited_selection_picked() {
        let sql = lease_batch_statement(holder(), 1, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"WHERE "id" = ANY(ARRAY (SELECT "id" FROM "pending_side_effects""#), "{sql}");
        assert!(sql.contains("LIMIT 1 FOR UPDATE SKIP LOCKED))"), "{sql}");
        assert!(!sql.contains(" IN (SELECT"), "{sql}");
    }

    /// A replay may outlive its lease and fail afterwards; its release must
    /// still reach the row and record the failure and the backoff, so the
    /// release is guarded by the claimant alone. A job another worker has
    /// reclaimed since names that worker, and the release reaches nothing.
    #[test]
    fn every_release_is_guarded_by_the_claimant_and_not_by_the_clock() {
        for sql in [
            nack_statement(Uuid::nil(), holder(), "boom", Utc::now()).to_string(PostgresQueryBuilder),
            mark_dead_statement(Uuid::nil(), holder(), "boom").to_string(PostgresQueryBuilder),
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
        let sql = mark_dead_statement(Uuid::nil(), holder(), "boom").to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""exhausted_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
    }

    /// The read that failed is counted as the attempt that retires the job.
    #[test]
    fn the_failed_read_that_retires_a_job_is_counted() {
        let sql = mark_dead_statement(Uuid::nil(), holder(), "boom").to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
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
            mark_dead_statement(Uuid::nil(), holder(), &detail),
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
