//! The claim, pending-read and lease-guarded acknowledgement statements of the
//! temporal append intent queue.
//!
//! The queue lives only in the live branch, in the tenant's default schema, so
//! no statement here names a branch.

use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
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
/// The original data-modifying CTE (`WITH eligible AS (SELECT … FOR UPDATE SKIP
/// LOCKED) UPDATE … RETURNING`) is rebuilt as one `UPDATE` whose `WHERE intent_key
/// IN (SELECT … FOR UPDATE SKIP LOCKED)` both selects and claims the batch
/// atomically. When `entity_id` is given, only that entity's intents are
/// eligible; the original expressed this with a `$4 IS NULL OR entity_id = $4`
/// guard, which is here simply an absent predicate.
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
            TemporalAppendIntents::Payload,
            TemporalAppendIntents::AttemptCount,
        ]))
        .to_owned()
}

/// Read pending intents in key order, optionally for one entity.
pub(super) fn pending_statement(entity_id: Option<&str>) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .column(TemporalAppendIntents::IntentKey)
        .column(TemporalAppendIntents::Payload)
        .from(TemporalAppendIntents::Table);
    if let Some(entity_id) = entity_id {
        statement.and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id));
    }
    statement.order_by(TemporalAppendIntents::IntentKey, Order::Asc).to_owned()
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

/// Remove one acknowledged intent held under this worker's live lease.
pub(super) fn acknowledge_statement(intent_key: i64, write_id: Uuid, owner: LeaseHolder) -> DeleteStatement {
    Query::delete()
        .from_table(TemporalAppendIntents::Table)
        .cond_where(lease_guard(intent_key, write_id, owner))
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
    use crate::intent_store::lease_statements::{acknowledge_statement, claim_statement, pending_statement, retry_statement};
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

    #[test]
    fn pending_reads_are_ordered_by_intent_key() {
        let all = pending_statement(None).to_string(PostgresQueryBuilder);
        assert!(all.contains(r#"ORDER BY "intent_key""#), "{all}");
        let one = pending_statement(Some("urn:ngsi-ld:Vehicle:1")).to_string(PostgresQueryBuilder);
        assert!(one.contains(r#""entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{one}");
    }

    /// Owner alone is not the guard: a lapsed lease may already have been
    /// reclaimed, so both statements also require the lease to be live.
    #[test]
    fn acknowledgement_and_retry_are_guarded_by_lease_ownership_and_a_live_lease() {
        let ack = acknowledge_statement(1, Uuid::nil(), holder()).to_string(PostgresQueryBuilder);
        assert!(
            ack.contains(r#"DELETE FROM "temporal_append_intents""#) && ack.contains(r#""claimed_by" ="#),
            "{ack}"
        );
        assert!(ack.contains(r#""lease_until" > CURRENT_TIMESTAMP"#), "{ack}");
        let retry = retry_statement(Utc::now(), "boom", 1, Uuid::nil(), holder()).to_string(PostgresQueryBuilder);
        assert!(retry.contains(r#""claimed_by" = NULL"#) && retry.contains(r#""lease_until" = NULL"#), "{retry}");
        assert!(retry.contains(r#""lease_until" > CURRENT_TIMESTAMP"#), "{retry}");
    }
}
