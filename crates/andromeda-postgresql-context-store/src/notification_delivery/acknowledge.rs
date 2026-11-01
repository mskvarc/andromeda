//! Releasing a claimed delivery back to the queue, or out of it.
//!
//! Every statement here is guarded by the claim it was taken under: it names
//! the worker that holds the lease and requires that lease to still be live.
//! A worker whose lease expired while it was delivering therefore updates
//! nothing instead of overwriting the progress of the worker that reclaimed the
//! row, and the caller learns that from the returned [`LeaseAcknowledgement`]
//! rather than from a silent no-op.
//!
//! Each acknowledgement also moves the hold of the delivery's Subscription
//! ([`crate::notification_delivery::hold`]) in the same transaction: a
//! retryable failure takes it, a delivery releases it and an exhaustion hands
//! it over. The guarded update is the first statement's first common table
//! expression and the hold reads the row from it, so a guard that refused
//! returns no row and changes no hold either.
//!
//! The failure diagnostic arrives already trimmed to the length the schema
//! accepts, because the bound is a delivery-policy decision the notification
//! crate owns. Nothing here shortens it a second time: what is stored is
//! exactly what the worker recorded.

use crate::{
    error::ContextStoreError,
    notification_delivery::hold::{
        Acknowledged,
        HeldSubscription,
        Probed,
        Released,
        Taken,
        acknowledged_beside,
        hand_over_statement,
        probed_on_exhaustion,
        released_on_delivery,
        resume_statement,
        set_aside_statement,
        taken_on_failure,
    },
};
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::{NotificationDeliveries, NotificationDeliveryHolds};
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{CommonTableExpression, Expr, ExprTrait, JoinType, Keyword, Query, SelectStatement, SimpleExpr, UpdateStatement, WithClause, WithQuery};
use sqlx::postgres::PgPool;

/// The statement name every delivery acknowledgement (delivered, failed,
/// exhausted) is observed under.
const DELIVERY_ACKNOWLEDGE: StatementName = StatementName("notification-delivery-acknowledge");

/// The statement name the second statement of an acknowledgement, the one
/// that sets deliveries aside, releases them or hands their hold over, is
/// observed under.
const DELIVERY_HOLD: StatementName = StatementName("notification-delivery-hold");

/// Record that `worker_id` delivered the notification, and release the hold of
/// its Subscription.
///
/// The claim triple stays in place: `delivered_at` already makes the delivery
/// unclaimable, and the recorded owner is the only trace of which worker sent
/// it. Clearing the lease alone would leave two of the three columns populated,
/// which the schema's claim coherence constraint rejects.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails.
pub async fn mark_delivered(
    pool: &PgPool,
    delivery_key: i64,
    worker_id: LeaseHolder,
    target: &ContextTarget,
) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let mut tx = pool.begin().await?;
    let recorded: Option<(i64, Option<i64>)> = observe_statement(
        DELIVERY_ACKNOWLEDGE,
        mark_delivered_statement(delivery_key, worker_id, target).to_query_as().fetch_optional(&mut *tx),
    )
    .await?;
    let outcome = match recorded {
        None => LeaseAcknowledgement::Lost,
        Some((_, released)) => {
            if let Some(subscription_key) = released {
                observe_statement(DELIVERY_HOLD, resume_statement(target, subscription_key).to_query().execute(&mut *tx)).await?;
            }
            LeaseAcknowledgement::Applied
        }
    };
    tx.commit().await?;
    Ok(outcome)
}

/// Record a failed attempt, release the lease, schedule the next attempt and
/// hold the rest of the Subscription's queue behind it.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails.
pub async fn mark_failed(
    pool: &PgPool,
    delivery_key: i64,
    worker_id: LeaseHolder,
    failure: &str,
    next_attempt_at: DateTime<Utc>,
    target: &ContextTarget,
) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let statement = mark_failed_statement(delivery_key, worker_id, failure, next_attempt_at, target)?;
    let mut tx = pool.begin().await?;
    let recorded: Option<(i64, i64, i64)> = observe_statement(DELIVERY_ACKNOWLEDGE, statement.to_query_as().fetch_optional(&mut *tx)).await?;
    let outcome = match recorded {
        None => LeaseAcknowledgement::Lost,
        Some((_, subscription_key, probe_delivery_key)) => {
            let held = HeldSubscription {
                subscription_key,
                probe_delivery_key,
            };
            observe_statement(DELIVERY_HOLD, set_aside_statement(target, held).to_query().execute(&mut *tx)).await?;
            LeaseAcknowledgement::Applied
        }
    };
    tx.commit().await?;
    Ok(outcome)
}

