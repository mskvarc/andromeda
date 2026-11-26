//! The holds retryable failures take, and the deliveries they set aside.
//!
//! A failure's first statement takes the hold of each failed delivery's
//! Subscription, or finds and locks the one it has. The second, once per held
//! Subscription, sets aside every queued delivery of it but its probe and
//! those still in flight.

use crate::{
    error::ContextStoreError,
    notification_delivery::hold::{Acknowledged, Probe, cte, null, probe, probe_key, without_live_lease},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationDeliveryHolds};
use andromeda_postgresql_seaquery::scalar_subquery::scalar_subquery;
use sea_query::{CommonTableExpression, Expr, ExprTrait, Iden, InsertStatement, OnConflict, Query, WithClause, WithQuery};

/// The hold a failure took or found, `taken`.
#[derive(Iden, Clone, Copy)]
#[iden = "taken"]
pub(crate) struct Taken;

/// The probe taken back into the claim order, `taken_back`, whose only
/// purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "taken_back"]
struct TakenBack;

/// The holds retryable failures take, or find and lock:
///
/// ```sql
/// taken AS (INSERT INTO notification_delivery_holds (subscription_key)
///           SELECT DISTINCT subscription_key FROM acknowledged
///           ON CONFLICT (subscription_key) DO UPDATE SET subscription_key = notification_delivery_holds.subscription_key
///           RETURNING subscription_key)
/// ```
///
/// One row per Subscription, because a conflict may move a row only once per
/// statement. The conflict branch writes the key it finds back, so the
/// existing hold comes back and its row stays locked to the end of the
/// transaction. The hold names no delivery: the order in which failures are
/// recorded says nothing of the order of their changes, so the probe is read
/// from the queue whenever it is needed ([`set_aside_statement`]).
///
/// # Errors
///
/// Returns [`ContextStoreError::StatementBuild`] when the insert cannot take
/// its rows from the acknowledged deliveries.
pub(crate) fn taken_on_failure(target: &ContextTarget) -> Result<CommonTableExpression, ContextStoreError> {
    Ok(cte(Taken, take_hold(target)?))
}

/// Set aside every queued delivery of the held Subscription `subscription_key`
/// but its probe and those in flight under a live lease, which their own
/// outcomes settle, and take the probe back if it is set aside.
///
/// ```sql
/// WITH probe AS (<earliest_queued>),
///      taken_back AS (UPDATE notification_deliveries SET held_at = NULL FROM probe
///                     WHERE notification_deliveries.delivery_key = probe.delivery_key
///                       AND notification_deliveries.held_at IS NOT NULL)
/// UPDATE notification_deliveries SET held_at = CURRENT_TIMESTAMP, claimed_by = NULL, lease_until = NULL
/// WHERE subscription_key = $2 AND held_at IS NULL AND delivered_at IS NULL AND exhausted_at IS NULL
///   AND (lease_until IS NULL OR lease_until <= CURRENT_TIMESTAMP)
///   AND delivery_key <> (SELECT delivery_key FROM probe)
/// ```
///
/// The probe is the Subscription's queued delivery first in change order, in
/// flight or not, as the queue stands once the hold is locked. A failed
/// delivery is set aside with the rest when it is not the probe, and a lapsed
/// lease is cleared with the rest of its claim, which a held row may not
/// carry. A probe found set aside rejoins the claim order with the due instant
/// it had, so the Subscription keeps the one delivery a claim may take.
pub(crate) fn set_aside_statement(target: &ContextTarget, subscription_key: i64) -> WithQuery {
    let taken_back = Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::HeldAt, null())
        .from(Probe)
        .and_where(Expr::col((NotificationDeliveries::Table, NotificationDeliveries::DeliveryKey)).equals((Probe, NotificationDeliveries::DeliveryKey)))
        .and_where(Expr::col((NotificationDeliveries::Table, NotificationDeliveries::HeldAt)).is_not_null())
        .to_owned();
    let set_aside = Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::HeldAt, Expr::current_timestamp())
        .value(NotificationDeliveries::ClaimedBy, null())
        .value(NotificationDeliveries::LeaseUntil, null())
        .and_where(Expr::col(NotificationDeliveries::SubscriptionKey).eq(subscription_key))
        .and_where(Expr::col(NotificationDeliveries::HeldAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::DeliveredAt).is_null())
        .and_where(Expr::col(NotificationDeliveries::ExhaustedAt).is_null())
        .and_where(without_live_lease())
        .and_where(Expr::col(NotificationDeliveries::DeliveryKey).ne(scalar_subquery(probe_key())))
        .to_owned();
    WithClause::new()
        .cte(probe(target, subscription_key))
        .cte(cte(TakenBack, taken_back))
        .to_owned()
        .query(set_aside)
}

/// Insert the hold of each acknowledged delivery's Subscription, or return the
/// one it already has.
fn take_hold(target: &ContextTarget) -> Result<InsertStatement, ContextStoreError> {
    let mut insert = Query::insert();
    insert
        .into_table(table_of(target, NotificationDeliveryHolds::Table))
        .columns([NotificationDeliveryHolds::SubscriptionKey])
        .select_from(
            Query::select()
                .distinct()
                .column(NotificationDeliveries::SubscriptionKey)
                .from(Acknowledged)
                .to_owned(),
        )?
        .on_conflict(
            OnConflict::column(NotificationDeliveryHolds::SubscriptionKey)
                .value(
                    NotificationDeliveryHolds::SubscriptionKey,
                    Expr::col((NotificationDeliveryHolds::Table, NotificationDeliveryHolds::SubscriptionKey)),
                )
                .to_owned(),
        )
        .returning_col(NotificationDeliveryHolds::SubscriptionKey);
    Ok(insert)
}

#[cfg(test)]
mod tests {
    use crate::{notification_delivery::hold::take::set_aside_statement, statement_text::text_of};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;

    /// Every queued delivery of the held Subscription is set aside but its
    /// probe, the first in change order, and one in flight under a live
    /// lease, whose own outcome settles it; a lapsed lease is cleared, and a
    /// probe found set aside is taken back.
    #[test]
    fn a_failure_sets_aside_every_queued_delivery_of_its_subscription_but_its_probe() {
        assert_statement_snapshot!(
            "hold-a-failure-sets-aside-every-queued-delivery-of-its-subscription-but-its-probe",
            set_aside_statement(&ContextTarget::Live, 7)
        );
    }

    #[test]
    fn a_snapshot_branch_failure_sets_aside_the_deliveries_of_its_own_branch() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let branch = ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let sql = text_of(&set_aside_statement(&branch, 7));
        assert!(sql.contains(r#""taken_back" AS (UPDATE "snap_5"."notification_deliveries""#), "{sql}");
        assert!(sql.contains(r#") UPDATE "snap_5"."notification_deliveries""#), "{sql}");
        assert!(!sql.contains(r#"FROM "notification_deliveries""#), "{sql}");
    }
}
