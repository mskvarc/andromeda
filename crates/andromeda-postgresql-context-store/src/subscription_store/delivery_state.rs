//! The `subscription_delivery_state` row: the clause 5.2.14.2 and 5.2.15
//! delivery counters of one subscription, and the statements that insert it,
//! record notification outcomes and interval answers on it, and read it back.
//!
//! The counters are kept off the subscription row so a delivery never rewrites
//! it. A read of a subscription joins the state row back in; a claim or an
//! outcome addresses the state row through the surrogate key the
//! subscription's `id` resolves to, so the wide row is read but never written
//! on the delivery path.

use crate::{
    store_fault::persistence_error,
    subscription_store::{
        status_code::status_is,
        stored_kind::{StoredKind, of_kind},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::subscriptions::{SubscriptionDeliveryState, Subscriptions};
use andromeda_postgresql_seaquery::{clock::now, interval::next_grid_point};
use andromeda_subscription::{error::SubscriptionError, outcome_tally::OutcomeTally};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Expr, ExprTrait, Func, InsertStatement, JoinType, Query, QueryStatementBuilder, SelectStatement, SimpleExpr, UpdateStatement};

/// Whether a state statement addresses only an active subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OwnerStatus {
    /// Only a subscription whose `status` is `active` owns the row addressed.
    ActiveOnly,
    /// Any subscription of the family owns the row addressed.
    Any,
}

/// The insert of the state row that completes a freshly created subscription,
/// under the surrogate key it minted, into the subscription's branch schema.
pub(crate) fn state_insert(target: &ContextTarget, subscription_key: i64) -> Result<InsertStatement, SubscriptionError> {
    let mut insert = Query::insert();
    insert
        .into_table(table_of(target, SubscriptionDeliveryState::Table))
        .columns([SubscriptionDeliveryState::SubscriptionKey])
        .values([Expr::val(subscription_key)])
        .map_err(|error| persistence_error::<SubscriptionError>(error))?;
    Ok(insert)
}

/// The scalar subquery resolving the state row's key from the subscription's
/// external `id`, family and, when asked, active status.
pub(crate) fn owner_key(target: &ContextTarget, id: &Identifier, kind: StoredKind, status: OwnerStatus) -> SimpleExpr {
    let mut owner = Query::select();
    owner
        .column((Subscriptions::Table, Subscriptions::SubscriptionKey))
        .from(table_of(target, Subscriptions::Table))
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::Id)).eq(id.as_str()))
        .and_where(of_kind(kind));
    match status {
        OwnerStatus::ActiveOnly => {
            owner.and_where(status_is(SubscriptionStatus::Active));
        }
        OwnerStatus::Any => {}
    }
    SimpleExpr::SubQuery(None, Box::new(owner.into_sub_query_statement()))
}

/// The one write recording the outcomes `tally` adds up on the state row,
/// whatever the subscription's status is by then: `last_success` at `NOW()`
/// when any notification succeeded, `last_failure` at `NOW()` when any failed.
///
/// Failures also advance `times_failed` by their count, which Table 5.2.14.2-1
/// defines as the "number of times an unsuccessful response (or timeout) has
/// been received when delivering the notification". Notifications the claim did
/// not stamp are counted here: `times_sent` advances by their count, and
/// `last_notification` moves to the latest of their claim instants unless a
/// notification claimed later already finished, since clause 5.8.6 makes it the
/// instant of the last notification sent.
pub(crate) fn outcome_statement(tally: OutcomeTally, target: &ContextTarget, id: &Identifier, kind: StoredKind) -> UpdateStatement {
    let mut statement = Query::update();
    statement
        .table(table_of(target, SubscriptionDeliveryState::Table))
        .and_where(Expr::col(SubscriptionDeliveryState::SubscriptionKey).eq(owner_key(target, id, kind, OwnerStatus::Any)));
    if tally.successes() > 0 {
        statement.value(SubscriptionDeliveryState::LastSuccess, now());
    }
    if tally.failures() > 0 {
        statement.value(SubscriptionDeliveryState::LastFailure, now()).value(
            SubscriptionDeliveryState::TimesFailed,
            Expr::col(SubscriptionDeliveryState::TimesFailed).add(i64::from(tally.failures())),
        );
    }
    if let Some(counted) = tally.counted() {
        statement
            .value(
                SubscriptionDeliveryState::TimesSent,
                Expr::col(SubscriptionDeliveryState::TimesSent).add(i64::from(counted.count.get())),
            )
            .value(
                SubscriptionDeliveryState::LastNotification,
                Func::greatest([Expr::col(SubscriptionDeliveryState::LastNotification), Expr::val(counted.latest)]),
            );
    }
    statement
}

