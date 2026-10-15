//! Leasing due deliveries, then reading back the events they answer.
//!
//! A delivery lives in the queue of the branch whose subscription matched the
//! change, so a claim sweeps every branch: the live branch, then each Snapshot
//! branch under `snap_<snapshot_key>`. The branch is chosen by qualifying the
//! statement's tables with its schema, and the events, deliveries and
//! subscriptions a claim reads are all in that one branch -- there is no
//! cross-branch join to recover which Snapshot a delivery belongs to, because
//! the branch being drained already names it.
//!
//! A Snapshot branch is claimed only while its control record is key-share
//! locked. Deleting, expiring or evicting a Snapshot locks that record `FOR
//! UPDATE` before `DROP SCHEMA ... CASCADE` takes each branch relation in its
//! own order; a claim that reached the branch relations without the record
//! would hold some of them while the drop held others, a cycle PostgreSQL
//! breaks by aborting one side. Taking the record first orders the two
//! instead: a claim waits out a drop in flight and then finds the record gone,
//! and a drop waits out a claim in flight.

use crate::{
    error::ContextStoreError,
    notification_delivery::{
        branches::{drainable_targets, is_reaped_branch},
        claimed_event_read::{ClaimDictionaries, ClaimedEventRead},
        rows::{RawClaimedDeliveryRow, UnreadableClaimedDelivery},
    },
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::{context_target::ContextTarget, snapshot_target::SnapshotTarget};
use andromeda_notification::{delivery_entry::DeliveryEntry, delivery_lease::LeaseTerm, held_events::HeldEvents};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{notification::NotificationDeliveries, snapshot::Snapshots, subscriptions::Subscriptions};
use andromeda_postgresql_seaquery::{interval::make_interval_secs, queue::claimable_at};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{Expr, ExprTrait, Iden, LockBehavior, LockType, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Postgres, Transaction, postgres::PgPool};

/// The statement name the delivery claim is observed under.
const DELIVERY_CLAIM: StatementName = StatementName("notification-delivery-claim");

/// Claim up to `batch_size` due deliveries for `worker_id` under `lease`,
/// sweeping the live branch and every Snapshot branch until the batch is filled.
///
/// An event the claiming pass already holds in `held` is handed over without
/// being read, and every event the claim reads is added to `held`.
///
/// Each branch is drained in turn against the remaining budget. A branch reaped
/// between the enumeration and its claim (a Snapshot deleted concurrently) shows
/// up as a missing schema and is skipped -- its notifications are moot once the
/// branch is gone.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails for a reason
/// other than a reaped branch, [`ContextStoreError::NameLookup`] when a
/// dictionary cannot be read, or [`ContextStoreError::StoredSnapshot`] when the
/// branch enumeration cannot read back a stored Snapshot identity. A single
/// unreadable row is reported inside the returned vector, still leased and
/// naming its branch and key, rather than as a failure of the claim.
pub async fn claim_due(
    pool: &PgPool,
    dictionaries: ClaimDictionaries<'_>,
    worker_id: LeaseHolder,
    lease: LeaseTerm,
    batch_size: i32,
    held: &HeldEvents,
) -> Result<Vec<Result<DeliveryEntry, Box<UnreadableClaimedDelivery>>>, ContextStoreError> {
    let mut results = Vec::new();
    let mut remaining = batch_size;
    for target in drainable_targets(pool).await? {
        if remaining <= 0 {
            break;
        }
        match claim_branch(pool, dictionaries, &target, worker_id, lease, remaining, held).await {
            Ok(claimed) => {
                remaining -= i32::try_from(claimed.len()).unwrap_or(remaining);
                results.extend(claimed);
            }
            Err(error) if is_reaped_branch(&error) => {}
            Err(error) => return Err(error),
        }
    }
    Ok(results)
}

/// Claim and read back up to `batch_size` due deliveries from one branch,
/// reading only the events `held` does not already hold.
async fn claim_branch(
    pool: &PgPool,
    dictionaries: ClaimDictionaries<'_>,
    target: &ContextTarget,
    worker_id: LeaseHolder,
    lease: LeaseTerm,
    batch_size: i32,
    held: &HeldEvents,
) -> Result<Vec<Result<DeliveryEntry, Box<UnreadableClaimedDelivery>>>, ContextStoreError> {
    let mut tx = pool.begin().await?;
    if let Some(snapshot) = target.snapshot()
        && hold_snapshot_record(&mut tx, snapshot).await? == SnapshotRecord::Reaped
    {
        tx.commit().await?;
        return Ok(Vec::new());
    }

    let (sql, values) = claim_statement(target, worker_id, batch_size, f64::from(lease.seconds())).build_sqlx(PostgresQueryBuilder);
    let claim = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut *tx);
    let deliveries: Vec<RawClaimedDeliveryRow> = observe_statement(DELIVERY_CLAIM, claim).await?;
    if deliveries.is_empty() {
        tx.commit().await?;
        return Ok(Vec::new());
    }

    let read = ClaimedEventRead::read(&mut tx, target, deliveries.iter().map(|delivery| delivery.event_key), held).await?;

    tx.commit().await?;
    let events = read.assemble(dictionaries, held, target).await?;
    Ok(deliveries.into_iter().map(|delivery| delivery.into_entry(&events, target)).collect())
}

