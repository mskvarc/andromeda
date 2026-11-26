//! The holds delivered attempts release, and the deliveries they take back.
//!
//! The endpoint answered, whichever delivery reached it, so the Subscription's
//! queue flows again: the first statement deletes the hold, and the second
//! takes back every delivery it set aside.

use crate::notification_delivery::hold::{acknowledged, cte, null};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationDeliveryHolds};
use andromeda_postgresql_seaquery::pg_array::any;
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, Query, UpdateStatement};

/// The hold an attempt that reached the endpoint deleted, `released`.
#[derive(Iden, Clone, Copy)]
#[iden = "released"]
pub(crate) struct Released;

/// The holds delivered attempts delete:
///
/// ```sql
/// released AS (DELETE FROM notification_delivery_holds
///              WHERE subscription_key IN (SELECT subscription_key FROM acknowledged) RETURNING subscription_key)
/// ```
///
/// A Subscription that is not held deletes nothing.
pub(crate) fn released_on_delivery(target: &ContextTarget) -> CommonTableExpression {
    let released = Query::delete()
        .from_table(table_of(target, NotificationDeliveryHolds::Table))
        .and_where(Expr::col(NotificationDeliveryHolds::SubscriptionKey).in_subquery(acknowledged(NotificationDeliveries::SubscriptionKey)))
        .returning_col(NotificationDeliveryHolds::SubscriptionKey)
        .to_owned();
    cte(Released, released)
}

/// Take every delivery the released holds of `subscription_keys` set aside
/// back into the claim order, each at the due instant it had.
///
/// ```sql
/// UPDATE notification_deliveries SET held_at = NULL WHERE subscription_key = ANY($1) AND held_at IS NOT NULL
/// ```
pub(crate) fn resume_statement(target: &ContextTarget, subscription_keys: Vec<i64>) -> UpdateStatement {
    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::HeldAt, null())
        .and_where(Expr::col(NotificationDeliveries::SubscriptionKey).eq(any(Expr::val(subscription_keys))))
        .and_where(Expr::col(NotificationDeliveries::HeldAt).is_not_null())
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::{notification_delivery::hold::release::resume_statement, statement_text::text_of};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};

    /// The endpoint answered, so every released Subscription's deliveries
    /// rejoin the claim order, in one statement, with the due instants they
    /// had.
    #[test]
    fn the_released_holds_take_back_every_delivery_they_set_aside_at_once() {
        pretty_assertions::assert_eq!(
            text_of(&resume_statement(&ContextTarget::Live, vec![7, 9])),
            r#"UPDATE "notification_deliveries" SET "held_at" = NULL WHERE "subscription_key" = ANY($1) AND "held_at" IS NOT NULL"#
        );
    }

    #[test]
    fn a_snapshot_branch_release_takes_back_the_deliveries_of_its_own_branch() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let branch = ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let sql = text_of(&resume_statement(&branch, vec![7]));
        assert!(sql.starts_with(r#"UPDATE "snap_5"."notification_deliveries""#), "{sql}");
    }
}
