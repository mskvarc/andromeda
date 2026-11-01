//! The hold a failing endpoint puts on the rest of its Subscription's queue.
//!
//! A retryable failure takes a hold on the Subscription, naming the failed
//! delivery its probe, and sets every other queued delivery of the
//! Subscription aside (`held_at`). The claim then offers the Subscription's
//! probe alone, so an endpoint that never answers receives one attempt per
//! retry step however many deliveries wait for it, and the slots of the
//! tenant's drain stay free for every other Subscription.
//!
//! An attempt that reaches the endpoint deletes the hold and releases the
//! deliveries it set aside, each with the attempt count and the due instant it
//! had, so they go out in the order they were queued. A probe that runs out of
//! attempts hands the hold to the delivery set aside first, due one retry step
//! later; a hold with nothing left to hand over is deleted.
//!
//! An acknowledgement changes the hold in two statements of its own
//! transaction. The first records the outcome and locks the hold row, by
//! taking, deleting or selecting it for update; the second sets deliveries
//! aside or releases them. Every change to a Subscription's held deliveries is
//! made under that lock, and the second statement reads a snapshot taken once
//! the lock is granted, so it sees every delivery the previous holder of the
//! lock set aside. One statement could not: its every part reads the snapshot
//! taken before it waited, and a release would leave behind, held for good,
//! the deliveries a failure committed while it waited. A delivery not yet set
//! aside is never lost that way, because the claim keeps it back only while
//! the hold exists.

use crate::error::ContextStoreError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationDeliveryHolds};
use chrono::{DateTime, Utc};
use sea_query::{
    CommonTableExpression,
    Expr,
    ExprTrait,
    Iden,
    InsertStatement,
    JoinType,
    Keyword,
    LockType,
    OnConflict,
    Order,
    Query,
    SelectStatement,
    SimpleExpr,
    SubQueryStatement,
    UpdateStatement,
    WithClause,
    WithQuery,
};

/// The acknowledged delivery, `acknowledged`: the CTE every acknowledgement
/// opens with, returning the delivery's key and its Subscription's.
#[derive(Iden, Clone, Copy)]
#[iden = "acknowledged"]
pub(crate) struct Acknowledged;

/// The hold a failure took or found, `taken`.
#[derive(Iden, Clone, Copy)]
#[iden = "taken"]
pub(crate) struct Taken;

/// The hold an attempt that reached the endpoint deleted, `released`.
#[derive(Iden, Clone, Copy)]
#[iden = "released"]
pub(crate) struct Released;

/// The hold whose probe was exhausted, locked for its hand-over, `probed`.
#[derive(Iden, Clone, Copy)]
#[iden = "probed"]
pub(crate) struct Probed;

/// The delivery an exhausted probe hands its hold to, `successor`.
#[derive(Iden, Clone, Copy)]
#[iden = "successor"]
struct Successor;

/// The hold moved onto the successor, `handed_over`, whose only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "handed_over"]
struct HandedOver;

/// The hold of an exhausted probe no delivery succeeds, `dropped`, whose only
/// purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "dropped"]
struct Dropped;

/// The successor taken back into the claim order, `revived`, whose only
/// purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "revived"]
struct Revived;

/// The held Subscription and its probe, as a failure's first statement
/// returns them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct HeldSubscription {
    /// The Subscription the hold is on.
    pub(crate) subscription_key: i64,
    /// The one delivery of the Subscription a claim may take.
    pub(crate) probe_delivery_key: i64,
}

/// The hold a retryable failure takes, or finds and locks:
///
/// ```sql
/// taken AS (INSERT INTO notification_delivery_holds (subscription_key, delivery_key)
///           SELECT subscription_key, delivery_key FROM acknowledged
///           ON CONFLICT (subscription_key) DO UPDATE SET delivery_key = notification_delivery_holds.delivery_key
///           RETURNING subscription_key, delivery_key)
/// ```
///
/// The failed delivery becomes the probe unless the Subscription is held
/// already; the conflict branch rewrites the probe it finds, so the existing
/// hold comes back and its row stays locked to the end of the transaction.
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] when the insert cannot take
/// its rows from the acknowledged delivery.
pub(crate) fn taken_on_failure(target: &ContextTarget) -> Result<CommonTableExpression, ContextStoreError> {
    Ok(cte(Taken, take_hold(target)?))
}

