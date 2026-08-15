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
use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
use andromeda_postgresql_schema::notification::NotificationDeliveries;
use chrono::{DateTime, Utc};
use sea_query::{Expr, ExprTrait, Keyword, PostgresQueryBuilder, Query, SimpleExpr, UpdateStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::postgres::PgPool;

/// The attempt count an exhausted delivery carries.
///
/// Exhaustion is expressed by pushing the counter past any reachable ceiling
/// rather than by a separate flag, so the one claim predicate that already
/// excludes deliveries over the configured maximum excludes dead ones too.
///
/// Shared with [`crate::notification_delivery::cleanup`], which selects
/// exhausted rows by this same sentinel when applying dead-letter retention.
pub(crate) const EXHAUSTED_ATTEMPT_COUNT: i32 = i32::MAX;

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
pub async fn mark_delivered(pool: &PgPool, id: i64, worker_id: &str) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let (sql, values) = mark_delivered_statement(id, worker_id).build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool).await?;
    Ok(acknowledgement(result.rows_affected()))
}

/// Record a failed attempt, release the lease and schedule the next attempt.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn mark_failed(
    pool: &PgPool,
    id: i64,
    worker_id: &str,
    failure: &str,
    next_attempt_at: DateTime<Utc>,
) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let (sql, values) = mark_failed_statement(id, worker_id, failure, next_attempt_at).build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool).await?;
    Ok(acknowledgement(result.rows_affected()))
}

/// Record a terminal failure and make the delivery permanently unclaimable.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn mark_exhausted(pool: &PgPool, id: i64, worker_id: &str, failure: &str) -> Result<LeaseAcknowledgement, ContextStoreError> {
    let (sql, values) = mark_exhausted_statement(id, worker_id, failure).build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).execute(pool).await?;
    Ok(acknowledgement(result.rows_affected()))
}

/// Every acknowledgement is scoped to the row this worker still holds the lease
/// on: the delivery's own id, the recorded owner, and a lease that has not yet
/// expired. The predicate matches at most one row.
fn lease_guard(id: i64, worker_id: &str) -> SimpleExpr {
    Expr::col(NotificationDeliveries::Id)
        .eq(id)
        .and(Expr::col(NotificationDeliveries::ClaimedBy).eq(worker_id))
        .and(Expr::col(NotificationDeliveries::LeaseUntil).gt(Expr::current_timestamp()))
}

/// Mark the delivery sent without disturbing the claim triple.
fn mark_delivered_statement(id: i64, worker_id: &str) -> UpdateStatement {
    Query::update()
        .table(NotificationDeliveries::Table)
        .value(NotificationDeliveries::DeliveredAt, Expr::current_timestamp())
        .and_where(lease_guard(id, worker_id))
        .to_owned()
}

/// Count the attempt, record its diagnostic, and release the lease for a retry.
fn mark_failed_statement(id: i64, worker_id: &str, failure: &str, next_attempt_at: DateTime<Utc>) -> UpdateStatement {
    Query::update()
        .table(NotificationDeliveries::Table)
        .value(NotificationDeliveries::AttemptCount, Expr::col(NotificationDeliveries::AttemptCount).add(1))
        .value(NotificationDeliveries::DeliveryError, failure)
        .value(NotificationDeliveries::NextAttemptAt, next_attempt_at)
        .value(NotificationDeliveries::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .and_where(lease_guard(id, worker_id))
        .to_owned()
}

/// Push the attempt count past the ceiling and release the lease for good.
fn mark_exhausted_statement(id: i64, worker_id: &str, failure: &str) -> UpdateStatement {
    Query::update()
        .table(NotificationDeliveries::Table)
        .value(NotificationDeliveries::AttemptCount, EXHAUSTED_ATTEMPT_COUNT)
        .value(NotificationDeliveries::DeliveryError, failure)
        .value(NotificationDeliveries::ClaimedAt, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::ClaimedBy, SimpleExpr::Keyword(Keyword::Null))
        .value(NotificationDeliveries::LeaseUntil, SimpleExpr::Keyword(Keyword::Null))
        .and_where(lease_guard(id, worker_id))
        .to_owned()
}

/// Decide whether a lease-guarded update reached the delivery this worker owned.
///
/// The guarded predicate matches at most one row, so a zero count means the
/// worker's lease had expired and another worker holds the delivery.
const fn acknowledgement(rows_affected: u64) -> LeaseAcknowledgement {
    if rows_affected == 0 {
        LeaseAcknowledgement::Lost
    } else {
        LeaseAcknowledgement::Applied
    }
}

#[cfg(test)]
mod tests {
    use crate::notification_delivery::acknowledge::{acknowledgement, mark_delivered_statement, mark_exhausted_statement, mark_failed_statement};
    use andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement;
    use chrono::Utc;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn an_update_that_reached_no_row_reports_the_lease_as_lost() {
        pretty_assertions::assert_eq!(acknowledgement(0), LeaseAcknowledgement::Lost);
    }

    #[test]
    fn an_update_that_reached_its_row_reports_the_lease_as_held() {
        pretty_assertions::assert_eq!(acknowledgement(1), LeaseAcknowledgement::Applied);
    }

    #[test]
    fn every_acknowledgement_is_guarded_by_the_live_lease_it_was_taken_under() {
        for sql in [
            mark_delivered_statement(1, "w").to_string(PostgresQueryBuilder),
            mark_failed_statement(1, "w", "boom", Utc::now()).to_string(PostgresQueryBuilder),
            mark_exhausted_statement(1, "w", "boom").to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#""id" = 1"#), "{sql}");
            assert!(sql.contains(r#""claimed_by" = 'w'"#), "{sql}");
            assert!(sql.contains(r#""lease_until" > CURRENT_TIMESTAMP"#), "{sql}");
        }
    }

    #[test]
    fn a_delivered_row_keeps_its_claim_triple() {
        let sql = mark_delivered_statement(1, "w").to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""delivered_at" = CURRENT_TIMESTAMP"#), "{sql}");
        assert!(!sql.contains(r#""claimed_by" = NULL"#), "{sql}");
    }

    #[test]
    fn a_failed_row_counts_the_attempt_and_releases_the_lease() {
        let sql = mark_failed_statement(1, "w", "boom", Utc::now()).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""attempt_count" = "attempt_count" + 1"#), "{sql}");
        assert!(sql.contains(r#""claimed_at" = NULL"#), "{sql}");
        assert!(sql.contains(r#""claimed_by" = NULL"#), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
    }

    #[test]
    fn an_exhausted_row_pushes_the_counter_past_the_ceiling() {
        let sql = mark_exhausted_statement(1, "w", "boom").to_string(PostgresQueryBuilder);
        assert!(sql.contains(&format!(r#""attempt_count" = {}"#, i32::MAX)), "{sql}");
        assert!(sql.contains(r#""lease_until" = NULL"#), "{sql}");
    }
}
