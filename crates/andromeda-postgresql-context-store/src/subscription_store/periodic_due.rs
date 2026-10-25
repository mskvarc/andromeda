//! Which periodic subscriptions are owed a notification.
//!
//! Clause 5.8.6 sends a notification each time a subscription's `timeInterval`
//! is reached, but only while the subscription is in force: clause 5.8.1.4
//! states that once `expiresAt` has passed "notifications will no longer be
//! sent", and a paused subscription sends none. The expiry sweep that moves an
//! expired subscription to the expired state runs on its own cadence, so every
//! reader of the periodic schedule applies the expiry itself rather than
//! waiting for that sweep.

use crate::subscription_store::{
    delivery_state::{interval_anchor_select, interval_owed},
    projection::projected_select,
    status_code::status_is,
    stored_kind::{StoredKind, of_kind},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use chrono::{DateTime, Utc};
use sea_query::{Cond, Expr, ExprTrait, SelectStatement, SimpleExpr};

/// A subscription still in force at `at`: it has no `expiresAt`, or one after
/// `at`.
///
/// The store's pushdown of `SubscriptionStanding::unexpired_at`, with the
/// same boundary: the expiration instant itself is past.
pub(crate) fn unexpired_at(at: SimpleExpr) -> Cond {
    Cond::any()
        .add(Expr::col((Subscriptions::Table, Subscriptions::ExpiresAt)).is_null())
        .add(Expr::col((Subscriptions::Table, Subscriptions::ExpiresAt)).gt(at))
}

/// Every active, unexpired periodic subscription of `kind` owed a grid point
/// at `cutoff`, in the read-side projection.
pub(super) fn due_interval_select(kind: StoredKind, target: &ContextTarget, cutoff: DateTime<Utc>) -> SelectStatement {
    let mut select = projected_select(kind, target);
    select
        .and_where(status_is(SubscriptionStatus::Active))
        .and_where(of_kind(kind))
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::TimeInterval)).is_not_null())
        .cond_where(unexpired_at(Expr::val(cutoff)))
        .and_where(interval_owed(cutoff));
    select
}

/// The grid anchors, intervals and last answered sweep instants of every
/// active periodic subscription of `kind` still in force at `cutoff`, for
/// scheduling the next sweep.
pub(super) fn interval_anchors_select(kind: StoredKind, target: &ContextTarget, cutoff: DateTime<Utc>) -> SelectStatement {
    let mut select = interval_anchor_select(target);
    select
        .and_where(status_is(SubscriptionStatus::Active))
        .and_where(of_kind(kind))
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::TimeInterval)).is_not_null())
        .cond_where(unexpired_at(Expr::val(cutoff)));
    select
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::subscription_store::{
        periodic_due::{due_interval_select, interval_anchors_select},
        stored_kind::StoredKind,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use chrono::{TimeZone, Utc};
    use sea_query::PostgresQueryBuilder;

    const UNEXPIRED: &str = r#"("subscriptions"."expires_at" IS NULL OR "subscriptions"."expires_at" > '2026-01-02 03:04:05.000000 +00:00')"#;

    fn cutoff() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 2, 3, 4, 5).single().expect("a valid instant")
    }

    /// Clause 5.8.1.4: an expired subscription sends no notification, even
    /// before the expiry sweep has marked it expired.
    #[test]
    fn a_due_periodic_subscription_must_still_be_in_force_at_the_cutoff() {
        for kind in [StoredKind::Entity, StoredKind::Csource] {
            let sql = due_interval_select(kind, &ContextTarget::Live, cutoff()).to_string(PostgresQueryBuilder);
            assert!(sql.contains(UNEXPIRED), "{kind:?}: {sql}");
            assert!(
                sql.contains(r#""subscriptions"."status" = 1"#),
                "{kind:?}: only active subscriptions are due: {sql}"
            );
        }
    }

    #[test]
    fn the_next_periodic_wake_is_scheduled_only_for_subscriptions_in_force() {
        for kind in [StoredKind::Entity, StoredKind::Csource] {
            let sql = interval_anchors_select(kind, &ContextTarget::Live, cutoff()).to_string(PostgresQueryBuilder);
            assert!(sql.contains(UNEXPIRED), "{kind:?}: {sql}");
        }
    }
}
