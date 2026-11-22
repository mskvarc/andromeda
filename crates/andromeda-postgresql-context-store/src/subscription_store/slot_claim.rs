//! The claim of one notification slot on a subscription's delivery state
//! row: whether the clause 5.2.12 `throttling` window and the Table 5.2.15-1
//! `cooldown` window admit a notification now, and the stamps they were
//! judged on.

use crate::subscription_store::{
    delivery_state::{OwnerStatus, owner_key},
    stored_kind::StoredKind,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::subscriptions::SubscriptionDeliveryState;
use andromeda_postgresql_seaquery::clock::now;
use andromeda_subscription::notification_slot::{DeliveryStamps, NotificationSlot, NotificationSlotGate, NotificationStamp, OutcomeRecord, Suppression};
use chrono::{DateTime, Utc};
use sea_query::{CommonTableExpression, Cond, Expr, ExprTrait, Iden, Query, SelectStatement, SimpleExpr, WithClause, WithQuery};

/// The statement claiming one notification slot, by when the gate stamps the
/// notification. Either answers one [`SlotClaimRow`] for an active
/// subscription and no row for any other.
#[derive(Debug)]
pub(crate) enum SlotClaimStatement {
    /// The `UPDATE` that stamps the notification at the claim, beside the
    /// read of the stamps it was gated on.
    Stamping(WithQuery),
    /// The `SELECT` that reads whether the gate admits a notification, with
    /// the stamps it was judged on, and writes nothing.
    Checking(SelectStatement),
}

impl SlotClaimStatement {
    /// What the outcome of a notification this claim admitted at
    /// `claimed_at` still records: nothing more after a stamping claim, the
    /// notification itself after a checking one.
    const fn admitted_record(&self, claimed_at: DateTime<Utc>) -> OutcomeRecord {
        match self {
            SlotClaimStatement::Stamping(_) => OutcomeRecord::OutcomeOnly,
            SlotClaimStatement::Checking(_) => OutcomeRecord::CountingNotification { notified_at: claimed_at },
        }
    }
}

/// One claim's answer for an active subscription, as both claim statements
/// return it.
#[derive(Debug, sqlx::FromRow)]
pub(crate) struct SlotClaimRow {
    /// `NOW()` at the claim, the instant of a notification it admitted.
    claimed_at: DateTime<Utc>,
    /// Whether every ceiling of the gate admitted the claim; for a stamping
    /// claim, whether it stamped.
    admitted: bool,
    /// The stored `last_notification` the throttling ceiling was compared
    /// with.
    last_notification: Option<DateTime<Utc>>,
    /// The stored `last_failure` the cooldown ceiling was compared with.
    last_failure: Option<DateTime<Utc>>,
}

impl SlotClaimRow {
    /// The slot this answer to `statement` is.
    pub(crate) const fn into_slot(self, statement: &SlotClaimStatement) -> NotificationSlot {
        if self.admitted {
            NotificationSlot::Claimed(statement.admitted_record(self.claimed_at))
        } else {
            NotificationSlot::Suppressed(Suppression::Windowed(DeliveryStamps {
                last_notification: self.last_notification,
                last_failure: self.last_failure,
            }))
        }
    }
}

/// The instant a claim was answered at, `claimed_at`.
#[derive(Iden, Clone, Copy)]
#[iden = "claimed_at"]
struct ClaimedAt;

/// Whether the gate admitted the claim, `admitted`.
#[derive(Iden, Clone, Copy)]
#[iden = "admitted"]
struct Admitted;

/// The state row a stamping claim is gated on, as it stood, `gated`.
#[derive(Iden, Clone, Copy)]
#[iden = "gated"]
struct Gated;

/// The state row a stamping claim stamped, `stamped`.
#[derive(Iden, Clone, Copy)]
#[iden = "stamped"]
struct Stamped;

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
///
/// Either way the answer carries the stamps the ceilings were compared with,
/// so a refusal says which window refused it without a second statement.
pub(crate) fn slot_claim_statement(target: &ContextTarget, id: &Identifier, kind: StoredKind, gate: NotificationSlotGate) -> SlotClaimStatement {
    let owner = || Expr::col(SubscriptionDeliveryState::SubscriptionKey).eq(owner_key(target, id, kind, OwnerStatus::ActiveOnly));
    let mut admits = Cond::all();
    if let Some(ceiling) = gate.last_failure {
        admits = admits.add(admitted_by(SubscriptionDeliveryState::LastFailure, ceiling));
    }
    let throttled = |admits: Cond, ceiling: Option<DateTime<Utc>>| match ceiling {
        Some(ceiling) => admits.add(admitted_by(SubscriptionDeliveryState::LastNotification, ceiling)),
        None => admits,
    };
    match gate.notification {
        NotificationStamp::Throttled { ceiling } => SlotClaimStatement::Stamping(stamping_claim(target, owner, throttled(admits, Some(ceiling)), None)),
        NotificationStamp::Periodic { tick } => SlotClaimStatement::Stamping(stamping_claim(target, owner, admits, Some(tick))),
        NotificationStamp::WithOutcome => SlotClaimStatement::Checking(checking_claim(target, owner(), admits)),
        NotificationStamp::Unstamped { ceiling } => SlotClaimStatement::Checking(checking_claim(target, owner(), throttled(admits, ceiling))),
    }
}

/// `SELECT NOW() AS claimed_at, <admits> AS admitted, last_notification,
/// last_failure FROM subscription_delivery_state WHERE <owner>`: whether the
/// `owner` row admits a notification, and the stamps it was judged on.
fn checking_claim(target: &ContextTarget, owner: SimpleExpr, admits: Cond) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .expr_as(now(), ClaimedAt)
        .expr_as(admits, Admitted)
        .columns([SubscriptionDeliveryState::LastNotification, SubscriptionDeliveryState::LastFailure])
        .from(table_of(target, SubscriptionDeliveryState::Table))
        .and_where(owner);
    statement
}