/// Record a terminal failure, make the delivery permanently unclaimable and,
/// when it was its Subscription's probe, hand the hold to the next probe, due
/// at `next_probe_at`.
///
/// Exhaustion is a decision the worker takes — on a failure no retry can fix,
/// or on the attempt that reached the configured ceiling — and it is recorded
/// as an instant of its own so the claim predicate and the dead-letter reap
/// need not know the ceiling.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when a statement fails.
pub async fn mark_exhausted(
    pool: &PgPool,
    delivery_key: i64,
    worker_id: LeaseHolder,
    failure: &str,
    next_probe_at: DateTime<Utc>,
    target: &ContextTarget,
) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let mut tx = pool.begin().await?;
    let recorded: Option<(i64, Option<i64>)> = observe_statement(
        DELIVERY_ACKNOWLEDGE,
        mark_exhausted_statement(delivery_key, worker_id, failure, target)
            .to_query_as()
            .fetch_optional(&mut *tx),
    )
    .await?;
    let outcome = match recorded {
        None => LeaseAcknowledgement::Lost,
        Some((_, probed)) => {
            if let Some(subscription_key) = probed {
                observe_statement(
                    DELIVERY_HOLD,
                    hand_over_statement(target, subscription_key, next_probe_at).to_query().execute(&mut *tx),
                )
                .await?;
            }
            LeaseAcknowledgement::Applied
        }
    };
    tx.commit().await?;
    Ok(outcome)
}

/// Every acknowledgement is scoped to the row this worker still holds the lease
/// on: the delivery's own key, the recorded owner, and a lease that has not yet
/// expired. The predicate matches at most one row.
fn lease_guard(delivery_key: i64, worker_id: LeaseHolder) -> SimpleExpr {
    Expr::col(NotificationDeliveries::DeliveryKey)
        .eq(delivery_key)
        .and(Expr::col(NotificationDeliveries::ClaimedBy).eq(worker_id.as_uuid()))
        .and(Expr::col(NotificationDeliveries::LeaseUntil).gt(Expr::current_timestamp()))
}

/// The guarded update of the acknowledged row, returning its key and its
/// Subscription's for the hold.
fn acknowledgement(delivery_key: i64, worker_id: LeaseHolder, target: &ContextTarget) -> UpdateStatement {
    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .and_where(lease_guard(delivery_key, worker_id))
        .returning(Query::returning().columns([NotificationDeliveries::DeliveryKey, NotificationDeliveries::SubscriptionKey]))
        .to_owned()
}

/// `WITH acknowledged AS (<update>), <hold> <select>`.
fn acknowledged_with(update: UpdateStatement, hold: CommonTableExpression, select: SelectStatement) -> WithQuery {
    WithClause::new()
        .cte(CommonTableExpression::new().table_name(Acknowledged).query(update).to_owned())
        .cte(hold)
        .to_owned()
        .query(select)
}

/// Mark the delivery sent without disturbing the claim triple, and delete the
/// hold of its Subscription.
fn mark_delivered_statement(delivery_key: i64, worker_id: LeaseHolder, target: &ContextTarget) -> WithQuery {
    let mut update = acknowledgement(delivery_key, worker_id, target);
    update.value(NotificationDeliveries::DeliveredAt, Expr::current_timestamp());
    acknowledged_with(
        update,
        released_on_delivery(target),
        acknowledged_beside(Released, &[NotificationDeliveryHolds::SubscriptionKey], JoinType::LeftJoin),
    )
}

