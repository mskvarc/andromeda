//! The claim, pending-probe, lease-guarded removal and retry statements of the
//! temporal append intent queue.
//!
//! The queue lives only in the live branch, in the tenant's default schema, so
//! no statement here names a branch.

use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
use andromeda_postgresql_seaquery::{clock::wall_clock, pg_array::any};
use chrono::{DateTime, Utc};
use sea_query::{
    Cond,
    Condition,
    DeleteStatement,
    Expr,
    ExprTrait,
    Keyword,
    LockBehavior,
    LockType,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
};
use uuid::Uuid;

/// Claim an ordered bounded batch of eligible intents for `owner`.
///
/// One `UPDATE` whose `WHERE intent_key IN (SELECT … FOR UPDATE SKIP LOCKED)`
/// both selects and claims the batch atomically. When `entity_id` is given,
/// only that entity's intents are eligible; otherwise the predicate is absent.
pub(super) fn claim_statement(owner: LeaseHolder, entity_id: Option<&str>, limit: u32, lease_until: DateTime<Utc>) -> UpdateStatement {
    let mut eligible = Query::select();
    eligible
        .column(TemporalAppendIntents::IntentKey)
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::NextAttemptAt).lte(Expr::current_timestamp()))
        .and_where(
            Expr::col(TemporalAppendIntents::LeaseUntil)
                .is_null()
                .or(Expr::col(TemporalAppendIntents::LeaseUntil).lte(Expr::current_timestamp())),
        );
    if let Some(entity_id) = entity_id {
        eligible.and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id));
    }
    eligible
        .order_by(TemporalAppendIntents::IntentKey, Order::Asc)
        .limit(u64::from(limit))
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked);

    Query::update()
        .table(TemporalAppendIntents::Table)
        .value(TemporalAppendIntents::ClaimedAt, Expr::current_timestamp())
        .value(TemporalAppendIntents::LeaseUntil, lease_until)
        .value(TemporalAppendIntents::ClaimedBy, owner.as_uuid())
        .value(TemporalAppendIntents::AttemptCount, Expr::col(TemporalAppendIntents::AttemptCount).add(1))
        .and_where(Expr::col(TemporalAppendIntents::IntentKey).in_subquery(eligible.clone()))
        .returning(Query::returning().columns([
            TemporalAppendIntents::IntentKey,
            TemporalAppendIntents::WriteId,
            TemporalAppendIntents::EntityId,
            TemporalAppendIntents::AttemptCount,
        ]))
        .to_owned()
}

/// Whether the queue holds an intent for one entity, leased or not; answered
/// by `idx_temporal_append_intents_entity` alone.
pub(super) fn holds_intent_statement(entity_id: &str) -> SelectStatement {
    let probe = Query::select()
        .expr(Expr::val(1))
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id))
        .to_owned();
    Query::select().expr(Expr::exists(probe)).to_owned()
}

/// The guard every lease-scoped statement carries: the row is this worker's
/// and its lease is still live.
///
/// Owner alone is not enough. A lease that lapsed lets another worker claim
/// the row and stamp its own owner, but between the lapse and that claim the
/// old owner still matches; only the live-lease test keeps a late
/// acknowledgement or retry from overwriting what the next worker is doing.
fn lease_guard(intent_key: i64, write_id: Uuid, owner: LeaseHolder) -> Condition {
    Cond::all()
        .add(Expr::col(TemporalAppendIntents::IntentKey).eq(intent_key))
        .add(Expr::col(TemporalAppendIntents::WriteId).eq(write_id))
        .add(Expr::col(TemporalAppendIntents::ClaimedBy).eq(owner.as_uuid()))
        .add(Expr::col(TemporalAppendIntents::LeaseUntil).gt(Expr::current_timestamp()))
}

