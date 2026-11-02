//! The `subscription_delivery_state` row: the clause 5.2.14.2 and 5.2.15
//! delivery counters of one subscription, and every statement that reads or
//! moves them.
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
use andromeda_subscription::{
    error::SubscriptionError,
    notification_slot::{NotificationSlotGate, NotificationStamp},
    outcome_tally::OutcomeTally,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, Cond, Expr, ExprTrait, Func, InsertStatement, JoinType, Query, QueryStatementBuilder, SelectStatement, SimpleExpr, UpdateStatement};

/// The statement claiming one notification slot, by when the gate stamps the
/// notification.
#[derive(Debug)]
pub enum SlotClaimStatement {
    /// The `UPDATE` that stamps the notification at the claim; zero rows
    /// affected is a refused slot.
    Stamping(UpdateStatement),
    /// The `SELECT NOW()` that only checks the slot may be taken; no row is a
    /// refused slot, and the instant read is the notification's.
    Checking(SelectStatement),
}

/// Whether a state statement addresses only an active subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerStatus {
    /// Only a subscription whose `status` is `active` owns the row addressed.
    ActiveOnly,
    /// Any subscription of the family owns the row addressed.
    Any,
}

/// The insert of the state row that completes a freshly created subscription,
/// under the surrogate key it minted, into the subscription's branch schema.
pub fn state_insert(target: &ContextTarget, subscription_key: i64) -> Result<InsertStatement, SubscriptionError> {
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
fn owner_key(target: &ContextTarget, id: &Identifier, kind: StoredKind, status: OwnerStatus) -> SimpleExpr {
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

/// `column IS NULL OR column <= ceiling`: a stored instant a slot ceiling
/// admits.
fn admitted_by(column: SubscriptionDeliveryState, ceiling: DateTime<Utc>) -> Cond {
    Cond::any().add(Expr::col(column).is_null()).add(Expr::col(column).lte(ceiling))
}

/// The claim of one notification slot on the state row of an active
/// subscription, gated by each ceiling the [`NotificationSlotGate`] carries.
/// A refused slot is how clause 5.2.12 `throttling` and Table 5.2.15-1
/// `cooldown` both stop a notification from being sent at all.
///
/// A gate stamping at the claim moves `times_sent + 1` and `last_notification =
/// NOW()` in the claim itself, because the next claim's throttling ceiling
/// reads that instant; a periodic claim also records the sweep instant it
/// answers the `timeInterval` grid at, which the next sweep reads. Any other
/// gate writes nothing: the claim reads `NOW()` as the notification's instant
/// and the outcome's write counts it, so the row is written once per
/// notification.
pub fn slot_claim_statement(target: &ContextTarget, id: &Identifier, kind: StoredKind, gate: NotificationSlotGate) -> SlotClaimStatement {
    let owner = Expr::col(SubscriptionDeliveryState::SubscriptionKey).eq(owner_key(target, id, kind, OwnerStatus::ActiveOnly));
    let cooldown = gate.last_failure.map(|ceiling| admitted_by(SubscriptionDeliveryState::LastFailure, ceiling));
    match gate.notification {
        NotificationStamp::Throttled { ceiling } => {
            let mut statement = stamping_claim(target, owner, cooldown);
            statement.cond_where(admitted_by(SubscriptionDeliveryState::LastNotification, ceiling));
            SlotClaimStatement::Stamping(statement)
        }
        NotificationStamp::Periodic { tick } => {
            let mut statement = stamping_claim(target, owner, cooldown);
            statement.value(SubscriptionDeliveryState::LastIntervalTick, Expr::val(tick));
            SlotClaimStatement::Stamping(statement)
        }
        NotificationStamp::WithOutcome => SlotClaimStatement::Checking(checking_claim(target, owner, cooldown, None)),
        NotificationStamp::Unstamped { ceiling } => SlotClaimStatement::Checking(checking_claim(target, owner, cooldown, ceiling)),
    }
}

/// The `SELECT NOW()` that reads whether the `owner` row admits a notification,
/// refused inside the endpoint's `cooldown` window and, when a `throttling`
/// ceiling is given, inside the subscription's `throttling` window.
fn checking_claim(target: &ContextTarget, owner: SimpleExpr, cooldown: Option<Cond>, throttling: Option<DateTime<Utc>>) -> SelectStatement {
    let mut statement = Query::select();
    statement.expr(now()).from(table_of(target, SubscriptionDeliveryState::Table)).and_where(owner);
    if let Some(cooldown) = cooldown {
        statement.cond_where(cooldown);
    }
    if let Some(ceiling) = throttling {
        statement.cond_where(admitted_by(SubscriptionDeliveryState::LastNotification, ceiling));
    }
    statement
}

/// The `UPDATE` counting a notification at its claim on the `owner` row,
/// refused inside the endpoint's `cooldown` window when one is set.
fn stamping_claim(target: &ContextTarget, owner: SimpleExpr, cooldown: Option<Cond>) -> UpdateStatement {
    let mut statement = Query::update();
    statement
        .table(table_of(target, SubscriptionDeliveryState::Table))
        .value(SubscriptionDeliveryState::TimesSent, Expr::col(SubscriptionDeliveryState::TimesSent).add(1))
        .value(SubscriptionDeliveryState::LastNotification, now())
        .and_where(owner);
    if let Some(cooldown) = cooldown {
        statement.cond_where(cooldown);
    }
    statement
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
pub fn outcome_statement(tally: OutcomeTally, target: &ContextTarget, id: &Identifier, kind: StoredKind) -> UpdateStatement {
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
pub fn join_delivery_state(select: &mut SelectStatement, target: &ContextTarget) {
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
pub fn interval_owed(cutoff: DateTime<Utc>) -> SimpleExpr {
    owed_grid_point().lte(Expr::val(cutoff))
}

/// Select every periodic subscription's grid anchor (its creation instant),
/// interval and last answered sweep instant, so a caller can compute when the
/// soonest one is next due.
///
/// The state row is outer-joined so a subscription whose state row is missing
/// still reads, as never answered. The caller narrows this to one family and to
/// active subscriptions with a non-null `time_interval`.
pub fn interval_anchor_select(target: &ContextTarget) -> SelectStatement {
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
pub fn record_interval_tick_statement(target: &ContextTarget, id: &Identifier, kind: StoredKind, cutoff: DateTime<Utc>) -> UpdateStatement {
    let mut statement = Query::update();
    statement
        .table(table_of(target, SubscriptionDeliveryState::Table))
        .value(SubscriptionDeliveryState::LastIntervalTick, Expr::val(cutoff))
        .and_where(Expr::col(SubscriptionDeliveryState::SubscriptionKey).eq(owner_key(target, id, kind, OwnerStatus::ActiveOnly)));
    statement
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::subscription_store::{
        delivery_state::{
            SlotClaimStatement,
            interval_anchor_select,
            interval_owed,
            join_delivery_state,
            outcome_statement,
            record_interval_tick_statement,
            slot_claim_statement,
            state_insert,
        },
        stored_kind::StoredKind,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, positive_milliseconds::PositiveMilliseconds};
    use andromeda_postgresql_schema::subscriptions::Subscriptions;
    use andromeda_subscription::{
        notification_slot::{NotificationCadence, NotificationOutcome, NotificationSlotGate, OutcomeRecord},
        outcome_tally::OutcomeTally,
    };
    use chrono::{DateTime, TimeZone, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    const UNTHROTTLED: NotificationCadence = NotificationCadence::OnChange { throttling_secs: None };

    const THROTTLED: NotificationCadence = NotificationCadence::OnChange { throttling_secs: Some(30.0) };

    fn subscription() -> Identifier {
        "urn:ngsi-ld:Subscription:one".parse().expect("valid")
    }

    fn target() -> ContextTarget {
        ContextTarget::Live
    }

    fn instant(seconds: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, seconds).single().expect("valid instant")
    }

    fn claim_sql(kind: StoredKind, gate: NotificationSlotGate) -> String {
        match slot_claim_statement(&target(), &subscription(), kind, gate) {
            SlotClaimStatement::Stamping(statement) => statement.to_string(PostgresQueryBuilder),
            SlotClaimStatement::Checking(statement) => statement.to_string(PostgresQueryBuilder),
        }
    }

    /// A probe reads the throttling and cooldown windows and writes nothing.
    #[test]
    fn an_unstamped_claim_selects_under_both_windows_and_updates_nothing() {
        let cooldown = PositiveMilliseconds::try_from(2_000).expect("positive");
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), THROTTLED, Some(cooldown)).unstamped());
        assert!(sql.starts_with("SELECT NOW()"), "{sql}");
        assert!(!sql.contains("UPDATE"), "{sql}");
        assert!(sql.contains("last_notification"), "{sql}");
        assert!(sql.contains("last_failure"), "{sql}");
    }

    fn outcome_sql(outcome: NotificationOutcome, record: OutcomeRecord) -> String {
        tally_sql(OutcomeTally::new(outcome, record))
    }

    fn tally_sql(tally: OutcomeTally) -> String {
        outcome_statement(tally, &target(), &subscription(), StoredKind::Entity).to_string(PostgresQueryBuilder)
    }

    const ACTIVE_OWNER: &str = r#"WHERE "subscription_key" = (SELECT "subscriptions"."subscription_key" FROM "subscriptions" WHERE "subscriptions"."id" = 'urn:ngsi-ld:Subscription:one' AND "subscriptions"."kind" = 1 AND "subscriptions"."status" = 1)"#;

    #[test]
    fn the_state_row_is_inserted_under_the_returned_surrogate_key() {
        let (sql, values) = state_insert(&ContextTarget::Live, 7).expect("builds").build(PostgresQueryBuilder);
        assert_eq!(sql, r#"INSERT INTO "subscription_delivery_state" ("subscription_key") VALUES ($1)"#);
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn a_stamping_claim_moves_only_the_state_row_of_an_active_subscription() {
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), NotificationCadence::Periodic, None));
        assert!(
            sql.starts_with(r#"UPDATE "subscription_delivery_state" SET "times_sent" = "times_sent" + 1, "last_notification" = NOW()"#),
            "{sql}"
        );
        assert!(sql.contains(ACTIVE_OWNER), "{sql}");
        assert!(
            !sql.contains("modified_at"),
            "a delivery must not touch the subscription's modification instant: {sql}"
        );
        assert!(!sql.contains(r#"UPDATE "subscriptions""#), "{sql}");
        assert!(
            !sql.contains("last_notification\" IS NULL"),
            "a periodic claim carries no throttling ceiling: {sql}"
        );
    }

    /// The claim records the sweep instant the notification answers the grid
    /// at, so the next sweep owes the following grid point however late this
    /// one ran (clause 5.8.6).
    #[test]
    fn a_periodic_claim_records_the_sweep_instant_it_answers_the_grid_at() {
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), NotificationCadence::Periodic, None));
        assert!(sql.contains(r#""last_interval_tick" = '2026-08-17 12:00:30.000000 +00:00'"#), "{sql}");
    }

    /// Clause 5.8.6 moves `timesSent` and `lastNotification` for every
    /// notification sent; a claim nothing decides by `lastNotification` leaves
    /// them to the outcome's write and only reads the instant it is taken at.
    #[test]
    fn an_unthrottled_claim_reads_the_instant_of_an_active_subscription_without_writing() {
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), UNTHROTTLED, None));
        assert_eq!(sql, format!(r#"SELECT NOW() FROM "subscription_delivery_state" {ACTIVE_OWNER}"#));
    }

    #[test]
    fn a_throttled_claim_is_gated_on_the_last_notification_instant() {
        let sql = claim_sql(StoredKind::Csource, NotificationSlotGate::at(instant(30), THROTTLED, None));
        assert!(sql.starts_with(r#"UPDATE "subscription_delivery_state" SET "times_sent""#), "{sql}");
        assert!(
            sql.contains(r#"("last_notification" IS NULL OR "last_notification" <= '2026-08-17 12:00:00.000000 +00:00')"#),
            "{sql}"
        );
        assert!(!sql.contains("last_failure"), "an endpoint without a cooldown adds no second gate: {sql}");
        assert!(sql.contains(r#""subscriptions"."kind" = 2"#), "{sql}");
    }

    /// Table 5.2.15-1: "If requests are received before the cooldown period has
    /// expired, no notification is sent", so the claim itself is narrowed by
    /// the endpoint's cooldown window and a refused claim sends nothing.
    #[test]
    fn a_cooldown_gates_the_claim_on_the_last_failure_instant() {
        let cooldown = PositiveMilliseconds::try_from(5_000).expect("a positive millisecond count");
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(5), UNTHROTTLED, Some(cooldown)));
        assert!(sql.starts_with(r#"SELECT NOW() FROM "subscription_delivery_state""#), "{sql}");
        assert!(
            sql.contains(r#"("last_failure" IS NULL OR "last_failure" <= '2026-08-17 12:00:00.000000 +00:00')"#),
            "{sql}"
        );
        assert!(
            !sql.contains("last_notification\" IS NULL"),
            "an unthrottled subscription adds no first gate: {sql}"
        );
    }

    #[test]
    fn a_cooldown_gates_a_periodic_claim_on_the_last_failure_instant() {
        let cooldown = PositiveMilliseconds::try_from(5_000).expect("a positive millisecond count");
        let sql = claim_sql(
            StoredKind::Entity,
            NotificationSlotGate::at(instant(5), NotificationCadence::Periodic, Some(cooldown)),
        );
        assert!(sql.starts_with(r#"UPDATE "subscription_delivery_state""#), "{sql}");
        assert!(sql.contains(r#""last_failure" <= '2026-08-17 12:00:00.000000 +00:00'"#), "{sql}");
    }

    #[test]
    fn throttling_and_cooldown_gate_the_same_claim_independently() {
        let cooldown = PositiveMilliseconds::try_from(10_000).expect("a positive millisecond count");
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), THROTTLED, Some(cooldown)));
        assert!(sql.contains(r#""last_notification" <= '2026-08-17 12:00:00.000000 +00:00'"#), "{sql}");
        assert!(sql.contains(r#""last_failure" <= '2026-08-17 12:00:20.000000 +00:00'"#), "{sql}");
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