/// Set aside every queued delivery of the held Subscription but its probe and
/// those in flight under a live lease, which their own outcomes settle.
///
/// ```sql
/// UPDATE notification_deliveries SET held_at = CURRENT_TIMESTAMP, claimed_at = NULL, claimed_by = NULL, lease_until = NULL
/// WHERE subscription_key = $1 AND held_at IS NULL AND delivery_key <> $2
///   AND delivered_at IS NULL AND exhausted_at IS NULL AND (lease_until IS NULL OR lease_until <= CURRENT_TIMESTAMP)
/// ```
///
/// The failed delivery is among them when it is not the probe; a lapsed lease
/// is cleared with the rest of its claim, which a held row may not carry.
pub(crate) fn set_aside_statement(target: &ContextTarget, held: HeldSubscription) -> UpdateStatement {
    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::HeldAt, Expr::current_timestamp())
        .value(NotificationDeliveries::ClaimedAt, null())
        .value(NotificationDeliveries::ClaimedBy, null())
        .value(NotificationDeliveries::LeaseUntil, null())
        .and_where(Expr::col(NotificationDeliveries::SubscriptionKey).eq(held.subscription_key))
        .and_where(Expr::col(NotificationDeliveries::HeldAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::DeliveryKey).ne(held.probe_delivery_key))
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::ExhaustedAt).is_null())
        .and_where(
            Expr::col(NotificationDeliveries::LeaseUntil)
                .is_null()
                .or(Expr::col(NotificationDeliveries::LeaseUntil).lte(Expr::current_timestamp())),
        )
        .to_owned()
}

/// The hold a delivered attempt deletes:
///
/// ```sql
/// released AS (DELETE FROM notification_delivery_holds
///              WHERE subscription_key IN (SELECT subscription_key FROM acknowledged) RETURNING subscription_key)
/// ```
///
/// The endpoint answered, whichever delivery reached it, so the Subscription's
/// queue flows again. A Subscription that is not held deletes nothing.
pub(crate) fn released_on_delivery(target: &ContextTarget) -> CommonTableExpression {
    let released = Query::delete()
        .from_table(table_of(target, NotificationDeliveryHolds::Table))
        .and_where(Expr::col(NotificationDeliveryHolds::SubscriptionKey).in_subquery(acknowledged(NotificationDeliveries::SubscriptionKey)))
        .returning_col(NotificationDeliveryHolds::SubscriptionKey)
        .to_owned();
    cte(Released, released)
}

/// Take every delivery the released hold of `subscription_key` set aside back
/// into the claim order, each at the due instant it had.
///
/// ```sql
/// UPDATE notification_deliveries SET held_at = NULL WHERE subscription_key = $1 AND held_at IS NOT NULL
/// ```
pub(crate) fn resume_statement(target: &ContextTarget, subscription_key: i64) -> UpdateStatement {
    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::HeldAt, null())
        .and_where(Expr::col(NotificationDeliveries::SubscriptionKey).eq(subscription_key))
        .and_where(Expr::col(NotificationDeliveries::HeldAt).is_not_null())
        .to_owned()
}

/// The hold whose probe an exhaustion acknowledges, locked for its hand-over:
///
/// ```sql
/// probed AS (SELECT subscription_key FROM notification_delivery_holds
///            WHERE delivery_key IN (SELECT delivery_key FROM acknowledged) FOR UPDATE)
/// ```
///
/// An exhausted delivery that is not a probe finds no hold naming it.
pub(crate) fn probed_on_exhaustion(target: &ContextTarget) -> CommonTableExpression {
    let probed = Query::select()
        .column(NotificationDeliveryHolds::SubscriptionKey)
        .from(table_of(target, NotificationDeliveryHolds::Table))
        .and_where(Expr::col(NotificationDeliveryHolds::DeliveryKey).in_subquery(acknowledged(NotificationDeliveries::DeliveryKey)))
        .lock(LockType::Update)
        .to_owned();
    cte(Probed, probed)
}

