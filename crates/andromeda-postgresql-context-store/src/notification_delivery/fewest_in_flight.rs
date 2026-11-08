//! The candidates of a claim that serves the Subscriptions with the fewest
//! deliveries in flight first.
//!
//! The queue-order claim reads the claim index from its front, so once a slow
//! endpoint's backlog fills that front, skipping its rows would mean reading
//! past every one of them on every claim. This claim reads
//! `idx_notification_deliveries_subscription_claim` instead: it walks the
//! Subscriptions with queued rows by their keys, one probe each, takes the
//! head of each in claim order and counts the rows of each its worker holds
//! leased, so its cost follows the number of Subscriptions with queued rows,
//! never the depth of any backlog.

use crate::notification_delivery::claim::delivery_claimable_at;
use andromeda_context_commit::lease_holder::LeaseHolder;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_notification::delivery_claim_order::InFlightCeiling;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationDeliveryHolds};
use andromeda_postgresql_seaquery::{
    aggregate::{count_all, row_number},
    queue::claimable_at,
    scalar_subquery::scalar_subquery,
};
use sea_query::{
    CommonTableExpression,
    Condition,
    Expr,
    ExprTrait,
    Iden,
    JoinType,
    LockBehavior,
    LockType,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    UnionType,
    WindowStatement,
    WithClause,
};
use std::num::NonZeroU32;

/// `WITH RECURSIVE ... SELECT delivery_key`: up to `batch_size` due
/// deliveries in `target`'s branch, ranked by the load their Subscription
/// would carry once they are taken (its rows leased to `worker_id` plus the
/// row's place among the Subscription's heads), then in claim order, leaving
/// out every row that would take its Subscription past `ceiling`.
///
/// Ranking by that load rather than by the count before the claim spreads a
/// claim of several rows over the Subscriptions, and holds each to the
/// ceiling however many rows one claim takes.
///
/// Each Subscription's heads are picked under `FOR UPDATE SKIP LOCKED`, so
/// the claims one pass runs side by side, one per slot that freed, each step
/// over the rows another has just locked and take the next, instead of all
/// ranking the same row first and all but one coming back empty. A head that
/// is locked and then not chosen stays locked only until the claim commits.
pub(crate) fn fewest_in_flight_candidates(target: &ContextTarget, worker_id: LeaseHolder, batch_size: NonZeroU32, ceiling: InFlightCeiling) -> SelectStatement {
    let mut position = WindowStatement::partition_by((Queued, NotificationDeliveries::SubscriptionKey));
    position
        .order_by((Head, ClaimableAt), Order::Asc)
        .order_by((Head, NotificationDeliveries::DeliveryKey), Order::Asc);
    let mut heads = Query::select();
    heads
        .column((Head, NotificationDeliveries::DeliveryKey))
        .column((Head, ClaimableAt))
        .expr_as(Expr::col((Flight, InFlight)), InFlight)
        .expr_window_as(row_number(), position, Position)
        .from(Queued)
        .join_lateral(JoinType::InnerJoin, head(target, batch_size), Head, Condition::all())
        .join_lateral(JoinType::InnerJoin, in_flight(target, worker_id), Flight, Condition::all());
    let load = Expr::col((Ranked, InFlight)).add(Expr::col((Ranked, Position)));
    let mut ranked = Query::select();
    ranked
        .column((Ranked, NotificationDeliveries::DeliveryKey))
        .from_subquery(heads, Ranked)
        .order_by_expr(load.clone(), Order::Asc) // clone: the ceiling check later compares the same load the order ranks by
        .order_by((Ranked, ClaimableAt), Order::Asc)
        .order_by((Ranked, NotificationDeliveries::DeliveryKey), Order::Asc)
        .limit(u64::from(batch_size.get()))
        .with_cte(queued_subscriptions(target));
    match ceiling {
        InFlightCeiling::Below(ceiling) => {
            ranked.and_where(load.lte(i64::from(ceiling.get())));
        }
        InFlightCeiling::Lifted => {}
    }
    ranked
}