/// The `UPDATE` counting a notification at its claim on the `owner` row when
/// it `admits` one, recording `tick` as the sweep instant a periodic claim
/// answers, beside the read of the row as it stood:
///
/// ```sql
/// WITH gated AS (SELECT subscription_key, last_notification, last_failure FROM subscription_delivery_state WHERE <owner>),
///      stamped AS (UPDATE subscription_delivery_state SET times_sent = times_sent + 1, last_notification = NOW()
///                  WHERE <owner> AND <admits> RETURNING subscription_key)
/// SELECT NOW() AS claimed_at, EXISTS (SELECT 1 FROM stamped) AS admitted, gated.last_notification, gated.last_failure FROM gated
/// ```
///
/// PostgreSQL runs a data-modifying CTE to completion whether or not the
/// query reads it. `gated` reads the statement's snapshot, so a claim the
/// update waited for shows its stamps only to the update, and the answer
/// then holds stamps that explain no refusal.
fn stamping_claim(target: &ContextTarget, owner: impl Fn() -> SimpleExpr, admits: Cond, tick: Option<DateTime<Utc>>) -> WithQuery {
    let gated = Query::select()
        .columns([
            SubscriptionDeliveryState::SubscriptionKey,
            SubscriptionDeliveryState::LastNotification,
            SubscriptionDeliveryState::LastFailure,
        ])
        .from(table_of(target, SubscriptionDeliveryState::Table))
        .and_where(owner())
        .to_owned();
    let mut stamped = Query::update();
    stamped
        .table(table_of(target, SubscriptionDeliveryState::Table))
        .value(SubscriptionDeliveryState::TimesSent, Expr::col(SubscriptionDeliveryState::TimesSent).add(1))
        .value(SubscriptionDeliveryState::LastNotification, now());
    if let Some(tick) = tick {
        stamped.value(SubscriptionDeliveryState::LastIntervalTick, Expr::val(tick));
    }
    stamped
        .and_where(owner())
        .cond_where(admits)
        .returning_col(SubscriptionDeliveryState::SubscriptionKey);
    let answer = Query::select()
        .expr_as(now(), ClaimedAt)
        .expr_as(Expr::exists(Query::select().expr(Expr::val(1)).from(Stamped).to_owned()), Admitted)
        .columns([
            (Gated, SubscriptionDeliveryState::LastNotification),
            (Gated, SubscriptionDeliveryState::LastFailure),
        ])
        .from(Gated)
        .to_owned();
    WithClause::new()
        .cte(CommonTableExpression::new().table_name(Gated).query(gated).to_owned())
        .cte(CommonTableExpression::new().table_name(Stamped).query(stamped).to_owned())
        .to_owned()
        .query(answer)
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::{
        delivery_state_fixture::{instant, subscription, target},
        slot_claim::{SlotClaimRow, SlotClaimStatement, slot_claim_statement},
        stored_kind::StoredKind,
    };
    use andromeda_ngsi_ld_lexicon::positive_milliseconds::PositiveMilliseconds;
    use andromeda_subscription::notification_slot::{DeliveryStamps, NotificationCadence, NotificationSlot, NotificationSlotGate, OutcomeRecord, Suppression};
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    const UNTHROTTLED: NotificationCadence = NotificationCadence::OnChange { throttling_secs: None };

    const THROTTLED: NotificationCadence = NotificationCadence::OnChange { throttling_secs: Some(30.0) };

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

    const ACTIVE_OWNER: &str = r#"WHERE "subscription_key" = (SELECT "subscriptions"."subscription_key" FROM "subscriptions" WHERE "subscriptions"."id" = 'urn:ngsi-ld:Subscription:one' AND "subscriptions"."kind" = 1 AND "subscriptions"."status" = 1)"#;

    /// The update a stamping claim's `stamped` CTE runs.
    fn stamped_part(sql: &str) -> &str {
        sql.split_once(r#""stamped" AS ("#)
            .and_then(|(_, rest)| rest.split_once(") SELECT NOW()"))
            .map_or("", |(update, _)| update)
    }

    #[test]
    fn a_stamping_claim_moves_only_the_state_row_of_an_active_subscription() {
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), NotificationCadence::Periodic, None));
        assert!(
            stamped_part(&sql).starts_with(r#"UPDATE "subscription_delivery_state" SET "times_sent" = "times_sent" + 1, "last_notification" = NOW()"#),
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
        assert_eq!(
            sql,
            format!(
                r#"SELECT NOW() AS "claimed_at", TRUE AS "admitted", "last_notification", "last_failure" FROM "subscription_delivery_state" {ACTIVE_OWNER}"#
            )
        );
    }

    #[test]
    fn a_throttled_claim_is_gated_on_the_last_notification_instant() {
        let sql = claim_sql(StoredKind::Csource, NotificationSlotGate::at(instant(30), THROTTLED, None));
        let stamped = stamped_part(&sql);
        assert!(stamped.starts_with(r#"UPDATE "subscription_delivery_state" SET "times_sent""#), "{sql}");
        assert!(
            stamped.contains(r#"("last_notification" IS NULL OR "last_notification" <= '2026-08-17 12:00:00.000000 +00:00')"#),
            "{sql}"
        );
        assert!(!stamped.contains("last_failure"), "an endpoint without a cooldown adds no second gate: {sql}");
        assert!(sql.contains(r#""subscriptions"."kind" = 2"#), "{sql}");
    }

    /// The update stamps only when the gate admits the claim, and the answer
    /// reads the row as the update was gated on it, so a refusal names the
    /// stamps that refused it in the same statement.
    #[test]
    fn a_stamping_claim_answers_whether_it_stamped_beside_the_stamps_it_was_gated_on() {
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), THROTTLED, None));
        assert_eq!(
            sql,
            format!(
                concat!(
                    r#"WITH "gated" AS (SELECT "subscription_key", "last_notification", "last_failure" FROM "subscription_delivery_state" {owner}) , "#,
                    r#""stamped" AS (UPDATE "subscription_delivery_state" SET "times_sent" = "times_sent" + 1, "last_notification" = NOW() {owner} "#,
                    r#"AND ("last_notification" IS NULL OR "last_notification" <= '2026-08-17 12:00:00.000000 +00:00') RETURNING "subscription_key") "#,
                    r#"SELECT NOW() AS "claimed_at", EXISTS(SELECT 1 FROM "stamped") AS "admitted", "gated"."last_notification", "gated"."last_failure" FROM "gated""#
                ),
                owner = ACTIVE_OWNER
            )
        );
    }

    /// Both windows must admit a checking claim: each is one `OR`, and the
    /// two are joined by `AND`.
    #[test]
    fn a_checking_claim_admits_only_inside_neither_window() {
        let cooldown = PositiveMilliseconds::try_from(10_000).expect("a positive millisecond count");
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), THROTTLED, Some(cooldown)).unstamped());
        assert!(
            sql.starts_with(concat!(
                r#"SELECT NOW() AS "claimed_at", ("last_failure" IS NULL OR "last_failure" <= '2026-08-17 12:00:20.000000 +00:00') "#,
                r#"AND ("last_notification" IS NULL OR "last_notification" <= '2026-08-17 12:00:00.000000 +00:00') AS "admitted", "#,
                r#""last_notification", "last_failure" FROM "subscription_delivery_state" WHERE"#
            )),
            "{sql}"
        );
    }

    /// A refusing answer read at 12:00:30.
    fn refusal() -> SlotClaimRow {
        SlotClaimRow {
            claimed_at: instant(30),
            admitted: false,
            last_notification: Some(instant(20)),
            last_failure: Some(instant(10)),
        }
    }

    fn admission() -> SlotClaimRow {
        SlotClaimRow { admitted: true, ..refusal() }
    }

    /// An admitted checking claim leaves the notification for its outcome to
    /// count, at the instant the claim read; a stamping claim counted it
    /// already.
    #[test]
    fn an_admitted_claim_leaves_its_outcome_what_the_claim_did_not_record() {
        let checking = slot_claim_statement(
            &target(),
            &subscription(),
            StoredKind::Entity,
            NotificationSlotGate::at(instant(30), UNTHROTTLED, None),
        );
        assert_eq!(
            admission().into_slot(&checking),
            NotificationSlot::Claimed(OutcomeRecord::CountingNotification { notified_at: instant(30) })
        );
        let stamping = slot_claim_statement(
            &target(),
            &subscription(),
            StoredKind::Entity,
            NotificationSlotGate::at(instant(30), THROTTLED, None),
        );
        assert_eq!(admission().into_slot(&stamping), NotificationSlot::Claimed(OutcomeRecord::OutcomeOnly));
    }

    #[test]
    fn a_refused_claim_carries_the_stamps_it_was_judged_on() {
        let stamping = slot_claim_statement(
            &target(),
            &subscription(),
            StoredKind::Entity,
            NotificationSlotGate::at(instant(30), THROTTLED, None),
        );
        assert_eq!(
            refusal().into_slot(&stamping),
            NotificationSlot::Suppressed(Suppression::Windowed(DeliveryStamps {
                last_notification: Some(instant(20)),
                last_failure: Some(instant(10)),
            }))
        );
    }

    /// Table 5.2.15-1: "If requests are received before the cooldown period has
    /// expired, no notification is sent", so the claim itself is narrowed by
    /// the endpoint's cooldown window and a refused claim sends nothing.
    #[test]
    fn a_cooldown_gates_the_claim_on_the_last_failure_instant() {
        let cooldown = PositiveMilliseconds::try_from(5_000).expect("a positive millisecond count");
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(5), UNTHROTTLED, Some(cooldown)));
        assert!(
            sql.starts_with(r#"SELECT NOW() AS "claimed_at", "last_failure" IS NULL OR "last_failure" <= '2026-08-17 12:00:00.000000 +00:00' AS "admitted""#),
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
        let stamped = stamped_part(&sql);
        assert!(stamped.starts_with(r#"UPDATE "subscription_delivery_state""#), "{sql}");
        assert!(stamped.contains(r#""last_failure" <= '2026-08-17 12:00:00.000000 +00:00'"#), "{sql}");
    }

    #[test]
    fn throttling_and_cooldown_gate_the_same_claim_independently() {
        let cooldown = PositiveMilliseconds::try_from(10_000).expect("a positive millisecond count");
        let sql = claim_sql(StoredKind::Entity, NotificationSlotGate::at(instant(30), THROTTLED, Some(cooldown)));
        assert!(sql.contains(r#""last_notification" <= '2026-08-17 12:00:00.000000 +00:00'"#), "{sql}");
        assert!(sql.contains(r#""last_failure" <= '2026-08-17 12:00:20.000000 +00:00'"#), "{sql}");
    }
}
