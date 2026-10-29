//! The claim, high-water, release-probe, lease-guarded removal and retry
//! statements of the temporal append intent queue.
//!
//! The queue lives only in the live branch, in the tenant's default schema, so
//! no statement here names a branch. The statements naming one entity are
//! answered by `idx_temporal_append_intents_entity (entity_id, intent_key)`.

use crate::intent_store::queue_order::{ClaimedIntent, HeadBlock, earlier_intent_blocks};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::queue::TemporalAppendIntents;
use andromeda_postgresql_seaquery::{clock::wall_clock, pg_array::any, queue::is_claimed_from};
use andromeda_temporal::append_intent::TemporalIntentKey;
use chrono::{DateTime, Utc};
use sea_query::{
    CaseStatement,
    Cond,
    Condition,
    DeleteStatement,
    Expr,
    ExprTrait,
    Func,
    Iden,
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

/// Which queued intents one claim may take.
#[derive(Clone, Copy, Debug)]
pub(super) enum ClaimScope<'a> {
    /// Every intent whose retry delay has passed, for the background drain.
    Due,
    /// The intents of one entity keyed at most `through`, whatever their retry
    /// delay, for the mutation barrier.
    EntityThrough {
        /// The entity whose intents are claimed.
        entity_id: &'a Identifier,
        /// The newest key the barrier waits for.
        through: TemporalIntentKey,
    },
}

/// The columns of the release probe's one row.
#[derive(Iden, Clone, Copy)]
pub(super) enum ReleaseProbe {
    /// The entity's intents up to the mark still queued.
    Queued,
    /// Those of them unleased, or whose lease has lapsed.
    Claimable,
    /// The latest lease among them, `NULL` when none is leased.
    LatestLease,
    /// The database clock the leases were compared against.
    DatabaseNow,
}

/// Whether an intent is unleased, or its lease has lapsed by `clock`.
fn unleased_by(clock: SimpleExpr) -> SimpleExpr {
    Expr::col(TemporalAppendIntents::LeaseUntil)
        .is_null()
        .or(Expr::col(TemporalAppendIntents::LeaseUntil).lte(clock))
}

/// Claim an ordered bounded batch of the intents `scope` admits for `owner`.
///
/// One `UPDATE` both selects and claims the batch atomically, leasing exactly
/// the keys its `FOR UPDATE SKIP LOCKED` selection picked
/// ([`is_claimed_from`]), so a claim never counts an attempt on an intent it
/// will not apply. An intent is eligible only
/// when no earlier intent of its Entity blocks it ([`earlier_intent_blocks`]),
/// so the batch holds each Entity's head and the run after it.
///
/// `SKIP LOCKED` lets a claim pass an earlier intent another claim has locked
/// but not yet leased, so the filter avoids churn without ordering anything:
/// the application's own guard refuses an intent claimed that way.
pub(super) fn claim_statement(owner: LeaseHolder, scope: ClaimScope<'_>, limit: u32, lease_until: DateTime<Utc>) -> UpdateStatement {
    let mut eligible = Query::select();
    eligible
        .column(TemporalAppendIntents::IntentKey)
        .from_as(TemporalAppendIntents::Table, ClaimedIntent)
        .and_where(unleased_by(Expr::current_timestamp()));
    let block = match scope {
        ClaimScope::Due => {
            eligible.and_where(Expr::col(TemporalAppendIntents::NextAttemptAt).lte(Expr::current_timestamp()));
            HeadBlock::LeaseOrBackoff
        }
        ClaimScope::EntityThrough { entity_id, through } => {
            eligible
                .and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id.as_str()))
                .and_where(Expr::col(TemporalAppendIntents::IntentKey).lte(through.get()));
            HeadBlock::Lease
        }
    };
    eligible.and_where(earlier_intent_blocks(block).not());
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
        .and_where(is_claimed_from(Expr::col(TemporalAppendIntents::IntentKey), eligible))
        .returning(Query::returning().columns([
            TemporalAppendIntents::IntentKey,
            TemporalAppendIntents::WriteId,
            TemporalAppendIntents::EntityId,
            TemporalAppendIntents::AttemptCount,
        ]))
        .to_owned()
}