/// Whether a Snapshot's control record still existed when a claim locked it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SnapshotRecord {
    /// The record exists and is key-share locked until the claim commits, so
    /// its branch cannot be dropped under the claim.
    Held,
    /// The Snapshot was deleted, expired or evicted; its branch is gone or
    /// about to be, and its deliveries are moot.
    Reaped,
}

/// Key-share lock the control record of `snapshot` inside the claim's
/// transaction, before any relation of its branch is touched.
///
/// `FOR KEY SHARE` conflicts with the `FOR UPDATE` every reaping path takes on
/// the record, and with nothing else: status transitions update non-key
/// columns and proceed alongside a claim.
async fn hold_snapshot_record(tx: &mut Transaction<'_, Postgres>, snapshot: &SnapshotTarget) -> Result<SnapshotRecord, ContextStoreError> {
    let (sql, values) = snapshot_record_lock_statement(snapshot).build_sqlx(PostgresQueryBuilder);
    let held: Option<i64> = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(if held.is_some() { SnapshotRecord::Held } else { SnapshotRecord::Reaped })
}

/// `SELECT snapshot_key FROM snapshots WHERE snapshot_key = $1 FOR KEY SHARE`.
fn snapshot_record_lock_statement(snapshot: &SnapshotTarget) -> SelectStatement {
    Query::select()
        .column(Snapshots::SnapshotKey)
        .from(Snapshots::Table)
        .and_where(Expr::col(Snapshots::SnapshotKey).eq(snapshot.key().as_i64()))
        .lock(LockType::KeyShare)
        .to_owned()
}

/// Claim a batch of due deliveries in `target`'s branch, returning each with
/// the subscription it answers.
///
/// The claimed rows are read back by the claim's own `RETURNING`: they carry a
/// live lease held by this transaction, so no other worker can alter them
/// before they are delivered, and joining their subscription in the claim's
/// `FROM` saves a statement per claim. Every delivery has its subscription:
/// the foreign key deletes a subscription's deliveries with it.
///
/// The candidate predicate is exactly the one the queue's partial claim index
/// carries: still queued (neither delivered nor exhausted) and claimable — the
/// later of its due instant and its lease expiry has passed. That instant is
/// the index's expression, spelled through the one shared helper so the
/// statement and the index match structurally; ordering by it, then by `delivery_key`,
/// is the index's own key order, so the claim is one bounded range scan whose
/// depth is the batch size. `FOR UPDATE SKIP LOCKED` in the candidate subquery
/// steps over a row another worker holds instead of waiting on it.
///
/// The attempt ceiling is not part of the predicate: a delivery that has run
/// out of attempts is stamped `exhausted_at` by the worker that made the last
/// attempt, so the queue never needs to know the configured maximum.
fn claim_statement(target: &ContextTarget, worker_id: LeaseHolder, batch_size: i32, lease_seconds: f64) -> UpdateStatement {
    let candidates = Query::select()
        .column(NotificationDeliveries::DeliveryKey)
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::ExhaustedAt).is_null())
        .and_where(delivery_claimable_at().lte(Expr::current_timestamp()))
        .order_by_expr(delivery_claimable_at(), Order::Asc)
        .order_by(NotificationDeliveries::DeliveryKey, Order::Asc)
        // A negative batch size is an absent bound, not a narrower one, so it
        // clamps to zero rather than wrapping into an enormous unsigned width.
        .limit(u64::try_from(batch_size).unwrap_or(0))
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned();

    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::ClaimedAt, Expr::current_timestamp())
        .value(NotificationDeliveries::ClaimedBy, worker_id.as_uuid())
        .value(
            NotificationDeliveries::LeaseUntil,
            Expr::current_timestamp().add(make_interval_secs(Expr::val(lease_seconds))),
        )
        .from(table_of(target, Subscriptions::Table).alias(DeliverySubscription))
        .and_where(Expr::col((NotificationDeliveries::Table, NotificationDeliveries::DeliveryKey)).in_subquery(candidates))
        .and_where(
            Expr::col((DeliverySubscription, Subscriptions::SubscriptionKey)).equals((NotificationDeliveries::Table, NotificationDeliveries::SubscriptionKey)),
        )
        .returning(Query::returning().exprs([
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::DeliveryKey)),
            Expr::col((DeliverySubscription, Subscriptions::Id)),
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::AttemptCount)),
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::NextAttemptAt)),
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::EventKey)),
        ]))
        .to_owned()
}