/// `WITH RECURSIVE queued (subscription_key)`: each Subscription with a queued
/// row, found by one index probe past the one before it, and a final NULL row
/// that ends the walk.
fn queued_subscriptions(target: &ContextTarget) -> WithClause {
    let mut first = Query::select().expr(scalar_subquery(next_queued(target, None))).to_owned();
    let step = Query::select()
        .expr(scalar_subquery(next_queued(
            target,
            Some(Expr::col((Queued, NotificationDeliveries::SubscriptionKey))),
        )))
        .from(Queued)
        .and_where(Expr::col((Queued, NotificationDeliveries::SubscriptionKey)).is_not_null())
        .to_owned();
    let walk = first.union(UnionType::All, step).to_owned();
    WithClause::new()
        .recursive(true)
        .cte(
            CommonTableExpression::new()
                .query(walk)
                .column(NotificationDeliveries::SubscriptionKey)
                .table_name(Queued)
                .to_owned(),
        )
        .to_owned()
}

/// The lowest key of a Subscription with a queued row, above `after` when
/// given.
fn next_queued(target: &ContextTarget, after: Option<SimpleExpr>) -> SelectStatement {
    let mut next = Query::select();
    next.column((Walked, NotificationDeliveries::SubscriptionKey))
        .from(table_of(target, NotificationDeliveries::Table).alias(Walked))
        .cond_where(still_queued(Walked));
    if let Some(after) = after {
        next.and_where(Expr::col((Walked, NotificationDeliveries::SubscriptionKey)).gt(after));
    }
    next.order_by((Walked, NotificationDeliveries::SubscriptionKey), Order::Asc).limit(1);
    next
}

/// The first `batch_size` due rows of the walked Subscription in claim order
/// that no other transaction holds, locked, leaving out every row but the
/// probe of a held Subscription.
///
/// The Subscription is bounded from both sides rather than matched, and the
/// rows are ordered by it first: with an equality the planner drops the key
/// from the order and may scan the claim index in queue order instead,
/// filtering past a slow Subscription's whole backlog to find this one's
/// first row, while only `idx_notification_deliveries_subscription_claim`
/// returns this order without a sort.
///
/// The hold is read once for the Subscription, not once per row: either the
/// Subscription is not held, or the row is its probe. Spelled as that
/// disjunction, the planner reads the index range in claim order and stops at
/// the first rows; a per-row test it rates as rare would make it read and
/// sort a whole backlog to find one.
fn head(target: &ContextTarget, batch_size: NonZeroU32) -> SelectStatement {
    Query::select()
        .column(NotificationDeliveries::DeliveryKey)
        .expr_as(delivery_claimable_at(), ClaimableAt)
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::SubscriptionKey))
                .gte(Expr::col((Queued, NotificationDeliveries::SubscriptionKey))),
        )
        .and_where(
            Expr::col((NotificationDeliveries::Table, NotificationDeliveries::SubscriptionKey))
                .lte(Expr::col((Queued, NotificationDeliveries::SubscriptionKey))),
        )
        .cond_where(still_queued(NotificationDeliveries::Table))
        .and_where(delivery_claimable_at().lte(Expr::current_timestamp()))
        .cond_where(
            Condition::any()
                .add(Expr::exists(hold(target, Expr::val(1))).not())
                .add(Expr::col(NotificationDeliveries::DeliveryKey).eq(scalar_subquery(hold(
                    target,
                    Expr::col((NotificationDeliveryHolds::Table, NotificationDeliveryHolds::DeliveryKey)),
                )))),
        )
        .order_by((NotificationDeliveries::Table, NotificationDeliveries::SubscriptionKey), Order::Asc)
        .order_by_expr(delivery_claimable_at(), Order::Asc)
        .order_by(NotificationDeliveries::DeliveryKey, Order::Asc)
        .limit(u64::from(batch_size.get()))
        .lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)
        .to_owned()
}

/// `SELECT <value> FROM notification_delivery_holds WHERE` the walked
/// Subscription is held: one row, whose `delivery_key` is its probe, the one
/// row of it a claim may take, or no row when it is not held.
fn hold(target: &ContextTarget, value: SimpleExpr) -> SelectStatement {
    Query::select()
        .expr(value)
        .from(table_of(target, NotificationDeliveryHolds::Table))
        .and_where(
            Expr::col((NotificationDeliveryHolds::Table, NotificationDeliveryHolds::SubscriptionKey)).equals((Queued, NotificationDeliveries::SubscriptionKey)),
        )
        .to_owned()
}