/// Join the state row into a `subscriptions` select and project its counters
/// under the names the mapper reads.
///
/// The join is outer so a subscription whose state row is missing still reads;
/// `times_sent` and `times_failed` then coalesce to zero, the instants to NULL,
/// which is exactly what a never-notified subscription reports.
pub(crate) fn join_delivery_state(select: &mut SelectStatement, target: &ContextTarget) {
    select
        .join(
            JoinType::LeftJoin,
            table_of(target, SubscriptionDeliveryState::Table),
            Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::SubscriptionKey))
                .equals((Subscriptions::Table, Subscriptions::SubscriptionKey)),
        )
        .expr_as(
            Func::coalesce([
                Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::TimesSent)),
                Expr::val(0),
            ]),
            SubscriptionDeliveryState::TimesSent,
        )
        .expr_as(
            Func::coalesce([
                Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::TimesFailed)),
                Expr::val(0),
            ]),
            SubscriptionDeliveryState::TimesFailed,
        )
        .column((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastNotification))
        .column((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastSuccess))
        .column((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastFailure));
}

/// The first `timeInterval` grid point a periodic subscription has not yet
/// answered: `created_at + k * time_interval` for the least `k >= 1` past
/// `last_interval_tick`, or the first grid point when it has never answered.
///
/// The same grid the interval sweep schedules its wakes on, computed in the
/// statement so a sweep selects what is owed at its cutoff. Clause 5.8.6 sends a notification each time the interval is reached; the
/// creation instant alone fixes the grid, so a sweep that answered late owes
/// the next grid point on time.
fn owed_grid_point() -> SimpleExpr {
    let created = || Expr::col((Subscriptions::Table, Subscriptions::CreatedAt));
    let answered = Func::greatest([
        Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastIntervalTick)),
        created(),
    ]);
    next_grid_point(created(), Expr::col((Subscriptions::Table, Subscriptions::TimeInterval)), answered.into())
}

/// The due-interval predicate over a select that has the state row joined: a
/// periodic subscription whose owed grid point has been reached at `cutoff`,
/// the sweep's own instant rather than the wall clock, so a sweep decides the
/// set it was scheduled for.
pub(crate) fn interval_owed(cutoff: DateTime<Utc>) -> SimpleExpr {
    owed_grid_point().lte(Expr::val(cutoff))
}

/// Select every periodic subscription's grid anchor (its creation instant),
/// interval and last answered sweep instant, so a caller can compute when the
/// soonest one is next due.
///
/// The state row is outer-joined so a subscription whose state row is missing
/// still reads, as never answered. The caller narrows this to one family and to
/// active subscriptions with a non-null `time_interval`.
pub(crate) fn interval_anchor_select(target: &ContextTarget) -> SelectStatement {
    let mut select = Query::select();
    select
        .expr_as(Expr::col((Subscriptions::Table, Subscriptions::CreatedAt)), Alias::new("anchor"))
        .column((Subscriptions::Table, Subscriptions::TimeInterval))
        .column((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastIntervalTick))
        .from(table_of(target, Subscriptions::Table))
        .join(
            JoinType::LeftJoin,
            table_of(target, SubscriptionDeliveryState::Table),
            Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::SubscriptionKey))
                .equals((Subscriptions::Table, Subscriptions::SubscriptionKey)),
        );
    select
}

