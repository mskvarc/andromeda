//! The `subscription_delivery_state` row: the clause 5.2.15 delivery counters
//! of one subscription, and every statement that reads or moves them.
//!
//! The counters are kept off the subscription row so a delivery never rewrites
//! it. A read of a subscription joins the state row back in; a claim or a mark
//! addresses the state row through the surrogate key the subscription's `id`
//! resolves to, so the wide row is read but never written on the delivery path.

use crate::subscription_store::{
    failure::persistence_error,
    stored_kind::{StoredKind, of_kind},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_schema::subscriptions::{SubscriptionDeliveryState, Subscriptions};
use andromeda_postgresql_seaquery::{clock::now, interval::make_interval_secs, literal::text_literal, placeholder::external_placeholder};
use andromeda_subscription::error::SubscriptionError;
use chrono::{DateTime, Utc};
use sea_query::{Cond, Expr, ExprTrait, Func, JoinType, PostgresQueryBuilder, Query, QueryStatementBuilder, SelectStatement, SimpleExpr, UpdateStatement};

/// Which delivery outcome a mark records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// The endpoint accepted the notification: `last_success` moves.
    Success,
    /// The endpoint refused or could not be reached: `last_failure` moves.
    Failure,
}

impl DeliveryOutcome {
    /// The state column this outcome stamps.
    const fn column(self) -> SubscriptionDeliveryState {
        match self {
            DeliveryOutcome::Success => SubscriptionDeliveryState::LastSuccess,
            DeliveryOutcome::Failure => SubscriptionDeliveryState::LastFailure,
        }
    }
}

/// Whether a state statement addresses only an active subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerStatus {
    /// Only a subscription whose `status` is `active` owns the row addressed.
    ActiveOnly,
    /// Any subscription of the family owns the row addressed.
    Any,
}

/// The insert of the state row that completes a freshly created subscription;
/// binds the context-space key as `$1` and surrogate key as `$2`.
pub fn state_insert_sql() -> Result<String, SubscriptionError> {
    let mut insert = Query::insert();
    insert
        .into_table(SubscriptionDeliveryState::Table)
        .columns([SubscriptionDeliveryState::ContextSpaceKey, SubscriptionDeliveryState::SubscriptionKey])
        .values([external_placeholder(1), external_placeholder(2)])
        .map_err(persistence_error)?;
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The scalar subquery resolving the state row's key from the subscription's
/// external `id`, family and, when asked, active status.
fn owner_key(target: &ContextTarget, id: &Uri, kind: StoredKind, status: OwnerStatus) -> SimpleExpr {
    let mut owner = Query::select();
    owner
        .column((Subscriptions::Table, Subscriptions::SubscriptionKey))
        .from(Subscriptions::Table)
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::Id)).eq(id.as_str()))
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::ContextSpaceKey)).eq(target.key().as_i64()))
        .and_where(of_kind(kind));
    match status {
        OwnerStatus::ActiveOnly => {
            owner.and_where(Expr::col((Subscriptions::Table, Subscriptions::Status)).eq(text_literal("active")));
        }
        OwnerStatus::Any => {}
    }
    SimpleExpr::SubQuery(None, Box::new(owner.into_sub_query_statement()))
}

/// The claim of one notification slot: `times_sent + 1`, `last_notification =
/// NOW()` on the state row of an active subscription, gated when a throttling
/// ceiling is given on `last_notification IS NULL OR last_notification <=
/// ceiling`. Zero rows affected means the slot was refused.
pub fn claim_statement(target: &ContextTarget, id: &Uri, kind: StoredKind, earliest_allowed_last_notification: Option<DateTime<Utc>>) -> UpdateStatement {
    let mut statement = Query::update();
    statement
        .table(SubscriptionDeliveryState::Table)
        .value(SubscriptionDeliveryState::TimesSent, Expr::col(SubscriptionDeliveryState::TimesSent).add(1))
        .value(SubscriptionDeliveryState::LastNotification, now())
        .and_where(Expr::col(SubscriptionDeliveryState::ContextSpaceKey).eq(target.key().as_i64()))
        .and_where(Expr::col(SubscriptionDeliveryState::SubscriptionKey).eq(owner_key(target, id, kind, OwnerStatus::ActiveOnly)));
    if let Some(ceiling) = earliest_allowed_last_notification {
        statement.cond_where(
            Cond::any()
                .add(Expr::col(SubscriptionDeliveryState::LastNotification).is_null())
                .add(Expr::col(SubscriptionDeliveryState::LastNotification).lte(ceiling)),
        );
    }
    statement
}

