//! Where the change a delivery notifies falls in its Subscription's order,
//! and the tests the queue runs on that order.
//!
//! A Subscription's deliveries are ordered by the changes they notify
//! ([`NotificationDeliveries::CHANGE_ORDER`]): the commit instant, then the
//! write position, then the key, never the key alone, which a connection's
//! cached block of keys can give a later change. A throttled delivery gives
//! way to one of a later change, and a held Subscription's probe is its queued
//! delivery first in that order.
//!
//! `idx_notification_deliveries_change_order` holds the queued rows of each
//! Subscription in that order, so every test here is one range scan of it
//! that stops at the first row it finds. Each spells the index's predicate
//! (`delivered_at` and `exhausted_at` NULL), because PostgreSQL proves a
//! partial index's predicate from a statement's own conditions alone.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::NotificationDeliveries;
use sea_query::{Expr, ExprTrait, Iden, Order, Query, SelectStatement, SimpleExpr};

/// A queued delivery of an earlier change, `earlier`.
#[derive(Iden, Clone, Copy)]
#[iden = "earlier"]
struct Earlier;

/// A queued delivery of a later change, `later`.
#[derive(Iden, Clone, Copy)]
#[iden = "later"]
struct Later;

/// Whether a delivery of the Subscription of `delivery`'s row notifying an
/// earlier change is still queued, held or not, leased or not:
///
/// ```sql
/// EXISTS (SELECT 1 FROM notification_deliveries AS earlier
///         WHERE earlier.subscription_key = <delivery>.subscription_key
///           AND earlier.delivered_at IS NULL AND earlier.exhausted_at IS NULL
///           AND (earlier.committed_at, earlier.event_ordinal, earlier.delivery_key)
///             < (<delivery>.committed_at, <delivery>.event_ordinal, <delivery>.delivery_key))
/// ```
///
/// A queued row of a held Subscription is its probe exactly when there is
/// none.
pub(crate) fn queued_earlier(target: &ContextTarget, delivery: impl Iden + Copy + 'static) -> SimpleExpr {
    Expr::exists(
        queued_of(target, Earlier, Expr::col((delivery, NotificationDeliveries::SubscriptionKey)))
            .and_where(change_position(Earlier).lt(change_position(delivery)))
            .to_owned(),
    )
}

/// Whether a delivery of the Subscription of `delivery`'s row notifying a
/// later change is still queued, held or not:
///
/// ```sql
/// EXISTS (SELECT 1 FROM notification_deliveries AS later
///         WHERE later.subscription_key = <delivery>.subscription_key
///           AND later.delivered_at IS NULL AND later.exhausted_at IS NULL
///           AND (later.committed_at, later.event_ordinal, later.delivery_key)
///             > (<delivery>.committed_at, <delivery>.event_ordinal, <delivery>.delivery_key))
/// ```
pub(crate) fn queued_later(target: &ContextTarget, delivery: impl Iden + Copy + 'static) -> SimpleExpr {
    Expr::exists(
        queued_of(target, Later, Expr::col((delivery, NotificationDeliveries::SubscriptionKey)))
            .and_where(change_position(Later).gt(change_position(delivery)))
            .to_owned(),
    )
}

/// Whether a delivery of `subscription_key` is still queued whose change
/// precedes, by commit instant and then write position, a change committed
/// at `committed_at` in write position `event_ordinal`:
///
/// ```sql
/// EXISTS (SELECT 1 FROM notification_deliveries AS earlier
///         WHERE earlier.subscription_key = <subscription_key>
///           AND earlier.delivered_at IS NULL AND earlier.exhausted_at IS NULL
///           AND (earlier.committed_at, earlier.event_ordinal) < (<committed_at>, <event_ordinal>))
/// ```
///
/// The comparison leaves out the key, so it serves a delivery not yet
/// written, whose key is drawn only as it is inserted.
pub(crate) fn queued_before_change(target: &ContextTarget, subscription_key: SimpleExpr, committed_at: SimpleExpr, event_ordinal: SimpleExpr) -> SimpleExpr {
    let earlier = Expr::tuple([
        Expr::col((Earlier, NotificationDeliveries::CommittedAt)),
        Expr::col((Earlier, NotificationDeliveries::EventOrdinal)),
    ]);
    Expr::exists(
        queued_of(target, Earlier, subscription_key)
            .and_where(earlier.lt(Expr::tuple([committed_at, event_ordinal])))
            .to_owned(),
    )
}

