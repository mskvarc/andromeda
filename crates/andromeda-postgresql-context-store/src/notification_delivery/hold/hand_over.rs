//! The hold an exhausted probe passes to the next change.
//!
//! An exhaustion's first statement finds, among the exhausted deliveries, the
//! probes of held Subscriptions and locks their holds. The second, once per
//! such Subscription, makes its queued delivery first in change order the next
//! probe, or deletes the hold when nothing of the Subscription is queued.

use crate::notification_delivery::{
    change_order::queued_earlier,
    hold::{Acknowledged, Probe, cte, null, probe, probe_key, without_live_lease},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_notification::delivery_backoff::BackoffStep;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationDeliveryHolds};
use andromeda_postgresql_seaquery::clock::span_after_transaction_start;
use sea_query::{CommonTableExpression, Expr, ExprTrait, Func, Iden, JoinType, LockType, Query, WithClause, WithQuery};

/// The held Subscription of each exhausted probe, locked for its hand-over,
/// `exhausted_probe`.
#[derive(Iden, Clone, Copy)]
#[iden = "exhausted_probe"]
pub(crate) struct ExhaustedProbe;

/// The hold row an exhausted probe locks, `held`.
#[derive(Iden, Clone, Copy)]
#[iden = "held"]
struct Held;

/// The hold of an exhausted probe nothing queued succeeds, `dropped`, whose
/// only purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "dropped"]
struct Dropped;

/// The next probe taken back into the claim order, `revived`, whose only
/// purpose is to run.
#[derive(Iden, Clone, Copy)]
#[iden = "revived"]
struct Revived;

/// The holds whose probes exhaustions acknowledge, locked for their
/// hand-over:
///
/// ```sql
/// exhausted_probe AS (SELECT acknowledged.delivery_key, held.subscription_key
///                     FROM acknowledged INNER JOIN notification_delivery_holds AS held
///                       ON held.subscription_key = acknowledged.subscription_key
///                     WHERE NOT <queued_earlier of acknowledged>
///                     FOR UPDATE OF held)
/// ```
///
/// An exhausted delivery was its held Subscription's probe when no queued
/// delivery of the Subscription notifies an earlier change, as the snapshot
/// the statement began with shows the queue: the exhausted rows are still
/// queued in it, so of several of one Subscription only the first in change
/// order passes, and one in flight beside the probe since before the hold
/// finds the probe ahead of it.
pub(crate) fn exhausted_probes(target: &ContextTarget) -> CommonTableExpression {
    let probes = Query::select()
        .column((Acknowledged, NotificationDeliveries::DeliveryKey))
        .column((Held, NotificationDeliveryHolds::SubscriptionKey))
        .from(Acknowledged)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, NotificationDeliveryHolds::Table),
            Held,
            Expr::col((Held, NotificationDeliveryHolds::SubscriptionKey)).equals((Acknowledged, NotificationDeliveries::SubscriptionKey)),
        )
        .and_where(queued_earlier(target, Acknowledged).not())
        .lock_with_tables(LockType::Update, [Held])
        .to_owned();
    cte(ExhaustedProbe, probes)
}

/// Pass the hold of `subscription_key` to its queued delivery first in change
/// order, due `probe_after` once the transaction began unless it is due later
/// already, or delete the hold when nothing of the Subscription is queued.
///
/// ```sql
/// WITH probe AS (<earliest_queued>),
///      dropped AS (DELETE FROM notification_delivery_holds WHERE subscription_key = $2
///                  AND NOT EXISTS (SELECT delivery_key FROM probe)),
///      revived AS (UPDATE notification_deliveries SET held_at = NULL,
///                    next_attempt_at = GREATEST(notification_deliveries.next_attempt_at,
///                                               CURRENT_TIMESTAMP + make_interval(secs => $3))
///                  FROM probe WHERE notification_deliveries.delivery_key = probe.delivery_key
///                    AND (lease_until IS NULL OR lease_until <= CURRENT_TIMESTAMP))
/// SELECT delivery_key FROM probe
/// ```
///
/// The next probe waits one retry step like any retry, so the endpoint still
/// sees one attempt per step across the hand-over, and one already due later
/// keeps its own step. One in flight under a live lease since before the hold
/// was taken is left to its own outcome. The hold row itself is left as it
/// is: it names no probe, so the exhaustion alone passes it on.
pub(crate) fn hand_over_statement(target: &ContextTarget, subscription_key: i64, probe_after: BackoffStep) -> WithQuery {
    let dropped = Query::delete()
        .from_table(table_of(target, NotificationDeliveryHolds::Table))
        .and_where(Expr::col(NotificationDeliveryHolds::SubscriptionKey).eq(subscription_key))
        .and_where(Expr::exists(probe_key()).not())
        .to_owned();
    let revived = Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::HeldAt, null())
        .value(
            NotificationDeliveries::NextAttemptAt,
            Func::greatest([
                Expr::col((NotificationDeliveries::Table, NotificationDeliveries::NextAttemptAt)),
                span_after_transaction_start(probe_after.duration()),
            ]),
        )
        .from(Probe)
        .and_where(Expr::col((NotificationDeliveries::Table, NotificationDeliveries::DeliveryKey)).equals((Probe, NotificationDeliveries::DeliveryKey)))
        .and_where(without_live_lease())
        .to_owned();
    WithClause::new()
        .cte(probe(target, subscription_key))
        .cte(cte(Dropped, dropped))
        .cte(cte(Revived, revived))
        .to_owned()
        .query(probe_key())
}

#[cfg(test)]
mod tests {
    use crate::{notification_delivery::hold::hand_over::hand_over_statement, statement_text::text_of};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use andromeda_notification::delivery_backoff::{BackoffStep, DeliveryBackoff};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_retry::schedule::BackoffSchedule;
    use std::time::Duration;

    fn first_step() -> BackoffStep {
        DeliveryBackoff::new(BackoffSchedule::constant(Duration::from_secs(5)))
            .expect("a schedule within the longest step")
            .first_step()
    }

    /// The hold passes to the queued delivery of the earliest change, by
    /// commit instant and write position, which rejoins the claim order one
    /// step after the transaction began, on the clock the claim compares it
    /// with, unless it is due later already; one in flight is left alone, the
    /// hold row is not rewritten, and with nothing queued the hold goes.
    #[test]
    fn an_exhausted_probe_passes_the_hold_to_the_earliest_queued_change_one_step_later() {
        assert_statement_snapshot!(
            "hold-an-exhausted-probe-passes-the-hold-to-the-earliest-queued-change-one-step-later",
            hand_over_statement(&ContextTarget::Live, 7, first_step())
        );
    }

    #[test]
    fn a_snapshot_branch_hand_over_touches_the_tables_of_its_own_branch() {
        let key = SnapshotKey::try_from(5).expect("positive key");
        let branch = ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let sql = text_of(&hand_over_statement(&branch, 7, first_step()));
        assert!(sql.contains(r#"DELETE FROM "snap_5"."notification_delivery_holds""#), "{sql}");
        assert!(sql.contains(r#"UPDATE "snap_5"."notification_deliveries""#), "{sql}");
        assert!(!sql.contains(r#"FROM "notification_deliveries""#), "{sql}");
        assert!(!sql.contains(r#"FROM "notification_delivery_holds""#), "{sql}");
    }
}