/// The `last_success` / `last_failure` stamp of one delivery outcome on the
/// state row, whatever the subscription's status is by then.
pub fn mark_statement(outcome: DeliveryOutcome, target: &ContextTarget, id: &Uri, kind: StoredKind) -> UpdateStatement {
    Query::update()
        .table(SubscriptionDeliveryState::Table)
        .value(outcome.column(), now())
        .and_where(Expr::col(SubscriptionDeliveryState::ContextSpaceKey).eq(target.key().as_i64()))
        .and_where(Expr::col(SubscriptionDeliveryState::SubscriptionKey).eq(owner_key(target, id, kind, OwnerStatus::Any)))
        .to_owned()
}

/// Join the state row into a `subscriptions` select and project its counters
/// under the names the mapper reads.
///
/// The join is outer so a subscription whose state row is missing still reads;
/// `times_sent` then coalesces to zero, the instants to NULL, which is exactly
/// what a never-notified subscription reports.
pub fn join_delivery_state(select: &mut SelectStatement) {
    select
        .join(
            JoinType::LeftJoin,
            SubscriptionDeliveryState::Table,
            Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::SubscriptionKey))
                .equals((Subscriptions::Table, Subscriptions::SubscriptionKey))
                .and(
                    Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::ContextSpaceKey))
                        .equals((Subscriptions::Table, Subscriptions::ContextSpaceKey)),
                ),
        )
        .expr_as(
            Func::coalesce([
                Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::TimesSent)),
                Expr::val(0),
            ]),
            SubscriptionDeliveryState::TimesSent,
        )
        .column((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastNotification))
        .column((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastSuccess))
        .column((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastFailure));
}

/// `NOW() - make_interval(secs => time_interval)`: the instant a periodic
/// subscription of the row's interval last became due.
fn interval_deadline() -> SimpleExpr {
    Expr::expr(now()).sub(make_interval_secs(Expr::col((Subscriptions::Table, Subscriptions::TimeInterval))))
}

/// The due-interval predicate over a select that has the state row joined: an
/// active periodic subscription whose interval has elapsed since its last
/// notification, or since creation if never sent.
pub fn interval_elapsed() -> Cond {
    let never_sent = Cond::all()
        .add(last_notification().is_null())
        .add(Expr::col((Subscriptions::Table, Subscriptions::CreatedAt)).lte(interval_deadline()));
    let since_last = Cond::all()
        .add(last_notification().is_not_null())
        .add(last_notification().lte(interval_deadline()));
    Cond::any().add(never_sent).add(since_last)
}