/// `SELECT COUNT(*) AS in_flight`: the walked Subscription's rows leased to
/// `worker_id` under a lease that still holds.
///
/// A row under a live lease is claimable only once its lease lapses, so these
/// rows sit past the present in the index's claim-order column, beside only
/// the Subscription's retries not yet due.
fn in_flight(target: &ContextTarget, worker_id: LeaseHolder) -> SelectStatement {
    Query::select()
        .expr_as(count_all(), InFlight)
        .from(table_of(target, NotificationDeliveries::Table).alias(Leased))
        .and_where(Expr::col((Leased, NotificationDeliveries::SubscriptionKey)).equals((Queued, NotificationDeliveries::SubscriptionKey)))
        .cond_where(still_queued(Leased))
        .and_where(claimable_at_of(Leased).gt(Expr::current_timestamp()))
        .and_where(Expr::col((Leased, NotificationDeliveries::ClaimedBy)).eq(worker_id.as_uuid()))
        .to_owned()
}

/// The partial predicate of both claim indexes, on the rows of `table`:
/// neither delivered, exhausted nor set aside behind a probe.
fn still_queued(table: impl Iden + Copy + 'static) -> Condition {
    Condition::all()
        .add(Expr::col((table, NotificationDeliveries::DeliveredAt)).is_null())
        .add(Expr::col((table, NotificationDeliveries::ExhaustedAt)).is_null())
        .add(Expr::col((table, NotificationDeliveries::HeldAt)).is_null())
}

/// The claim-order instant of a row of `table`, the expression both claim
/// indexes carry.
fn claimable_at_of(table: impl Iden + Copy + 'static) -> SimpleExpr {
    claimable_at(
        Expr::col((table, NotificationDeliveries::NextAttemptAt)),
        Expr::col((table, NotificationDeliveries::LeaseUntil)),
    )
}

/// The Subscriptions the walk found, `queued`.
#[derive(Iden, Clone, Copy)]
#[iden = "queued"]
struct Queued;

/// The deliveries table as the walk probes it, `walked`.
#[derive(Iden, Clone, Copy)]
#[iden = "walked"]
struct Walked;

/// The head rows of one walked Subscription, `head`.
#[derive(Iden, Clone, Copy)]
#[iden = "head"]
struct Head;

/// The in-flight count of one walked Subscription, `flight`.
#[derive(Iden, Clone, Copy)]
#[iden = "flight"]
struct Flight;

/// The deliveries table as the in-flight count reads it, `leased`.
#[derive(Iden, Clone, Copy)]
#[iden = "leased"]
struct Leased;

/// A head row's claim-order instant, `claimable_at`.
#[derive(Iden, Clone, Copy)]
#[iden = "claimable_at"]
struct ClaimableAt;

/// The ranked head rows, `ranked`.
#[derive(Iden, Clone, Copy)]
#[iden = "ranked"]
struct Ranked;

/// A head row's place among its Subscription's heads, from 1, `position`.
#[derive(Iden, Clone, Copy)]
#[iden = "position"]
struct Position;

/// A walked Subscription's rows in flight, `in_flight`.
#[derive(Iden, Clone, Copy)]
#[iden = "in_flight"]
struct InFlight;

#[cfg(test)]
mod tests {
    use crate::notification_delivery::fewest_in_flight::fewest_in_flight_candidates;
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_notification::delivery_claim_order::InFlightCeiling;
    use sea_query::PostgresQueryBuilder;
    use std::num::NonZeroU32;
    use uuid::Uuid;

