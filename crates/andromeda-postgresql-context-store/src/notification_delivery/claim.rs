//! Leasing due deliveries and reading the events they answer.
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
        rows::{ClaimedDelivery, ClaimedDeliveryError, RawClaimedDeliveryRow},
    },
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::{context_target::ContextTarget, snapshot_target::SnapshotTarget};
use andromeda_notification::delivery_lease::LeaseTerm;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    notification::{NotificationDeliveries, NotificationEvents},
    snapshot::Snapshots,
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{
    aggregate::array_agg_ordered,
    interval::make_interval_secs,
    pg_array::{any, text_array},
    pg_type::PostgresType,
    queue::claimable_at,
    unnest::{UnnestColumn, apply_with_ordinality, unnest_call},
};
use andromeda_telemetry::statement::{StatementName, observe_statement};
use sea_query::{
    Alias,
    Expr,
    ExprTrait,
    Func,
    JoinType,
    LockBehavior,
    LockType,
    Order,
    PostgresQueryBuilder,
    Query,
    QueryStatementBuilder,
    SelectStatement,
    SimpleExpr,
    UpdateStatement,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::{Postgres, Transaction, postgres::PgPool};

/// The statement name the delivery claim is observed under.
const DELIVERY_CLAIM: StatementName = StatementName("notification-delivery-claim");

/// Claim up to `batch_size` due deliveries for `worker_id` under `lease`,
/// sweeping the live branch and every Snapshot branch until the batch is filled.
///
/// Each branch is drained in turn against the remaining budget. A branch reaped
/// between the enumeration and its claim (a Snapshot deleted concurrently) shows
/// up as a missing schema and is skipped -- its notifications are moot once the
/// branch is gone.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails for a reason
/// other than a reaped branch, or [`ContextStoreError::Internal`] when the
/// branch enumeration cannot read back a stored Snapshot identity. A single
/// unreadable row is reported inside the returned vector rather than as a
/// failure of the claim.
pub async fn claim_due(
    pool: &PgPool,
    worker_id: LeaseHolder,
    lease: LeaseTerm,
    batch_size: i32,
) -> Result<Vec<Result<ClaimedDelivery, ClaimedDeliveryError>>, ContextStoreError> {
    let mut results = Vec::new();
    let mut remaining = batch_size;
    for target in drainable_targets(pool).await? {
        if remaining <= 0 {
            break;
        }
        match claim_branch(pool, &target, worker_id, lease, remaining).await {
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

/// Claim and read back up to `batch_size` due deliveries from one branch.
async fn claim_branch(
    pool: &PgPool,
    target: &ContextTarget,
    worker_id: LeaseHolder,
    lease: LeaseTerm,
    batch_size: i32,
) -> Result<Vec<Result<ClaimedDelivery, ClaimedDeliveryError>>, ContextStoreError> {
    let mut tx = pool.begin().await?;
    if let Some(snapshot) = target.snapshot()
        && hold_snapshot_record(&mut tx, snapshot).await? == SnapshotRecord::Reaped
    {
        tx.commit().await?;
        return Ok(Vec::new());
    }

    let (sql, values) = claim_statement(target, worker_id, batch_size, f64::from(lease.seconds())).build_sqlx(PostgresQueryBuilder);
    let claim = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut *tx);
    let claimed_ids: Vec<i64> = observe_statement(DELIVERY_CLAIM, claim).await?;
    if claimed_ids.is_empty() {
        tx.commit().await?;
        return Ok(Vec::new());
    }

    let (sql, values) = claimed_projection_statement(target, &claimed_ids).build_sqlx(PostgresQueryBuilder);
    let (unnest_alias, unnest_columns, ordinality) = ordinality_alias();
    let sql = apply_with_ordinality(sql, &unnest_alias, &unnest_columns, &ordinality);
    let rows: Vec<RawClaimedDeliveryRow> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut *tx).await?;

    tx.commit().await?;
    Ok(rows.into_iter().map(|row| ClaimedDelivery::from_row(row, target.clone())).collect())
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

/// The `UNNEST … WITH ORDINALITY AS k(name_id, position)` names that
/// [`apply_with_ordinality`] rewrites the projection's set-returning source to.
///
/// The one row's `name_id` is the dictionary key `UNNEST` yields and `position`
/// is the ordinality that preserves the order the event recorded its attributes
/// in; both must match the names the projection's aggregate reads.
fn ordinality_alias() -> (Alias, [UnnestColumn; 1], Alias) {
    (
        Alias::new("k"),
        [UnnestColumn::new(Alias::new("name_id"), PostgresType::IntegerArray)],
        Alias::new("position"),
    )
}

/// Claim a batch of due deliveries in `target`'s branch, returning their identifiers.
///
/// The candidate predicate is exactly the one the queue's partial claim index
/// carries: still queued (neither delivered nor exhausted) and claimable — the
/// later of its due instant and its lease expiry has passed. That instant is
/// the index's expression, spelled through the one shared helper so the
/// statement and the index match structurally; ordering by it, then by `id`,
/// is the index's own key order, so the claim is one bounded range scan whose
/// depth is the batch size. `FOR UPDATE SKIP LOCKED` in the candidate subquery
/// steps over a row another worker holds instead of waiting on it.
///
/// The attempt ceiling is not part of the predicate: a delivery that has run
/// out of attempts is stamped `exhausted_at` by the worker that made the last
/// attempt, so the queue never needs to know the configured maximum.
fn claim_statement(target: &ContextTarget, worker_id: LeaseHolder, batch_size: i32, lease_seconds: f64) -> UpdateStatement {
    let candidates = Query::select()
        .column(NotificationDeliveries::Id)
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::ExhaustedAt).is_null())
        .and_where(delivery_claimable_at().lte(Expr::current_timestamp()))
        .order_by_expr(delivery_claimable_at(), Order::Asc)
        .order_by(NotificationDeliveries::Id, Order::Asc)
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
        .and_where(Expr::col(NotificationDeliveries::Id).in_subquery(candidates))
        .returning_col(NotificationDeliveries::Id)
        .to_owned()
}

/// The instant a delivery row becomes claimable, as `idx_notification_deliveries_claimable` indexes it.
fn delivery_claimable_at() -> SimpleExpr {
    claimable_at(Expr::col(NotificationDeliveries::NextAttemptAt), Expr::col(NotificationDeliveries::LeaseUntil))
}

/// Read the claimed deliveries back with the events they answer, in `target`'s branch.
///
/// Only the rows this worker just claimed are named, by identifier: they carry
/// a live lease held by this transaction, so no other worker can have altered
/// them between the claim and this read. The changed attributes are resolved
/// back through the tenant-global `attribute_names` dictionary in event-recorded
/// order by a lateral aggregate over the event's `changed_attribute_name_ids`,
/// defaulting to an empty array when the event touched none.
fn claimed_projection_statement(target: &ContextTarget, claimed_ids: &[i64]) -> SelectStatement {
    let (delivery, event, subscription) = (Alias::new("c"), Alias::new("e"), Alias::new("s"));
    let (unnest_alias, unnest_columns, position) = ordinality_alias();
    let dictionary = Alias::new("n");

    let ordered_names = Query::select()
        .expr(array_agg_ordered(
            Expr::col((dictionary.clone(), AttributeNames::Name)),
            Expr::col((unnest_alias.clone(), position)),
        ))
        .from_function(
            unnest_call([(&unnest_columns[0], Expr::col((event.clone(), NotificationEvents::ChangedAttributeNameIds)))]),
            unnest_alias.clone(),
        )
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            dictionary.clone(),
            Expr::col((dictionary, AttributeNames::Id)).equals((unnest_alias, Alias::new("name_id"))),
        )
        .to_owned();

    let changed_attributes = Func::coalesce([SimpleExpr::SubQuery(None, Box::new(ordered_names.into_sub_query_statement())), text_array(&[])]);

    Query::select()
        .column((delivery.clone(), NotificationDeliveries::Id))
        .expr_as(Expr::col((subscription.clone(), Subscriptions::Id)), Alias::new("subscription_id"))
        .column((delivery.clone(), NotificationDeliveries::AttemptCount))
        .column((delivery.clone(), NotificationDeliveries::NextAttemptAt))
        .expr_as(Expr::col((event.clone(), NotificationEvents::Id)), Alias::new("event_id"))
        .column((event.clone(), NotificationEvents::WriteId))
        .column((event.clone(), NotificationEvents::EventOrdinal))
        .column((event.clone(), NotificationEvents::Trigger))
        .column((event.clone(), NotificationEvents::EntityId))
        .expr_as(changed_attributes, Alias::new("changed_attributes"))
        .column((event.clone(), NotificationEvents::ChangedDatasetId))
        .column((event.clone(), NotificationEvents::EntitySnapshot))
        .column((event.clone(), NotificationEvents::PreviousSnapshot))
        .column((event.clone(), NotificationEvents::CreatedAt))
        .from(table_of(target, NotificationDeliveries::Table).alias(delivery.clone()))
        .join_as(
            JoinType::InnerJoin,
            table_of(target, NotificationEvents::Table),
            event.clone(),
            Expr::col((event, NotificationEvents::Id)).equals((delivery.clone(), NotificationDeliveries::EventId)),
        )
        .join_as(
            JoinType::InnerJoin,
            table_of(target, Subscriptions::Table),
            subscription.clone(),
            Expr::col((subscription, Subscriptions::SubscriptionKey)).equals((delivery.clone(), NotificationDeliveries::SubscriptionKey)),
        )
        .and_where(Expr::col((delivery, NotificationDeliveries::Id)).eq(any(bigint_array(claimed_ids))))
        .to_owned()
}