/// The state row's `last_notification`, qualified for a joined select.
fn last_notification() -> Expr {
    Expr::col((SubscriptionDeliveryState::Table, SubscriptionDeliveryState::LastNotification))
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::{
        delivery_state::{DeliveryOutcome, claim_statement, interval_elapsed, join_delivery_state, mark_statement, state_insert_sql},
        stored_kind::StoredKind,
    };
    use andromeda_context_space::{
        context_space::{ContextSpace, ContextSpaceKind},
        context_space_key::ContextSpaceKey,
        context_target::ContextTarget,
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_postgresql_schema::subscriptions::Subscriptions;
    use chrono::{TimeZone, Utc};
    use sea_query::{PostgresQueryBuilder, Query};

    fn subscription() -> Uri {
        "urn:ngsi-ld:Subscription:one".parse().expect("valid")
    }

    fn target() -> ContextTarget {
        let key = ContextSpaceKey::try_from(1).expect("positive context-space key");
        ContextTarget::live(ContextSpace::new(key, ContextSpaceKind::Live)).expect("live context target")
    }

    #[test]
    fn the_state_row_is_inserted_under_the_returned_surrogate_key_alone() {
        assert_eq!(
            state_insert_sql().expect("builds"),
            r#"INSERT INTO "subscription_delivery_state" ("subscription_key") VALUES ($1)"#
        );
    }

    #[test]
    fn the_claim_moves_only_the_state_row_of_an_active_subscription() {
        let sql = claim_statement(&target(), &subscription(), StoredKind::Entity, None).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(r#"UPDATE "subscription_delivery_state" SET "times_sent" = "times_sent" + 1, "last_notification" = NOW()"#),
            "{sql}"
        );
        assert!(
            sql.contains(
                r#"WHERE "subscription_key" = (SELECT "subscriptions"."subscription_key" FROM "subscriptions" WHERE "subscriptions"."id" = 'urn:ngsi-ld:Subscription:one' AND "subscriptions"."kind" = ('entity') AND "subscriptions"."status" = ('active'))"#
            ),
            "{sql}"
        );
        assert!(
            !sql.contains("modified_at"),
            "a delivery must not touch the subscription's modification instant: {sql}"
        );
        assert!(!sql.contains(r#"UPDATE "subscriptions""#), "{sql}");
    }

    #[test]
    fn a_throttled_claim_is_gated_on_the_last_notification_instant() {
        let ceiling = Utc.with_ymd_and_hms(2026, 8, 17, 12, 0, 0).single().expect("valid instant");
        let sql = claim_statement(&target(), &subscription(), StoredKind::Csource, Some(ceiling)).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"("last_notification" IS NULL OR "last_notification" <= '2026-08-17 12:00:00.000000 +00:00')"#),
            "{sql}"
        );
        assert!(sql.contains("'csource'"), "{sql}");
    }

    #[test]
    fn a_mark_stamps_one_outcome_column_whatever_the_subscription_status_is() {
        let success = mark_statement(DeliveryOutcome::Success, &target(), &subscription(), StoredKind::Entity).to_string(PostgresQueryBuilder);
        assert!(
            success.starts_with(r#"UPDATE "subscription_delivery_state" SET "last_success" = NOW() WHERE"#),
            "{success}"
        );
        assert!(!success.contains("status"), "{success}");
        let failure = mark_statement(DeliveryOutcome::Failure, &target(), &subscription(), StoredKind::Entity).to_string(PostgresQueryBuilder);
        assert!(failure.contains(r#""last_failure" = NOW()"#) && !failure.contains("last_success"), "{failure}");
    }

    #[test]
    fn a_projection_outer_joins_the_state_row_and_coalesces_the_counter() {
        let mut select = Query::select();
        select.column((Subscriptions::Table, Subscriptions::Id)).from(Subscriptions::Table);
        join_delivery_state(&mut select);
        let sql = select.to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"LEFT JOIN "subscription_delivery_state" ON "subscription_delivery_state"."subscription_key" = "subscriptions"."subscription_key""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"COALESCE("subscription_delivery_state"."times_sent", 0) AS "times_sent""#),
            "{sql}"
        );
        for column in ["last_notification", "last_success", "last_failure"] {
            assert!(sql.contains(&format!(r#""subscription_delivery_state"."{column}""#)), "{sql}");
        }
    }

    #[test]
    fn the_interval_predicate_reads_the_last_notification_from_the_state_row() {
        let sql = Query::select().cond_where(interval_elapsed()).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#""subscription_delivery_state"."last_notification" IS NULL AND "subscriptions"."created_at" <="#),
            "{sql}"
        );
        assert!(
            sql.contains(r#""subscription_delivery_state"."last_notification" <= NOW() - (make_interval(secs => "subscriptions"."time_interval"))"#),
            "{sql}"
        );
    }
}