/// Record `cutoff` as the sweep instant one active periodic subscription
/// answered its grid at without a notification: nothing matched, so it is not
/// owed again before its next grid point, and `last_notification` keeps the
/// instant of the last notification sent (Table 5.2.14.2-1).
pub(crate) fn record_interval_tick_statement(target: &ContextTarget, id: &Identifier, kind: StoredKind, cutoff: DateTime<Utc>) -> UpdateStatement {
    let mut statement = Query::update();
    statement
        .table(table_of(target, SubscriptionDeliveryState::Table))
        .value(SubscriptionDeliveryState::LastIntervalTick, Expr::val(cutoff))
        .and_where(Expr::col(SubscriptionDeliveryState::SubscriptionKey).eq(owner_key(target, id, kind, OwnerStatus::ActiveOnly)));
    statement
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::{
        delivery_state::{interval_anchor_select, interval_owed, join_delivery_state, outcome_statement, record_interval_tick_statement, state_insert},
        delivery_state_fixture::{instant, subscription, target},
        stored_kind::StoredKind,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_postgresql_schema::subscriptions::Subscriptions;
    use andromeda_subscription::{
        notification_slot::{NotificationOutcome, OutcomeRecord},
        outcome_tally::OutcomeTally,
    };
    use chrono::{TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn outcome_sql(outcome: NotificationOutcome, record: OutcomeRecord) -> String {
        tally_sql(OutcomeTally::new(outcome, record))
    }

    fn tally_sql(tally: OutcomeTally) -> String {
        outcome_statement(tally, &target(), &subscription(), StoredKind::Entity).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_state_row_is_inserted_under_the_returned_surrogate_key() {
        let (sql, values) = state_insert(&ContextTarget::Live, 7).expect("builds").build(PostgresQueryBuilder);
        assert_eq!(sql, r#"INSERT INTO "subscription_delivery_state" ("subscription_key") VALUES ($1)"#);
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn an_outcome_stamps_one_outcome_column_whatever_the_subscription_status_is() {
        let success = outcome_sql(NotificationOutcome::Success, OutcomeRecord::OutcomeOnly);
        assert!(
            success.starts_with(r#"UPDATE "subscription_delivery_state" SET "last_success" = NOW() WHERE"#),
            "{success}"
        );
        assert!(!success.contains("status"), "{success}");
        assert!(!success.contains("times_failed"), "an accepted delivery moves no failure counter: {success}");
        let failure = outcome_sql(NotificationOutcome::Failure, OutcomeRecord::OutcomeOnly);
        assert!(failure.contains(r#""last_failure" = NOW()"#) && !failure.contains("last_success"), "{failure}");
    }

    /// Table 5.2.14.2-1: `timesFailed` is the "number of times an unsuccessful
    /// response (or timeout) has been received when delivering the
    /// notification", so only a failed delivery advances it.
    #[test]
    fn a_failed_delivery_advances_the_times_failed_counter() {
        let failure = outcome_sql(NotificationOutcome::Failure, OutcomeRecord::OutcomeOnly);
        assert!(failure.contains(r#""times_failed" = "times_failed" + 1"#), "{failure}");
        assert!(!failure.contains("times_sent"), "the attempt was already counted at claim time: {failure}");
    }

    /// Table 5.2.14.2-1 counts a refused notification as sent and failed, and
    /// clause 5.8.6 makes `lastNotification` the instant it was sent, so one
    /// write counts it, keeps the later of the stored and the claimed instant,
    /// and stamps the failure.
    #[test]
    fn an_outcome_counting_its_notification_moves_every_member_in_one_write() {
        let record = OutcomeRecord::CountingNotification { notified_at: instant(30) };
        let failure = outcome_sql(NotificationOutcome::Failure, record);
        assert_eq!(
            failure,
            r#"UPDATE "subscription_delivery_state" SET "last_failure" = NOW(), "times_failed" = "times_failed" + 1, "times_sent" = "times_sent" + 1, "last_notification" = GREATEST("last_notification", '2026-08-17 12:00:30.000000 +00:00') WHERE "subscription_key" = (SELECT "subscriptions"."subscription_key" FROM "subscriptions" WHERE "subscriptions"."id" = 'urn:ngsi-ld:Subscription:one' AND "subscriptions"."kind" = 1)"#
        );
        let success = outcome_sql(NotificationOutcome::Success, record);
        assert!(
            success.starts_with(r#"UPDATE "subscription_delivery_state" SET "last_success" = NOW(), "times_sent" = "times_sent" + 1"#),
            "{success}"
        );
        assert!(!success.contains("times_failed"), "{success}");
    }

    /// Outcomes recorded together move each counter by its count and each
    /// instant once: `lastNotification` to the latest claim among them.
    #[test]
    fn a_tally_of_several_outcomes_moves_every_member_by_its_count_in_one_write() {
        let mut tally = OutcomeTally::new(NotificationOutcome::Success, OutcomeRecord::CountingNotification { notified_at: instant(30) });
        tally.include(NotificationOutcome::Failure, OutcomeRecord::CountingNotification { notified_at: instant(45) });
        tally.include(NotificationOutcome::Success, OutcomeRecord::CountingNotification { notified_at: instant(40) });
        tally.include(NotificationOutcome::Failure, OutcomeRecord::OutcomeOnly);
        assert_eq!(
            tally_sql(tally),
            r#"UPDATE "subscription_delivery_state" SET "last_success" = NOW(), "last_failure" = NOW(), "times_failed" = "times_failed" + 2, "times_sent" = "times_sent" + 3, "last_notification" = GREATEST("last_notification", '2026-08-17 12:00:45.000000 +00:00') WHERE "subscription_key" = (SELECT "subscriptions"."subscription_key" FROM "subscriptions" WHERE "subscriptions"."id" = 'urn:ngsi-ld:Subscription:one' AND "subscriptions"."kind" = 1)"#
        );
    }

    #[test]
    fn a_projection_outer_joins_the_state_row_and_coalesces_the_counter() {
        let mut select = Query::select();
        select.column((Subscriptions::Table, Subscriptions::Id)).from(Subscriptions::Table);
        join_delivery_state(&mut select, &ContextTarget::Live);
        let sql = select.to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"LEFT JOIN "subscription_delivery_state" ON "subscription_delivery_state"."subscription_key" = "subscriptions"."subscription_key""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"COALESCE("subscription_delivery_state"."times_sent", 0) AS "times_sent""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"COALESCE("subscription_delivery_state"."times_failed", 0) AS "times_failed""#),
            "{sql}"
        );
        for column in ["last_notification", "last_success", "last_failure"] {
            assert!(sql.contains(&format!(r#""subscription_delivery_state"."{column}""#)), "{sql}");
        }
    }

    #[test]
    fn a_subscription_is_owed_the_first_grid_point_from_its_creation_past_its_last_answer() {
        let cutoff = Utc.with_ymd_and_hms(2026, 9, 20, 12, 0, 0).single().expect("valid instant");
        let sql = Query::select().and_where(interval_owed(cutoff)).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"("subscriptions"."created_at" + make_interval(secs => "subscriptions"."time_interval" * (floor(extract(epoch from (GREATEST("subscription_delivery_state"."last_interval_tick", "subscriptions"."created_at") - "subscriptions"."created_at")) / "subscriptions"."time_interval") + 1))) <= '2026-09-20 12:00:00.000000 +00:00'"#),
            "{sql}"
        );
        assert!(
            !sql.contains("last_notification"),
            "the grid never reads the instant of the last notification sent: {sql}"
        );
    }

    #[test]
    fn the_anchor_select_reads_the_creation_instant_and_the_last_answer() {
        let sql = interval_anchor_select(&ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""subscriptions"."created_at" AS "anchor""#), "{sql}");
        assert!(sql.contains(r#""subscriptions"."time_interval""#), "{sql}");
        assert!(sql.contains(r#""subscription_delivery_state"."last_interval_tick""#), "{sql}");
        assert!(
            sql.contains(r#"LEFT JOIN "subscription_delivery_state" ON "subscription_delivery_state"."subscription_key" = "subscriptions"."subscription_key""#),
            "{sql}"
        );
    }

    /// Table 5.2.14.2-1: `lastNotification` is the instant the last
    /// notification was sent, so a sweep that sent nothing leaves it alone.
    #[test]
    fn an_answer_without_a_notification_moves_only_the_tick_of_an_active_subscription() {
        let cutoff = Utc.with_ymd_and_hms(2026, 9, 20, 12, 0, 30).single().expect("valid instant");
        let sql = record_interval_tick_statement(&target(), &subscription(), StoredKind::Entity, cutoff).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"UPDATE "subscription_delivery_state" SET "last_interval_tick" = '2026-09-20 12:00:30.000000 +00:00' WHERE"#),
            "{sql}"
        );
        assert!(!sql.contains("times_sent"), "{sql}");
        assert!(!sql.contains("last_notification"), "{sql}");
        assert!(sql.contains(r#""subscriptions"."status" = 1"#), "{sql}");
    }
}