/// `SELECT delivery_key FROM notification_deliveries WHERE subscription_key = $1
/// AND delivered_at IS NULL AND exhausted_at IS NULL ORDER BY committed_at,
/// event_ordinal, delivery_key LIMIT 1`: the queued delivery of
/// `subscription_key` first in change order, held or not, leased or not,
/// which is its probe while the Subscription is held.
pub(crate) fn earliest_queued(target: &ContextTarget, subscription_key: i64) -> SelectStatement {
    Query::select()
        .column(NotificationDeliveries::DeliveryKey)
        .from(table_of(target, NotificationDeliveries::Table))
        .and_where(Expr::col(NotificationDeliveries::SubscriptionKey).eq(subscription_key))
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::ExhaustedAt).is_null())
        .order_by_columns(NotificationDeliveries::CHANGE_ORDER.map(|column| (column, Order::Asc)))
        .limit(1)
        .to_owned()
}

/// `SELECT 1 FROM notification_deliveries AS <alias> WHERE
/// <alias>.subscription_key = <subscription_key> AND <alias>.delivered_at IS
/// NULL AND <alias>.exhausted_at IS NULL`: the queued deliveries of one
/// Subscription in `target`'s branch, for a test to narrow by change order.
fn queued_of(target: &ContextTarget, alias: impl Iden + Copy + 'static, subscription_key: SimpleExpr) -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from(table_of(target, NotificationDeliveries::Table).alias(alias))
        .and_where(Expr::col((alias, NotificationDeliveries::SubscriptionKey)).eq(subscription_key))
        .and_where(Expr::col((alias, NotificationDeliveries::DeliveredAt)).is_null())
        .and_where(Expr::col((alias, NotificationDeliveries::ExhaustedAt)).is_null())
        .to_owned()
}

/// `(<table>.committed_at, <table>.event_ordinal, <table>.delivery_key)`:
/// where the change a delivery of `table` notifies falls in its
/// Subscription's order.
fn change_position(table: impl Iden + Copy + 'static) -> Expr {
    Expr::tuple(NotificationDeliveries::CHANGE_ORDER.map(|column| Expr::col((table, column))))
}

#[cfg(test)]
mod tests {
    use crate::{
        notification_delivery::change_order::{change_position, earliest_queued, queued_before_change, queued_earlier, queued_later},
        statement_text::text_of,
    };
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_postgresql_schema::notification::NotificationDeliveries;
    use sea_query::{Alias, Expr, Query};

    /// The text of `expr` as the one column of a select.
    fn text_of_expr(expr: Expr) -> String {
        text_of(&Query::select().expr(expr).to_owned())
    }

    fn snapshot() -> ContextTarget {
        let key = SnapshotKey::try_from(5).expect("positive key");
        ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")))
    }

    #[test]
    fn the_change_position_is_the_commit_instant_then_the_write_position_then_the_key() {
        pretty_assertions::assert_eq!(
            text_of_expr(change_position(NotificationDeliveries::Table)),
            r#"SELECT ("notification_deliveries"."committed_at", "notification_deliveries"."event_ordinal", "notification_deliveries"."delivery_key")"#
        );
    }

