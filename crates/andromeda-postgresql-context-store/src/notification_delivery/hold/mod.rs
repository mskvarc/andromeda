//! The hold a failing endpoint puts on the rest of its Subscription's queue.
//!
//! A retryable failure takes a hold on the Subscription and sets aside
//! (`held_at`) every queued delivery of it but its probe. The probe is the
//! Subscription's queued delivery first in change order
//! ([`NotificationDeliveries::CHANGE_ORDER`]), the change's commit instant and
//! write position, never the key alone, which a connection's cached block of
//! keys can give a later change. The hold names the Subscription alone: every
//! statement that needs the probe reads it from the queue, so however the
//! failures of one endpoint are grouped into acknowledgements, the probe is
//! the earliest change still owed. The claim then offers the Subscription's
//! probe alone, so an endpoint that never answers receives one attempt per
//! retry step however many deliveries wait for it, and the slots of the
//! tenant's drain stay free for every other Subscription.
//!
//! An attempt that reaches the endpoint deletes the hold and releases the
//! deliveries it set aside, each with the attempt count and the due instant it
//! had, so they go out in the order they were queued. A probe that runs out
//! of attempts passes the hold to the queued delivery of the next change, due
//! one retry step after the hand-over on the store's clock unless it is due
//! later already; a hold with nothing left queued is deleted.
//!
//! An acknowledgement changes the holds in two statements of its own
//! transaction. The first records the outcomes and locks the hold rows, by
//! taking, deleting or selecting them for update; the second sets deliveries
//! aside or releases them. Every change to a
//! Subscription's held deliveries is made under that lock, and the second
//! statement reads a snapshot taken once the lock is granted, so it sees every
//! delivery the previous holder of the lock set aside and finds the probe as
//! the queue stands then. One statement could not: its every part reads the
//! snapshot taken before it waited, and a release would leave behind, held
//! for good, the deliveries a failure committed while it waited. A delivery
//! not yet set aside is never lost that way, because the claim keeps it back
//! only while the hold exists.
//!
//! A delivery queued while its Subscription is held is written set aside,
//! under a key-share lock on the hold row, unless no queued delivery of the
//! Subscription notifies an earlier change: a write that began before the
//! changes already queued commits a change that sorts before them, and that
//! delivery is the probe. The release's deletion of the hold waits for the
//! lock, so the statement that takes the deliveries back sees the new row and
//! takes it back with the rest. A delivery queued while the hold is being
//! taken, or one a still earlier change displaces as the probe, reaches the
//! claim index unheld, and the claim's own probe test keeps it back until the
//! next failure sets it aside.

/// The hold an exhausted probe passes to the next change.
pub(crate) mod hand_over;
/// The holds delivered attempts release.
pub(crate) mod release;
/// The holds retryable failures take, and the deliveries they set aside.
pub(crate) mod take;

use crate::notification_delivery::change_order::earliest_queued;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationDeliveryHolds};
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, JoinType, Keyword, Query, SelectStatement, SimpleExpr, SubQueryStatement};

/// The acknowledged deliveries, `acknowledged`: the CTE every acknowledgement
/// opens with, returning each delivery's key, its Subscription's and where its
/// change falls in the Subscription's order.
#[derive(Iden, Clone, Copy)]
#[iden = "acknowledged"]
pub(crate) struct Acknowledged;

/// The probe of a held Subscription, `probe`.
#[derive(Iden, Clone, Copy)]
#[iden = "probe"]
struct Probe;

/// `SELECT acknowledged.delivery_key, <hold>.subscription_key FROM
/// acknowledged <join> <hold> ON <hold>.<on> = acknowledged.<on>`: each
/// acknowledged row beside the Subscription whose hold its hold piece moved
/// for it, matched on the Subscription or the delivery `on` names,
/// inner-joined when the piece returns a row for every acknowledged one and
/// left-joined when it may return none.
pub(crate) fn acknowledged_beside(hold: impl Iden + Copy + 'static, join: JoinType, on: NotificationDeliveries) -> SelectStatement {
    Query::select()
        .column((Acknowledged, NotificationDeliveries::DeliveryKey))
        .column((hold, NotificationDeliveryHolds::SubscriptionKey))
        .from(Acknowledged)
        .join(join, hold, Expr::col((hold, on)).equals((Acknowledged, on)))
        .to_owned()
}

/// The SQL `NULL` a cleared column takes.
pub(crate) const fn null() -> SimpleExpr {
    SimpleExpr::Keyword(Keyword::Null)
}

/// A common table expression named `name` over `query`.
pub(crate) fn cte(name: impl Iden + 'static, query: impl Into<SubQueryStatement>) -> CommonTableExpression {
    CommonTableExpression::new().table_name(name).query(query).to_owned()
}

/// `probe AS (<earliest_queued>)`: the probe of the held Subscription
/// `subscription_key`, as the snapshot of the statement that opens with it
/// shows the queue.
fn probe(target: &ContextTarget, subscription_key: i64) -> CommonTableExpression {
    cte(Probe, earliest_queued(target, subscription_key))
}

/// `SELECT delivery_key FROM probe`.
fn probe_key() -> SelectStatement {
    Query::select().column(NotificationDeliveries::DeliveryKey).from(Probe).to_owned()
}

/// `lease_until IS NULL OR lease_until <= CURRENT_TIMESTAMP`: the row is not
/// in flight under a live lease, so no outcome of its own is still to settle
/// it.
fn without_live_lease() -> SimpleExpr {
    Expr::col(NotificationDeliveries::LeaseUntil)
        .is_null()
        .or(Expr::col(NotificationDeliveries::LeaseUntil).lte(Expr::current_timestamp()))
}

/// `SELECT <column> FROM acknowledged`.
fn acknowledged(column: NotificationDeliveries) -> SelectStatement {
    Query::select().column(column).from(Acknowledged).to_owned()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::hold::acknowledged_beside;
    use andromeda_postgresql_schema::notification::NotificationDeliveries;
    use sea_query::{Iden, JoinType, PostgresQueryBuilder};

    /// A hold piece's CTE, `piece`.
    #[derive(Iden, Clone, Copy)]
    #[iden = "piece"]
    struct Piece;

    #[test]
    fn each_acknowledged_row_is_returned_beside_the_subscription_its_hold_piece_moved() {
        pretty_assertions::assert_eq!(
            acknowledged_beside(Piece, JoinType::LeftJoin, NotificationDeliveries::DeliveryKey).to_string(PostgresQueryBuilder),
            concat!(
                r#"SELECT "acknowledged"."delivery_key", "piece"."subscription_key" FROM "acknowledged" "#,
                r#"LEFT JOIN "piece" ON "piece"."delivery_key" = "acknowledged"."delivery_key""#
            )
        );
    }
}
