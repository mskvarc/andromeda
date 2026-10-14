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
        claimed_event::{RawClaimedEventRow, ReadEvents},
        rows::{RawClaimedDeliveryRow, UnreadableClaimedDelivery},
    },
};
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::{context_target::ContextTarget, snapshot_target::SnapshotTarget};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_notification::{delivery_entry::DeliveryEntry, delivery_lease::LeaseTerm};
use andromeda_postgresql_attribute_instance::name_resolution::names_of;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
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
use indexmap::IndexSet;
use sea_query::{
    Alias,
    Expr,
    ExprTrait,
    Func,
    Iden,
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

/// The dictionaries a claim reads the names of its events' states back
/// through.
#[derive(Debug, Clone, Copy)]
pub struct ClaimDictionaries<'a> {
    /// The `attribute_names` dictionary.
    pub attribute_names: &'a dyn NameLookup<AttributeName>,
    /// The `entity_type_names` dictionary.
    pub entity_type_names: &'a dyn NameLookup<EntityTypeName>,
}

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
) -> Result<Vec<Result<DeliveryEntry, Box<UnreadableClaimedDelivery>>>, ContextStoreError> {
    let mut results = Vec::new();
    let mut remaining = batch_size;
    for target in drainable_targets(pool).await? {
        if remaining <= 0 {
            break;
        }
        match claim_branch(pool, dictionaries, &target, worker_id, lease, remaining).await {
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
    dictionaries: ClaimDictionaries<'_>,
    target: &ContextTarget,
    worker_id: LeaseHolder,
    lease: LeaseTerm,
    batch_size: i32,
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

    let event_keys = distinct_event_keys(deliveries.iter().map(|delivery| delivery.event_key));
    let (sql, values) = claimed_events_statement(target, &event_keys).build_sqlx(PostgresQueryBuilder);
    let (unnest_alias, unnest_columns, ordinality) = ordinality_alias();
    let sql = apply_with_ordinality(sql, &unnest_alias, &unnest_columns, &ordinality);
    let event_rows: Vec<RawClaimedEventRow> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&mut *tx).await?;

    tx.commit().await?;
    let read = ReadEvents::read(event_rows);
    let names = names_of(&read.held_keys(), dictionaries.attribute_names, dictionaries.entity_type_names)
        .await
        .map_err(ContextStoreError::NameLookup)?;
    let events = read.assemble(&names);
    Ok(deliveries.into_iter().map(|delivery| delivery.into_entry(&events, target)).collect())
}

/// The events a claim's deliveries answer, each once, in the order the
/// deliveries first name them.
fn distinct_event_keys(event_keys: impl IntoIterator<Item = i64>) -> Vec<i64> {
    event_keys.into_iter().collect::<IndexSet<i64>>().into_iter().collect()
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

/// The `UNNEST … WITH ORDINALITY AS k(name_key, position)` names that
/// [`apply_with_ordinality`] rewrites the projection's set-returning source to.
///
/// The one row's `name_key` is the dictionary key `UNNEST` yields and `position`
/// is the ordinality that preserves the order the event recorded its attributes
/// in; both must match the names the projection's aggregate reads.
fn ordinality_alias() -> (Alias, [UnnestColumn; 1], Alias) {
    (
        Alias::new("k"),
        [UnnestColumn::new(Alias::new("name_key"), PostgresType::IntegerArray)],
        Alias::new("position"),
    )
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

/// Read each event the claimed deliveries answer once, in `target`'s branch.
///
/// The changed attributes are resolved back through the tenant-global
/// `attribute_names` dictionary in event-recorded order by a lateral aggregate
/// over the event's `change_attribute_name_keys`, defaulting to an empty array
/// when the event touched none. The aggregate keeps one name per key, repeats
/// included, so it stays position for position with the event's transitions
/// and `datasetId`s.
fn claimed_events_statement(target: &ContextTarget, event_keys: &[i64]) -> SelectStatement {
    let (unnest_alias, unnest_columns, position) = ordinality_alias();

    let ordered_names = Query::select()
        .expr(array_agg_ordered(
            Expr::col((ChangedName, AttributeNames::Name)),
            // clone: the aggregate and the join below name the same unnested relation.
            Expr::col((unnest_alias.clone(), position)),
        ))
        .from_function(
            unnest_call([(&unnest_columns[0], Expr::col((ClaimedEventRow, NotificationEvents::ChangeAttributeNameKeys)))]),
            // clone: the unnested relation is named by its function call and by its join.
            unnest_alias.clone(),
        )
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            ChangedName,
            Expr::col((ChangedName, AttributeNames::AttributeNameKey)).equals((unnest_alias, Alias::new("name_key"))),
        )
        .to_owned();

    let change_attributes = Func::coalesce([SimpleExpr::SubQuery(None, Box::new(ordered_names.into_sub_query_statement())), text_array(&[])]);

    Query::select()
        .column((ClaimedEventRow, NotificationEvents::EventKey))
        .column((ClaimedEventRow, NotificationEvents::WriteId))
        .column((ClaimedEventRow, NotificationEvents::EventOrdinal))
        .column((ClaimedEventRow, NotificationEvents::Lifecycle))
        .column((ClaimedEventRow, NotificationEvents::EntityId))
        .column((ClaimedEventRow, NotificationEvents::ChangeKinds))
        .expr_as(change_attributes, Alias::new("change_attributes"))
        .column((ClaimedEventRow, NotificationEvents::ChangeDatasetIds))
        .column((ClaimedEventRow, NotificationEvents::EntitySnapshot))
        .column((ClaimedEventRow, NotificationEvents::PreviousSnapshot))
        .column((ClaimedEventRow, NotificationEvents::EntityCreatedAt))
        .column((ClaimedEventRow, NotificationEvents::EntityModifiedAt))
        .column((ClaimedEventRow, NotificationEvents::EntityExpiresAt))
        .column((ClaimedEventRow, NotificationEvents::CreatedAt))
        .from(table_of(target, NotificationEvents::Table).alias(ClaimedEventRow))
        .and_where(Expr::col((ClaimedEventRow, NotificationEvents::EventKey)).eq(any(bigint_array(event_keys))))
        .to_owned()
}

/// The alias of the subscription a delivery answers, `s`.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct DeliverySubscription;

/// The alias of an event a claimed delivery answers, `e`.
#[derive(Iden, Clone, Copy)]
#[iden = "e"]
struct ClaimedEventRow;

/// The alias of one changed attribute's dictionary row, `n`.
#[derive(Iden, Clone, Copy)]
#[iden = "n"]
struct ChangedName;

/// A bound `bigint[]` value from a batch of claimed identifiers.
fn bigint_array(ids: &[i64]) -> SimpleExpr {
    Expr::val(ids.to_vec())
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::claim::{
        claim_statement,
        claimed_events_statement,
        distinct_event_keys,
        ordinality_alias,
        snapshot_record_lock_statement,
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_postgresql_seaquery::unnest::apply_with_ordinality;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    fn events_sql(target: &ContextTarget) -> String {
        let sql = claimed_events_statement(target, &[7, 3]).to_string(PostgresQueryBuilder);
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

    #[test]
    fn the_changed_attributes_keep_the_order_the_event_recorded_them_in() {
        let sql = events_sql(&ContextTarget::Live);
        assert!(sql.contains(r#"array_agg("n"."name" ORDER BY "k"."position")"#), "{sql}");
        assert!(sql.contains("WITH ORDINALITY AS k(name_key, position)"), "{sql}");
        assert!(sql.contains("COALESCE"), "{sql}");
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
    fn the_event_projection_reads_each_claimed_event_once() {
        let sql = events_sql(&ContextTarget::Live);
        assert!(sql.contains(r#"FROM "notification_events" AS "e""#), "{sql}");
        assert!(sql.contains(r#""e"."event_key" = ANY("#), "{sql}");
        assert!(!sql.contains("notification_deliveries"), "{sql}");
        for column in [
            "event_key",
            "write_id",
            "event_ordinal",
            "lifecycle",
            "entity_id",
            "change_kinds",
            "change_dataset_ids",
            "entity_snapshot",
            "previous_snapshot",
            "entity_created_at",
            "entity_modified_at",
            "entity_expires_at",
            "created_at",
        ] {
            assert!(sql.contains(&format!(r#""e"."{column}""#)), "{column}: {sql}");
        }
        assert!(sql.contains(r#"AS "change_attributes""#), "{sql}");
    }

    #[test]
    fn a_snapshot_branch_event_projection_qualifies_its_relation() {
        let sql = events_sql(&snapshot());
        assert!(sql.contains(r#"FROM "snap_5"."notification_events" AS "e""#), "{sql}");
    }

    #[test]
    fn the_event_keys_of_a_claim_are_distinct_in_first_seen_order() {
        pretty_assertions::assert_eq!(distinct_event_keys([7, 3, 7, 9, 3]), [7, 3, 9]);
    }

    #[test]
    fn the_projection_does_not_recover_the_branch_from_a_control_table_join() {
        for sql in [claim_sql(&ContextTarget::Live), events_sql(&ContextTarget::Live)] {
            assert!(!sql.contains("context_spaces"), "the branch is the caller's, not a joined control row: {sql}");
            assert!(!sql.contains(r#""snap""#), "{sql}");
        }
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