/// The newest intent key the queue holds for one entity, leased or not, and
/// `NULL` when it holds none.
pub(super) fn last_intent_statement(entity_id: &Identifier) -> SelectStatement {
    Query::select()
        .expr(Func::max(Expr::col(TemporalAppendIntents::IntentKey)))
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id.as_str()))
        .to_owned()
}

/// One row describing the intents of one entity keyed at most `through`: how
/// many are queued, how many of them are unleased or lapsed, the latest lease
/// among them, and the database clock that lease is compared against.
///
/// The clock is read where the leases are stamped, so the remaining lease is
/// measured on one clock however far the broker's own drifts.
pub(super) fn release_probe_statement(entity_id: &Identifier, through: TemporalIntentKey) -> SelectStatement {
    let claimable_mark: SimpleExpr = CaseStatement::new().case(unleased_by(wall_clock()), Expr::val(1)).into();
    Query::select()
        .expr_as(Func::count(Expr::col(TemporalAppendIntents::IntentKey)), ReleaseProbe::Queued)
        .expr_as(Func::count(claimable_mark), ReleaseProbe::Claimable)
        .expr_as(Func::max(Expr::col(TemporalAppendIntents::LeaseUntil)), ReleaseProbe::LatestLease)
        .expr_as(wall_clock(), ReleaseProbe::DatabaseNow)
        .from(TemporalAppendIntents::Table)
        .and_where(Expr::col(TemporalAppendIntents::EntityId).eq(entity_id.as_str()))
        .and_where(Expr::col(TemporalAppendIntents::IntentKey).lte(through.get()))
        .to_owned()
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

/// Release the leases of the intents named by `intent_keys` that this worker
/// still holds under a live lease, giving back the attempt their claim
/// counted.
///
/// A held-back intent was never attempted, so its retry delay and last
/// failure are left as they were.
pub(super) fn release_statement(intent_keys: &[i64], owner: LeaseHolder) -> UpdateStatement {
    Query::update()
        .table(TemporalAppendIntents::Table)
        .value(TemporalAppendIntents::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(TemporalAppendIntents::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .value(TemporalAppendIntents::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(TemporalAppendIntents::AttemptCount, Expr::col(TemporalAppendIntents::AttemptCount).sub(1))
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
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::intent_store::lease_statements::{
        ClaimScope,
        applied_removal_statement,
        claim_statement,
        last_intent_statement,
        release_probe_statement,
        release_statement,
        retry_statement,
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_temporal::append_intent::TemporalIntentKey;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    fn holder() -> LeaseHolder {
        LeaseHolder::from(Uuid::nil())
    }

    fn vehicle() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("a valid identifier")
    }

    fn key(value: i64) -> TemporalIntentKey {
        TemporalIntentKey::new(value).expect("a positive key")
    }

    #[test]
    fn the_claim_steps_over_locked_rows_and_leases_the_batch() {
        let sql = claim_statement(holder(), ClaimScope::Due, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "temporal_append_intents""#), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
    }

    /// Every leased intent has an attempt counted, so a claim that leased past
    /// its limit would count attempts on intents it never applies.
    #[test]
    fn the_claim_leases_only_the_keys_its_limited_selection_picked() {
        let sql = claim_statement(holder(), ClaimScope::Due, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"WHERE "intent_key" = ANY(ARRAY (SELECT "intent_key" FROM "temporal_append_intents" AS "c""#),
            "{sql}"
        );
        assert!(sql.contains("LIMIT 10 FOR UPDATE SKIP LOCKED))"), "{sql}");
        assert!(!sql.contains(" IN (SELECT"), "{sql}");
    }

    #[test]
    fn the_background_claim_takes_only_intents_whose_retry_delay_has_passed() {
        let sql = claim_statement(holder(), ClaimScope::Due, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""next_attempt_at" <= CURRENT_TIMESTAMP"#), "{sql}");
        assert!(!sql.contains(r#""entity_id" = '"#), "{sql}");
    }

    /// Clauses 4.5.7, 4.5.8 and 4.8 read an Entity's history in commit
    /// order, so the drain claims no intent behind an earlier one of its
    /// Entity that is leased or waiting for its retry delay.
    #[test]
    fn the_background_claim_takes_no_intent_behind_a_leased_or_backing_off_one_of_its_entity() {
        let sql = claim_statement(holder(), ClaimScope::Due, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "temporal_append_intents" AS "c""#), "{sql}");
        assert!(
            sql.contains(concat!(
                r#"NOT EXISTS(SELECT 1 FROM "temporal_append_intents" AS "p" WHERE "p"."entity_id" = "c"."entity_id" "#,
                r#"AND "p"."intent_key" < "c"."intent_key" "#,
                r#"AND ("p"."lease_until" > CURRENT_TIMESTAMP OR "p"."next_attempt_at" > CURRENT_TIMESTAMP))"#
            )),
            "{sql}"
        );
    }

    /// The barrier's client is waiting on its own mutation, so the retry
    /// delay that paces the background drain does not hold the intent back.
    #[test]
    fn the_barrier_claim_takes_one_entitys_intents_up_to_its_mark_whatever_their_retry_delay() {
        let scope = ClaimScope::EntityThrough {
            entity_id: &vehicle(),
            through: key(42),
        };
        let sql = claim_statement(holder(), scope, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(!sql.contains(r#""next_attempt_at""#), "{sql}");
        assert!(sql.contains(r#""entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
        assert!(sql.contains(r#""intent_key" <= 42"#), "{sql}");
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
        assert!(sql.contains(r#""lease_until" IS NULL"#), "{sql}");
        assert!(
            sql.contains(r#"AND "p"."intent_key" < "c"."intent_key" AND "p"."lease_until" > CURRENT_TIMESTAMP)"#),
            "{sql}"
        );
    }

    /// The claim names each intent without reading its entries: the queue
    /// applies them where they wait.
    #[test]
    fn the_claim_returns_the_intent_identity_and_no_entries() {
        let sql = claim_statement(holder(), ClaimScope::Due, 10, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"RETURNING "intent_key", "write_id", "entity_id", "attempt_count""#), "{sql}");
        assert!(!sql.contains("entries"), "{sql}");
    }

    #[test]
    fn the_high_water_probe_reads_the_newest_key_of_one_entity() {
        let sql = last_intent_statement(&vehicle()).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT MAX("intent_key") FROM "temporal_append_intents""#), "{sql}");
        assert!(sql.contains(r#""entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
        assert!(!sql.contains("entries"), "{sql}");
    }

    /// The probe compares the leases with the instant it runs on the clock
    /// that stamped them.
    #[test]
    fn the_release_probe_counts_one_entitys_intents_up_to_its_mark_on_the_database_clock() {
        let sql = release_probe_statement(&vehicle(), key(42)).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"COUNT("intent_key") AS "queued""#), "{sql}");
        assert!(sql.contains(r#"AS "claimable""#), "{sql}");
        assert!(sql.contains(r#"MAX("lease_until") AS "latest_lease""#), "{sql}");
        assert!(sql.contains(r#"CLOCK_TIMESTAMP() AS "database_now""#), "{sql}");
        assert!(sql.contains(r#""lease_until" <= CLOCK_TIMESTAMP()"#), "{sql}");
        assert!(sql.contains(r#""entity_id" = 'urn:ngsi-ld:Vehicle:1'"#), "{sql}");
        assert!(sql.contains(r#""intent_key" <= 42"#), "{sql}");
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

    /// A held-back intent was never attempted: the release gives back the
    /// attempt its claim counted and keeps its retry delay and last failure.
    #[test]
    fn a_release_gives_back_the_lease_and_the_claims_attempt_under_a_live_lease() {
        let sql = release_statement(&[2, 3], holder()).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"UPDATE "temporal_append_intents" SET"#), "{sql}");
        for column in ["claimed_at", "lease_until", "claimed_by"] {
            assert!(sql.contains(&format!(r#""{column}" = NULL"#)), "{column}: {sql}");
        }
        assert!(sql.contains(r#""attempt_count" = "attempt_count" - 1"#), "{sql}");
        assert!(!sql.contains("next_attempt_at") && !sql.contains("last_error"), "{sql}");
        assert!(sql.contains(r#""intent_key" = ANY(ARRAY [2,3])"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" ="#), "{sql}");
        assert!(sql.contains(r#""lease_until" > CLOCK_TIMESTAMP()"#), "{sql}");
    }
}