/// Remove the applied intents named by `intent_keys` that this worker still
/// holds under a live lease, in the transaction that applied them.
///
/// The lease is compared with the clock at the removal, not at the start of
/// the transaction: the application may outlast the lease, and then another
/// worker may already hold the intents, so a removal that reaches fewer rows
/// than it names rolls the application back.
pub(super) fn applied_removal_statement(intent_keys: &[i64], owner: LeaseHolder) -> DeleteStatement {
    Query::delete()
        .from_table(TemporalAppendIntents::Table)
        .cond_where(
            Cond::all()
                .add(Expr::col(TemporalAppendIntents::IntentKey).eq(any(Expr::val(intent_keys.to_vec()))))
                .add(Expr::col(TemporalAppendIntents::ClaimedBy).eq(owner.as_uuid()))
                .add(Expr::col(TemporalAppendIntents::LeaseUntil).gt(wall_clock())),
        )
        .to_owned()
}

/// Reschedule one intent held under this worker's live lease and release the lease.
pub(super) fn retry_statement(next_attempt_at: DateTime<Utc>, last_error: &str, intent_key: i64, write_id: Uuid, owner: LeaseHolder) -> UpdateStatement {
    Query::update()
        .table(TemporalAppendIntents::Table)
        .value(TemporalAppendIntents::NextAttemptAt, next_attempt_at)
        .value(TemporalAppendIntents::LastError, last_error)
        .value(TemporalAppendIntents::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(TemporalAppendIntents::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(TemporalAppendIntents::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .cond_where(lease_guard(intent_key, write_id, owner))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::intent_store::lease_statements::{applied_removal_statement, claim_statement, holds_intent_statement, retry_statement};
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    fn holder() -> LeaseHolder {
        LeaseHolder::from(Uuid::nil())
    }
    #[test]
    fn the_claim_steps_over_locked_rows_and_leases_the_batch() {
        let sql = claim_statement(holder(), None, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "temporal_append_intents""#), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
    }

    #[test]
    fn a_claim_for_one_entity_restricts_the_eligible_set() {
        let sql = claim_statement(holder(), Some("urn:ngsi-ld:Vehicle:1"), 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
    }

    /// The claim names each intent without reading its entries: the queue
    /// applies them where they wait.
    #[test]
    fn the_claim_returns_the_intent_identity_and_no_entries() {
        let sql = claim_statement(holder(), None, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"RETURNING "intent_key", "write_id", "entity_id", "attempt_count""#), "{sql}");
        assert!(!sql.contains("entries"), "{sql}");
    }

    #[test]
    fn the_pending_probe_asks_whether_one_entity_holds_an_intent() {
        let sql = holds_intent_statement("urn:ngsi-ld:Vehicle:1").to_string(PostgresQueryBuilder);
        assert!(sql.starts_with("SELECT EXISTS(SELECT 1 FROM"), "{sql}");
        assert!(sql.contains(r#""entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
        assert!(!sql.contains("entries"), "{sql}");
    }

    /// Owner alone is not the guard: a lapsed lease may already have been
    /// reclaimed, so the removal and the retry also require the lease to be
    /// live, the removal at the instant it runs.
    #[test]
    fn the_application_removes_only_intents_held_under_a_live_lease() {
        let removal = applied_removal_statement(&[1, 2], holder()).to_string(PostgresQueryBuilder);
        assert!(removal.starts_with(r#"DELETE FROM "temporal_append_intents""#), "{removal}");
        assert!(removal.contains(r#""intent_key" = ANY(ARRAY [1,2])"#), "{removal}");
        assert!(removal.contains(r#""claimed_by" ="#), "{removal}");
        assert!(removal.contains(r#""lease_until" > CLOCK_TIMESTAMP()"#), "{removal}");
        let retry = retry_statement(Utc::now(), "boom", 1, Uuid::nil(), holder()).to_string(PostgresQueryBuilder);
        assert!(retry.contains(r#""claimed_by" = NULL"#) && retry.contains(r#""lease_until" = NULL"#), "{retry}");
        assert!(retry.contains(r#""lease_until" > CURRENT_TIMESTAMP"#), "{retry}");
    }
}