    /// Earlier is strictly before in change order, so a row is never earlier
    /// than itself, and the test reads only queued rows, which the change
    /// order index holds.
    #[test]
    fn an_earlier_queued_delivery_is_one_strictly_before_in_change_order() {
        pretty_assertions::assert_eq!(
            text_of_expr(queued_earlier(&ContextTarget::Live, NotificationDeliveries::Table)),
            concat!(
                r#"SELECT EXISTS(SELECT $1 FROM "notification_deliveries" AS "earlier" "#,
                r#"WHERE "earlier"."subscription_key" = "notification_deliveries"."subscription_key" "#,
                r#"AND "earlier"."delivered_at" IS NULL AND "earlier"."exhausted_at" IS NULL "#,
                r#"AND ("earlier"."committed_at", "earlier"."event_ordinal", "earlier"."delivery_key") "#,
                r#"< ("notification_deliveries"."committed_at", "notification_deliveries"."event_ordinal", "notification_deliveries"."delivery_key"))"#
            )
        );
    }

    #[test]
    fn a_later_queued_delivery_is_one_strictly_after_in_change_order() {
        pretty_assertions::assert_eq!(
            text_of_expr(queued_later(&ContextTarget::Live, NotificationDeliveries::Table)),
            concat!(
                r#"SELECT EXISTS(SELECT $1 FROM "notification_deliveries" AS "later" "#,
                r#"WHERE "later"."subscription_key" = "notification_deliveries"."subscription_key" "#,
                r#"AND "later"."delivered_at" IS NULL AND "later"."exhausted_at" IS NULL "#,
                r#"AND ("later"."committed_at", "later"."event_ordinal", "later"."delivery_key") "#,
                r#"> ("notification_deliveries"."committed_at", "notification_deliveries"."event_ordinal", "notification_deliveries"."delivery_key"))"#
            )
        );
    }

    /// A delivery not yet written has no key, so the test compares the
    /// commit instant and the write position alone.
    #[test]
    fn a_delivery_queued_before_a_change_precedes_its_commit_instant_then_its_write_position() {
        pretty_assertions::assert_eq!(
            text_of_expr(queued_before_change(
                &ContextTarget::Live,
                Expr::col((Alias::new("s"), NotificationDeliveries::SubscriptionKey)),
                Expr::current_timestamp(),
                Expr::col((Alias::new("p"), NotificationDeliveries::EventOrdinal)),
            )),
            concat!(
                r#"SELECT EXISTS(SELECT $1 FROM "notification_deliveries" AS "earlier" "#,
                r#"WHERE "earlier"."subscription_key" = "s"."subscription_key" "#,
                r#"AND "earlier"."delivered_at" IS NULL AND "earlier"."exhausted_at" IS NULL "#,
                r#"AND ("earlier"."committed_at", "earlier"."event_ordinal") < (CURRENT_TIMESTAMP, "p"."event_ordinal"))"#
            )
        );
    }

    #[test]
    fn the_earliest_queued_delivery_is_the_first_of_the_subscriptions_queue_in_change_order() {
        pretty_assertions::assert_eq!(
            text_of(&earliest_queued(&ContextTarget::Live, 7)),
            concat!(
                r#"SELECT "delivery_key" FROM "notification_deliveries" WHERE "subscription_key" = $1 "#,
                r#"AND "delivered_at" IS NULL AND "exhausted_at" IS NULL "#,
                r#"ORDER BY "committed_at" ASC, "event_ordinal" ASC, "delivery_key" ASC LIMIT 1"#
            )
        );
    }

    #[test]
    fn every_change_order_test_of_a_snapshot_branch_reads_its_own_queue() {
        for sql in [
            text_of_expr(queued_earlier(&snapshot(), NotificationDeliveries::Table)),
            text_of_expr(queued_later(&snapshot(), NotificationDeliveries::Table)),
            text_of(&earliest_queued(&snapshot(), 7)),
        ] {
            assert!(sql.contains(r#"FROM "snap_5"."notification_deliveries""#), "{sql}");
        }
    }
}