/// Count the attempt, record its diagnostic, release the lease for a retry,
/// and take or find the hold of its Subscription.
fn mark_failed_statement(
    delivery_key: i64,
    worker_id: LeaseHolder,
    failure: &str,
    next_attempt_at: DateTime<Utc>,
    target: &ContextTarget,
) -> Result<WithQuery, ContextStoreError> {
    let mut update = acknowledgement(delivery_key, worker_id, target);
    update
        .value(NotificationDeliveries::AttemptCount, Expr::col(NotificationDeliveries::AttemptCount).add(1))
        .value(NotificationDeliveries::LastError, failure)
        .value(NotificationDeliveries::NextAttemptAt, next_attempt_at)
        .value(NotificationDeliveries::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::LeaseUntil, SimpleExpr::Keyword(Keyword::Null));
    Ok(acknowledged_with(
        update,
        taken_on_failure(target)?,
        acknowledged_beside(
            Taken,
            &[NotificationDeliveryHolds::SubscriptionKey, NotificationDeliveryHolds::DeliveryKey],
            JoinType::InnerJoin,
        ),
    ))
}

/// Count the final attempt, stamp the exhaustion instant, release the lease
/// for good, and lock the hold the delivery was the probe of.
///
/// The claim triple is cleared in the same statement because
/// `chk_notification_deliveries_exhausted_state` forbids an exhausted row that
/// still holds a claim.
fn mark_exhausted_statement(delivery_key: i64, worker_id: LeaseHolder, failure: &str, target: &ContextTarget) -> WithQuery {
    let mut update = acknowledgement(delivery_key, worker_id, target);
    update
        .value(NotificationDeliveries::AttemptCount, Expr::col(NotificationDeliveries::AttemptCount).add(1))
        .value(NotificationDeliveries::ExhaustedAt, Expr::current_timestamp())
        .value(NotificationDeliveries::LastError, failure)
        .value(NotificationDeliveries::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::LeaseUntil, SimpleExpr::Keyword(Keyword::Null));
    acknowledged_with(
        update,
        probed_on_exhaustion(target),
        acknowledged_beside(Probed, &[NotificationDeliveryHolds::SubscriptionKey], JoinType::LeftJoin),
    )
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::notification_delivery::{
        acknowledge::{mark_delivered_statement, mark_exhausted_statement, mark_failed_statement},
        hold::{HeldSubscription, hand_over_statement, resume_statement, set_aside_statement},
    };
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    fn worker() -> LeaseHolder {
        LeaseHolder::from(Uuid::nil())
    }

    fn delivered(target: &ContextTarget) -> String {
        mark_delivered_statement(1, worker(), target).to_string(PostgresQueryBuilder)
    }

    fn failed(target: &ContextTarget) -> String {
        mark_failed_statement(1, worker(), "boom", Utc::now(), target)
            .expect("a buildable statement")
            .to_string(PostgresQueryBuilder)
    }

    fn exhausted(target: &ContextTarget) -> String {
        mark_exhausted_statement(1, worker(), "boom", target).to_string(PostgresQueryBuilder)
    }

    fn held() -> HeldSubscription {
        HeldSubscription {
            subscription_key: 7,
            probe_delivery_key: 3,
        }
    }

    fn snapshot() -> ContextTarget {
        let key = SnapshotKey::try_from(5).expect("positive key");
        ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")))
    }

    /// The text of the guarded update, the statement's first common table
    /// expression.
    fn acknowledgement_of(sql: &str) -> &str {
        sql.split(" RETURNING ").next().unwrap_or_default()
    }

    #[test]
    fn every_acknowledgement_is_guarded_by_the_live_lease_it_was_taken_under() {
        for sql in [delivered(&ContextTarget::Live), failed(&ContextTarget::Live), exhausted(&ContextTarget::Live)] {
            assert!(sql.starts_with(r#"WITH "acknowledged" AS (UPDATE "notification_deliveries" SET"#), "{sql}");
            let guard = acknowledgement_of(&sql);
            assert!(guard.contains(r#""delivery_key" = 1"#), "{sql}");
            assert!(guard.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
            assert!(guard.contains(r#""lease_until" > CURRENT_TIMESTAMP"#), "{sql}");
        }
    }

    /// A guard that refused leaves `acknowledged` empty, so the first
    /// statement returns no row whatever its hold piece did.
    #[test]
    fn every_acknowledgement_returns_one_row_only_when_its_guard_held() {
        let delivered = delivered(&ContextTarget::Live);
        assert!(
            delivered.ends_with(r#"SELECT "acknowledged"."delivery_key", "released"."subscription_key" FROM "acknowledged" LEFT JOIN "released" ON TRUE"#),
            "{delivered}"
        );
        let failed = failed(&ContextTarget::Live);
        assert!(
            failed.ends_with(concat!(
                r#"SELECT "acknowledged"."delivery_key", "taken"."subscription_key", "taken"."delivery_key" "#,
                r#"FROM "acknowledged" INNER JOIN "taken" ON TRUE"#
            )),
            "{failed}"
        );
        let exhausted = exhausted(&ContextTarget::Live);
        assert!(
            exhausted.ends_with(r#"SELECT "acknowledged"."delivery_key", "probed"."subscription_key" FROM "acknowledged" LEFT JOIN "probed" ON TRUE"#),
            "{exhausted}"
        );
    }

    #[test]
    fn a_delivered_row_keeps_its_claim_triple() {
        let sql = delivered(&ContextTarget::Live);
        let guard = acknowledgement_of(&sql);
        assert!(guard.contains(r#""delivered_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(!guard.contains(r#""claimed_by" = NULL"#), "{sql}");
    }

    /// The endpoint answered, so the Subscription's hold goes, and its
    /// deliveries rejoin the claim order with the due instant they had.
    #[test]
    fn a_delivered_row_releases_its_subscriptions_hold() {
        let sql = delivered(&ContextTarget::Live);
        assert!(
            sql.contains(concat!(
                r#""released" AS (DELETE FROM "notification_delivery_holds" WHERE "subscription_key" IN "#,
                r#"(SELECT "subscription_key" FROM "acknowledged") RETURNING "subscription_key")"#
            )),
            "{sql}"
        );
        let resume = resume_statement(&ContextTarget::Live, 7).to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            resume,
            r#"UPDATE "notification_deliveries" SET "held_at" = NULL WHERE "subscription_key" = 7 AND "held_at" IS NOT NULL"#
        );
    }

    #[test]
    fn a_failed_row_counts_the_attempt_and_releases_the_lease() {
        let sql = failed(&ContextTarget::Live);
        let guard = acknowledgement_of(&sql);
        assert!(guard.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
        assert!(guard.contains(r#""claimed_at" = NULL"#), "{sql}");
        assert!(guard.contains(r#""claimed_by" = NULL"#), "{sql}");
        assert!(guard.contains(r#""lease_until" = NULL"#), "{sql}");
    }

    /// A failure makes the failed delivery the probe of a Subscription not yet
    /// held, and finds and locks the hold of one that is.
    #[test]
    fn a_failed_row_takes_its_subscriptions_hold_or_finds_it() {
        let sql = failed(&ContextTarget::Live);
        assert!(
            sql.contains(concat!(
                r#""taken" AS (INSERT INTO "notification_delivery_holds" ("subscription_key", "delivery_key") "#,
                r#"SELECT "subscription_key", "delivery_key" FROM "acknowledged" ON CONFLICT ("subscription_key") "#,
                r#"DO UPDATE SET "delivery_key" = "notification_delivery_holds"."delivery_key" "#,
                r#"RETURNING "subscription_key", "delivery_key")"#
            )),
            "{sql}"
        );
    }

    /// Every other queued delivery of the held Subscription is set aside,
    /// except one in flight under a live lease, whose own outcome settles it.
    #[test]
    fn a_failure_sets_the_rest_of_its_subscriptions_queue_aside() {
        let sql = set_aside_statement(&ContextTarget::Live, held()).to_string(PostgresQueryBuilder);
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"UPDATE "notification_deliveries" SET "held_at" = CURRENT_TIMESTAMP, "claimed_at" = NULL, "claimed_by" = NULL, "#,
                r#""lease_until" = NULL WHERE "subscription_key" = 7 AND "held_at" IS NULL AND "delivery_key" <> 3 "#,
                r#"AND "delivered_at" IS NULL AND "exhausted_at" IS NULL "#,
                r#"AND ("lease_until" IS NULL OR "lease_until" <= CURRENT_TIMESTAMP)"#
            )
        );
    }

    #[test]
    fn an_exhausted_row_is_stamped_with_the_exhaustion_instant_and_releases_its_claim() {
        let sql = exhausted(&ContextTarget::Live);
        let guard = acknowledgement_of(&sql);
        assert!(guard.contains(r#""exhausted_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(guard.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
        assert!(guard.contains(r#""claimed_at" = NULL"#), "{sql}");
        assert!(guard.contains(r#""claimed_by" = NULL"#), "{sql}");
        assert!(guard.contains(r#""lease_until" = NULL"#), "{sql}");
        assert!(!sql.contains(&i32::MAX.to_string()), "no sentinel attempt count: {sql}");
    }

    /// Only a probe's exhaustion finds the hold, which it locks before the
    /// hand-over reads the deliveries set aside.
    #[test]
    fn an_exhausted_row_locks_the_hold_it_was_the_probe_of() {
        let sql = exhausted(&ContextTarget::Live);
        assert!(
            sql.contains(concat!(
                r#""probed" AS (SELECT "subscription_key" FROM "notification_delivery_holds" WHERE "delivery_key" IN "#,
                r#"(SELECT "delivery_key" FROM "acknowledged") FOR UPDATE)"#
            )),
            "{sql}"
        );
    }

    /// The hold passes to the delivery set aside first, which rejoins the
    /// claim order at the instant given; with none set aside the hold goes.
    #[test]
    fn an_exhausted_probe_hands_its_hold_to_the_delivery_set_aside_first() {
        let sql = hand_over_statement(&ContextTarget::Live, 7, Utc::now()).to_string(PostgresQueryBuilder);
        assert!(
            sql.starts_with(concat!(
                r#"WITH "successor" AS (SELECT "delivery_key" FROM "notification_deliveries" WHERE "subscription_key" = 7 "#,
                r#"AND "held_at" IS NOT NULL ORDER BY "held_at" ASC, "delivery_key" ASC LIMIT 1) , "#,
                r#""handed_over" AS (UPDATE "notification_delivery_holds" SET "delivery_key" = "successor"."delivery_key" "#,
                r#"FROM "successor" WHERE "notification_delivery_holds"."subscription_key" = 7) , "#,
                r#""dropped" AS (DELETE FROM "notification_delivery_holds" WHERE "subscription_key" = 7 "#,
                r#"AND (NOT EXISTS(SELECT 1 FROM "successor"))) , "#,
                r#""revived" AS (UPDATE "notification_deliveries" SET "held_at" = NULL, "next_attempt_at" = '"#
            )),
            "{sql}"
        );
        assert!(
            sql.ends_with(concat!(
                r#"FROM "successor" WHERE "notification_deliveries"."delivery_key" = "successor"."delivery_key") "#,
                r#"SELECT "delivery_key" FROM "successor""#
            )),
            "{sql}"
        );
    }

    #[test]
    fn a_snapshot_branch_acknowledgement_moves_the_hold_of_its_own_branch() {
        for sql in [
            delivered(&snapshot()),
            failed(&snapshot()),
            exhausted(&snapshot()),
            hand_over_statement(&snapshot(), 7, Utc::now()).to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#""snap_5"."notification_delivery_holds""#), "{sql}");
            assert!(!sql.contains(r#"FROM "notification_delivery_holds""#), "{sql}");
            assert!(!sql.contains(r#"INTO "notification_delivery_holds""#), "{sql}");
        }
        for sql in [
            set_aside_statement(&snapshot(), held()).to_string(PostgresQueryBuilder),
            resume_statement(&snapshot(), 7).to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.starts_with(r#"UPDATE "snap_5"."notification_deliveries""#), "{sql}");
        }
    }
}