/// Hand the hold of `subscription_key` to the delivery it set aside first, due
/// at `next_probe_at`, or delete it when it set none aside.
///
/// ```sql
/// WITH successor AS (SELECT delivery_key FROM notification_deliveries WHERE subscription_key = $1 AND held_at IS NOT NULL
///                    ORDER BY held_at, delivery_key LIMIT 1),
///      handed_over AS (UPDATE notification_delivery_holds SET delivery_key = successor.delivery_key FROM successor
///                      WHERE notification_delivery_holds.subscription_key = $1),
///      dropped AS (DELETE FROM notification_delivery_holds WHERE subscription_key = $1 AND NOT EXISTS (SELECT 1 FROM successor)),
///      revived AS (UPDATE notification_deliveries SET held_at = NULL, next_attempt_at = $2 FROM successor
///                  WHERE notification_deliveries.delivery_key = successor.delivery_key)
/// SELECT delivery_key FROM successor
/// ```
///
/// The successor waits one retry step like any retry, so the endpoint still
/// sees one attempt per step across the hand-over.
pub(crate) fn hand_over_statement(target: &ContextTarget, subscription_key: i64, next_probe_at: DateTime<Utc>) -> WithQuery {
    let successor = Query::select()
        .column(NotificationDeliveries::DeliveryKey)
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(Expr::col(NotificationDeliveries::SubscriptionKey).eq(subscription_key))
        .and_where(Expr::col(NotificationDeliveries::HeldAt).is_not_null())
        .order_by(NotificationDeliveries::HeldAt, Order::Asc)
        .order_by(NotificationDeliveries::DeliveryKey, Order::Asc)
        .limit(1)
        .to_owned();
    let handed_over = Query::update()
        .table(table_of(target, NotificationDeliveryHolds::Table))
        .value(
            NotificationDeliveryHolds::DeliveryKey,
            Expr::col((Successor, NotificationDeliveries::DeliveryKey)),
        )
        .from(Successor)
        .and_where(Expr::col((NotificationDeliveryHolds::Table, NotificationDeliveryHolds::SubscriptionKey)).eq(subscription_key))
        .to_owned();
    let dropped = Query::delete()
        .from_table(table_of(target, NotificationDeliveryHolds::Table))
        .and_where(Expr::col(NotificationDeliveryHolds::SubscriptionKey).eq(subscription_key))
        .and_where(Expr::exists(Query::select().expr(Expr::val(1)).from(Successor).to_owned()).not())
        .to_owned();
    let revived = Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::HeldAt, null())
        .value(NotificationDeliveries::NextAttemptAt, next_probe_at)
        .from(Successor)
        .and_where(Expr::col((NotificationDeliveries::Table, NotificationDeliveries::DeliveryKey)).equals((Successor, NotificationDeliveries::DeliveryKey)))
        .to_owned();
    WithClause::new()
        .cte(cte(Successor, successor))
        .cte(cte(HandedOver, handed_over))
        .cte(cte(Dropped, dropped))
        .cte(cte(Revived, revived))
        .to_owned()
        .query(Query::select().column(NotificationDeliveries::DeliveryKey).from(Successor).to_owned())
}

/// `SELECT acknowledged.delivery_key, <hold>.<columns> FROM acknowledged <join> <hold> ON TRUE`:
/// the acknowledged row beside what its hold piece returned, inner-joined when
/// the piece returns a row for every acknowledged one and left-joined when it
/// may return none.
pub(crate) fn acknowledged_beside(hold: impl Iden + Copy + 'static, columns: &[NotificationDeliveryHolds], join: JoinType) -> SelectStatement {
    let mut select = Query::select();
    select.column((Acknowledged, NotificationDeliveries::DeliveryKey));
    for column in columns {
        select.column((hold, *column));
    }
    select.from(Acknowledged).join(join, hold, SimpleExpr::from(true));
    select
}

/// Insert the hold of the acknowledged delivery's Subscription, or return the
/// one it already has.
fn take_hold(target: &ContextTarget) -> Result<InsertStatement, ContextStoreError> {
    let mut insert = Query::insert();
    insert
        .into_table(table_of(target, NotificationDeliveryHolds::Table))
        .columns([NotificationDeliveryHolds::SubscriptionKey, NotificationDeliveryHolds::DeliveryKey])
        .select_from(
            Query::select()
                .columns([NotificationDeliveries::SubscriptionKey, NotificationDeliveries::DeliveryKey])
                .from(Acknowledged)
                .to_owned(),
        )?
        .on_conflict(
            OnConflict::column(NotificationDeliveryHolds::SubscriptionKey)
                .value(
                    NotificationDeliveryHolds::DeliveryKey,
                    Expr::col((NotificationDeliveryHolds::Table, NotificationDeliveryHolds::DeliveryKey)),
                )
                .to_owned(),
        )
        .returning(Query::returning().columns([NotificationDeliveryHolds::SubscriptionKey, NotificationDeliveryHolds::DeliveryKey]));
    Ok(insert)
}

/// `SELECT <column> FROM acknowledged`.
fn acknowledged(column: NotificationDeliveries) -> SelectStatement {
    Query::select().column(column).from(Acknowledged).to_owned()
}

/// The SQL `NULL` a cleared column takes.
const fn null() -> SimpleExpr {
    SimpleExpr::Keyword(Keyword::Null)
}

/// A common table expression named `name` over `query`.
fn cte(name: impl Iden + 'static, query: impl Into<SubQueryStatement>) -> CommonTableExpression {
    CommonTableExpression::new().table_name(name).query(query).to_owned()
}