/// A bound `bigint[]` value from a batch of claimed identifiers.
fn bigint_array(ids: &[i64]) -> SimpleExpr {
    Expr::val(ids.to_vec())
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::claim::{claim_statement, claimed_projection_statement, ordinality_alias, snapshot_record_lock_statement};
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_postgresql_seaquery::unnest::apply_with_ordinality;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    fn projection_sql(target: &ContextTarget) -> String {
        let sql = claimed_projection_statement(target, &[1, 2]).to_string(PostgresQueryBuilder);
        let (alias, columns, ordinality) = ordinality_alias();
        apply_with_ordinality(sql, &alias, &columns, &ordinality)
    }

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
    fn the_claim_scans_and_updates_deliveries_alone() {
        let sql = claim_statement(&ContextTarget::Live, LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UPDATE "notification_deliveries""#), "{sql}");
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
        assert!(!sql.contains("attempt_count"), "the claim must not repeat the attempt ceiling: {sql}");
    }

    #[test]
    fn the_candidates_are_ordered_by_the_claimable_instant_then_identifier() {
        let sql = claim_statement(&ContextTarget::Live, LeaseHolder::from(Uuid::nil()), 10, 30.0).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"ORDER BY GREATEST("next_attempt_at", COALESCE("lease_until", '-infinity'::timestamptz)) ASC, "id" ASC LIMIT 10"#),
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

    #[test]
    fn the_changed_attributes_keep_the_order_the_event_recorded_them_in() {
        let sql = projection_sql(&ContextTarget::Live);
        assert!(sql.contains(r#"array_agg("n"."name" ORDER BY "k"."position")"#), "{sql}");
        assert!(sql.contains("WITH ORDINALITY AS k(name_id, position)"), "{sql}");
    }

    #[test]
    fn the_projection_reads_only_the_rows_this_worker_claimed() {
        let sql = projection_sql(&ContextTarget::Live);
        assert!(sql.contains(r#""c"."id" = ANY("#), "{sql}");
        assert!(sql.contains("COALESCE"), "{sql}");
    }

    #[test]
    fn the_projection_does_not_recover_the_branch_from_a_control_table_join() {
        let sql = projection_sql(&ContextTarget::Live);
        assert!(!sql.contains("context_spaces"), "the branch is the caller's, not a joined control row: {sql}");
        assert!(!sql.contains(r#""snap""#), "{sql}");
    }

    #[test]
    fn a_snapshot_branch_projection_qualifies_its_relations() {
        let sql = projection_sql(&snapshot());
        assert!(sql.contains(r#"FROM "snap_5"."notification_deliveries" AS "c""#), "{sql}");
        assert!(sql.contains(r#""snap_5"."notification_events" AS "e""#), "{sql}");
        assert!(sql.contains(r#""snap_5"."subscriptions" AS "s""#), "{sql}");
    }

    #[test]
    fn the_projection_names_each_column_the_row_mapper_reads() {
        let sql = projection_sql(&ContextTarget::Live);
        assert!(sql.contains(r#"AS "subscription_id""#), "{sql}");
        assert!(sql.contains(r#"AS "event_id""#), "{sql}");
        assert!(sql.contains(r#"AS "changed_attributes""#), "{sql}");
    }
}
