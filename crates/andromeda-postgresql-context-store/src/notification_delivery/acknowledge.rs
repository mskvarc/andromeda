//! Releasing a claimed delivery back to the queue, or out of it.
//!
//! Every statement here is guarded by the claim it was taken under: it names
//! the worker that holds the lease and requires that lease to still be live.
//! A worker whose lease expired while it was delivering therefore updates
//! nothing instead of overwriting the progress of the worker that reclaimed the
//! row, and the caller learns that from the returned [`LeaseAcknowledgement`]
//! rather than from a silent no-op.
//!
//! The failure diagnostic arrives already trimmed to the length the schema
//! accepts, because the bound is a delivery-policy decision the notification
//! crate owns. Nothing here shortens it a second time: what is stored is
//! exactly what the worker recorded.

use crate::error::ContextStoreError;
use andromeda_context_commit::{lease_acknowledgement::LeaseAcknowledgement, lease_holder::LeaseHolder};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::notification::NotificationDeliveries;
use andromeda_telemetry::statement::{StatementName, observe_statement};
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Keyword, PostgresQueryBuilder, Query, SimpleExpr, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::postgres::PgPool;

/// The statement name every delivery acknowledgement (delivered, failed,
/// exhausted) is observed under.
const DELIVERY_ACKNOWLEDGE: StatementName = StatementName("notification-delivery-acknowledge");

/// Record that `worker_id` delivered the notification.
///
/// The claim triple stays in place: `delivered_at` already makes the delivery
/// unclaimable, and the recorded owner is the only trace of which worker sent
/// it. Clearing the lease alone would leave two of the three columns populated,
/// which the schema's claim coherence constraint rejects.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn mark_delivered(
    pool: &PgPool,
    delivery_key: i64,
    worker_id: LeaseHolder,
    target: &ContextTarget,
) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let (sql, values) = mark_delivered_statement(delivery_key, worker_id, target).build_sqlx(PostgresQueryBuilder);
    let result = observe_statement(DELIVERY_ACKNOWLEDGE, sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool)).await?;
    Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
}

/// Record a failed attempt, release the lease and schedule the next attempt.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn mark_failed(
    pool: &PgPool,
    delivery_key: i64,
    worker_id: LeaseHolder,
    failure: &str,
    next_attempt_at: DateTime<Utc>,
    target: &ContextTarget,
) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let (sql, values) = mark_failed_statement(delivery_key, worker_id, failure, next_attempt_at, target).build_sqlx(PostgresQueryBuilder);
    let result = observe_statement(DELIVERY_ACKNOWLEDGE, sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool)).await?;
    Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
}

/// Record a terminal failure and make the delivery permanently unclaimable.
///
/// Exhaustion is a decision the worker takes — on a failure no retry can fix,
/// or on the attempt that reached the configured ceiling — and it is recorded
/// as an instant of its own so the claim predicate and the dead-letter reap
/// need not know the ceiling.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn mark_exhausted(
    pool: &PgPool,
    delivery_key: i64,
    worker_id: LeaseHolder,
    failure: &str,
    target: &ContextTarget,
) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let (sql, values) = mark_exhausted_statement(delivery_key, worker_id, failure, target).build_sqlx(PostgresQueryBuilder);
    let result = observe_statement(DELIVERY_ACKNOWLEDGE, sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool)).await?;
    Ok(LeaseAcknowledgement::of_guarded_update(result.rows_affected()))
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

/// Mark the delivery sent without disturbing the claim triple.
fn mark_delivered_statement(delivery_key: i64, worker_id: LeaseHolder, target: &ContextTarget) -> UpdateStatement {
    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::DeliveredAt, Expr::current_timestamp())
        .and_where(lease_guard(delivery_key, worker_id))
        .to_owned()
}

/// Count the attempt, record its diagnostic, and release the lease for a retry.
fn mark_failed_statement(delivery_key: i64, worker_id: LeaseHolder, failure: &str, next_attempt_at: DateTime<Utc>, target: &ContextTarget) -> UpdateStatement {
    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::AttemptCount, Expr::col(NotificationDeliveries::AttemptCount).add(1))
        .value(NotificationDeliveries::LastError, failure)
        .value(NotificationDeliveries::NextAttemptAt, next_attempt_at)
        .value(NotificationDeliveries::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .and_where(lease_guard(delivery_key, worker_id))
        .to_owned()
}

/// Count the final attempt, stamp the exhaustion instant and release the lease
/// for good.
///
/// The claim triple is cleared in the same statement because
/// `chk_notification_deliveries_exhausted_state` forbids an exhausted row that
/// still holds a claim.
fn mark_exhausted_statement(delivery_key: i64, worker_id: LeaseHolder, failure: &str, target: &ContextTarget) -> UpdateStatement {
    Query::update()
        .table(table_of(target, NotificationDeliveries::Table))
        .value(NotificationDeliveries::AttemptCount, Expr::col(NotificationDeliveries::AttemptCount).add(1))
        .value(NotificationDeliveries::ExhaustedAt, Expr::current_timestamp())
        .value(NotificationDeliveries::LastError, failure)
        .value(NotificationDeliveries::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .and_where(lease_guard(delivery_key, worker_id))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::acknowledge::{mark_delivered_statement, mark_exhausted_statement, mark_failed_statement};
    use andromeda_context_commit::lease_holder::LeaseHolder;
    use andromeda_context_space::context_target::ContextTarget;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;
    use uuid::Uuid;

    #[test]
    fn every_acknowledgement_is_guarded_by_the_live_lease_it_was_taken_under() {
        for sql in [
            mark_delivered_statement(1, LeaseHolder::from(Uuid::nil()), &ContextTarget::Live).to_string(PostgresQueryBuilder),
            mark_failed_statement(1, LeaseHolder::from(Uuid::nil()), "boom", Utc::now(), &ContextTarget::Live).to_string(PostgresQueryBuilder),
            mark_exhausted_statement(1, LeaseHolder::from(Uuid::nil()), "boom", &ContextTarget::Live).to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#""delivery_key" = 1"#), "{sql}");
            assert!(sql.contains(r#""claimed_by" = '00000000-0000-0000-0000-000000000000'"#), "{sql}");
            assert!(sql.contains(r#""lease_until" > CURRENT_TIMESTAMP"#), "{sql}");
        }
    }

    #[test]
    fn a_delivered_row_keeps_its_claim_triple() {
        let sql = mark_delivered_statement(1, LeaseHolder::from(Uuid::nil()), &ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""delivered_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(!sql.contains(r#""claimed_by" = NULL"#), "{sql}");
    }

    #[test]
    fn a_failed_row_counts_the_attempt_and_releases_the_lease() {
        let sql = mark_failed_statement(1, LeaseHolder::from(Uuid::nil()), "boom", Utc::now(), &ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
        assert!(sql.contains(r#""claimed_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = NULL"#), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
    }

    #[test]
    fn an_exhausted_row_is_stamped_with_the_exhaustion_instant_and_releases_its_claim() {
        let sql = mark_exhausted_statement(1, LeaseHolder::from(Uuid::nil()), "boom", &ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""exhausted_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
        assert!(sql.contains(r#""claimed_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = NULL"#), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
        assert!(!sql.contains(&i32::MAX.to_string()), "no sentinel attempt count: {sql}");
    }
}