/// The instant a delivery row becomes claimable, as `idx_notification_deliveries_claim` indexes it.
fn delivery_claimable_at() -> SimpleExpr {
    claimable_at(Expr::col(NotificationDeliveries::NextAttemptAt), Expr::col(NotificationDeliveries::LeaseUntil))
}

/// The alias of the subscription a delivery answers, `s`.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct DeliverySubscription;

#[cfg(test)]
mod tests {
    use crate::notification_delivery::claim::{claim_statement, snapshot_record_lock_statement};
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    /// The claim orders itself against a reaping drop through the Snapshot's
    /// control record, so the lock must be the one that conflicts with the
    /// reaper's `FOR UPDATE` and with nothing a status transition takes.
    #[test]
    fn a_snapshot_branch_claim_key_share_locks_the_control_record_first() {
        let target = snapshot();
        let snapshot = target.snapshot().expect("a Snapshot target");
        let sql = snapshot_record_lock_statement(snapshot).to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(sql, r#"SELECT "snapshot_key" FROM "snapshots" WHERE "snapshot_key" = 5 FOR KEY SHARE"#);
    }

    fn snapshot() -> ContextTarget {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        ContextTarget::Snapshot(SnapshotTarget::published(key, id))
    }

    #[test]
    fn the_claim_updates_deliveries_alone() {
        let sql = claim_statement(&ContextTarget::Live, LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"UPDATE "notification_deliveries" SET"#), "{sql}");
        assert!(!sql.contains("notification_events"), "the claim must not touch the events table: {sql}");
    }

    #[test]
    fn a_snapshot_branch_claim_qualifies_the_deliveries_table() {
        let sql = claim_statement(&snapshot(), LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "snap_5"."notification_deliveries""#), "{sql}");
    }

    #[test]
    fn the_candidate_predicate_matches_the_partial_claim_index() {
        let sql = claim_statement(&ContextTarget::Live, LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""delivered_at" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""exhausted_at" IS NULL"#), "{sql}");
        assert!(
            sql.contains(r#"GREATEST("next_attempt_at", COALESCE("lease_until", '-infinity'::timestamptz)) <= CURRENT_TIMESTAMP"#),
            "{sql}"
        );
        let predicate = sql.split(" RETURNING ").next().unwrap_or_default();
        assert!(!predicate.contains("attempt_count"), "the claim must not repeat the attempt ceiling: {sql}");
    }

    #[test]
    fn the_candidates_are_ordered_by_the_claimable_instant_then_identifier() {
        let sql = claim_statement(&ContextTarget::Live, LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"ORDER BY GREATEST("next_attempt_at", COALESCE("lease_until", '-infinity'::timestamptz)) ASC, "delivery_key" ASC LIMIT 10"#),
            "{sql}"
        );
    }

    #[test]
    fn a_row_another_worker_holds_is_skipped_rather_than_waited_on() {
        let sql = claim_statement(&ContextTarget::Live, LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains("FOR UPDATE SKIP LOCKED"), "{sql}");
    }

    #[test]
    fn the_claim_takes_a_lease_alongside_the_claim_triple() {
        let sql = claim_statement(&ContextTarget::Live, LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""claimed_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
        assert!(sql.contains("make_interval(secs =>"), "{sql}");
    }

    fn claim_sql(target: &ContextTarget) -> String {
        claim_statement(target, LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder)
    }

    /// The claim returns the rows it leased with their subscriptions, so no
    /// further statement reads them back.
    #[test]
    fn the_claim_returns_the_rows_it_leased_with_their_subscription() {
        let sql = claim_sql(&ContextTarget::Live);
        assert!(
            sql.contains(r#"FROM "subscriptions" AS "s" WHERE "notification_deliveries"."delivery_key" IN (SELECT"#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""s"."subscription_key" = "notification_deliveries"."subscription_key""#),
            "{sql}"
        );
        assert!(!sql.contains("entity_snapshot") && !sql.contains("previous_snapshot"), "{sql}");
    }

    #[test]
    fn the_claim_does_not_recover_the_branch_from_a_control_table_join() {
        let sql = claim_sql(&ContextTarget::Live);
        assert!(!sql.contains("context_spaces"), "the branch is the caller's, not a joined control row: {sql}");
        assert!(!sql.contains(r#""snap""#), "{sql}");
    }

    #[test]
    fn a_snapshot_branch_claim_qualifies_its_subscriptions() {
        let sql = claim_sql(&snapshot());
        assert!(sql.contains(r#"FROM "snap_5"."subscriptions" AS "s""#), "{sql}");
    }

    #[test]
    fn the_claim_returns_each_column_the_row_mapper_reads() {
        let sql = claim_sql(&ContextTarget::Live);
        assert!(
            sql.ends_with(concat!(
                r#"RETURNING "notification_deliveries"."delivery_key", "s"."id", "notification_deliveries"."attempt_count", "#,
                r#""notification_deliveries"."next_attempt_at", "notification_deliveries"."event_key""#
            )),
            "{sql}"
        );
    }
}