    fn sql(target: &ContextTarget) -> String {
        fewest_in_flight_candidates(target, LeaseHolder::from(Uuid::nil()), NonZeroU32::MIN, InFlightCeiling::Lifted).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_walk_probes_one_subscription_past_the_one_before_it() {
        let sql = sql(&ContextTarget::Live);
        assert!(sql.contains(r#"WITH RECURSIVE "queued" ("subscription_key") AS"#), "{sql}");
        assert!(sql.contains(r#""walked"."subscription_key" > "queued"."subscription_key""#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "walked"."subscription_key" ASC LIMIT 1"#), "{sql}");
        assert!(sql.contains(r#"WHERE "queued"."subscription_key" IS NOT NULL"#), "{sql}");
    }

    #[test]
    fn the_subscriptions_with_the_fewest_in_flight_rank_first_then_claim_order() {
        let sql = sql(&ContextTarget::Live);
        assert!(
            sql.contains(
                r#"ROW_NUMBER() OVER ( PARTITION BY "queued"."subscription_key" ORDER BY "head"."claimable_at" ASC, "head"."delivery_key" ASC ) AS "position""#
            ),
            "{sql}"
        );
        assert!(
            sql.contains(concat!(
                r#"ORDER BY "ranked"."in_flight" + "ranked"."position" ASC, "ranked"."claimable_at" ASC, "#,
                r#""ranked"."delivery_key" ASC LIMIT 1"#
            )),
            "{sql}"
        );
    }

    #[test]
    fn in_flight_counts_the_rows_the_claiming_worker_holds_under_a_live_lease() {
        let sql = sql(&ContextTarget::Live);
        assert!(sql.contains(r#""leased"."claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
        assert!(
            sql.contains(r#"GREATEST("leased"."next_attempt_at", COALESCE("leased"."lease_until", '-infinity'::timestamptz)) > CURRENT_TIMESTAMP"#),
            "{sql}"
        );
    }

    #[test]
    fn a_held_subscription_offers_only_its_probe_as_its_head() {
        let sql = sql(&ContextTarget::Live);
        assert!(
            sql.contains(concat!(
                r#"((NOT EXISTS(SELECT 1 FROM "notification_delivery_holds" WHERE "notification_delivery_holds"."subscription_key" = "#,
                r#""queued"."subscription_key")) OR "delivery_key" = (SELECT "notification_delivery_holds"."delivery_key" "#,
                r#"FROM "notification_delivery_holds" WHERE "notification_delivery_holds"."subscription_key" = "queued"."subscription_key"))"#
            )),
            "{sql}"
        );
    }

    #[test]
    fn a_ceiling_passes_over_the_subscriptions_at_it() {
        let ceiling = InFlightCeiling::Below(NonZeroU32::MIN.saturating_add(9));
        let sql = fewest_in_flight_candidates(&ContextTarget::Live, LeaseHolder::from(Uuid::nil()), NonZeroU32::MIN, ceiling).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"WHERE "ranked"."in_flight" + "ranked"."position" <= 10 ORDER BY"#), "{sql}");
    }

    #[test]
    fn a_lifted_ceiling_passes_over_no_subscription() {
        let sql = sql(&ContextTarget::Live);
        assert!(!sql.contains(r#""ranked"."position" <="#), "{sql}");
    }

    #[test]
    fn each_head_is_locked_as_it_is_picked_so_side_by_side_claims_take_different_rows() {
        let sql = sql(&ContextTarget::Live);
        assert!(sql.contains(r#""delivery_key" ASC LIMIT 1 FOR UPDATE SKIP LOCKED) AS "head""#), "{sql}");
        assert!(
            sql.contains(concat!(
                r#""notification_deliveries"."subscription_key" >= "queued"."subscription_key" AND "#,
                r#""notification_deliveries"."subscription_key" <= "queued"."subscription_key""#
            )),
            "{sql}"
        );
        assert!(
            sql.contains(r#"ORDER BY "notification_deliveries"."subscription_key" ASC, GREATEST("next_attempt_at""#),
            "{sql}"
        );
        assert_eq!(sql.matches("FOR UPDATE").count(), 1, "{sql}");
    }

    #[test]
    fn every_relation_of_a_snapshot_branch_claim_is_qualified_with_its_schema() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let id = "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri");
        let sql = sql(&ContextTarget::Snapshot(SnapshotTarget::published(key, id)));
        assert!(!sql.contains(r#"FROM "notification_deliveries""#), "{sql}");
        assert!(!sql.contains(r#"FROM "notification_delivery_holds""#), "{sql}");
    }
}
